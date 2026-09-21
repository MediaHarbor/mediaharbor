use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::LazyLock;

use crate::defaults::Settings;
use crate::downloads::dedup::DedupLedger;
use crate::errors::{MhError, MhResult};
use crate::services::common::pipeline::{
    AlbumInfo, CoverCache, PlaylistInfo, TrackOutcome, TrackPlacement, TrackRow,
};
use crate::services::common::playlist_file::{
    entry_from_tagged_file, write_playlist_file, PlaylistEntry,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Deezer,
    Qobuz,
    Tidal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentType {
    Track,
    Album,
    Playlist,
    Artist,
    Label,
    Video,
}

fn extract_id(url: &str, patterns: &[Regex]) -> Option<String> {
    for re in patterns {
        if let Some(caps) = re.captures(url) {
            if let Some(m) = caps.get(1) {
                return Some(m.as_str().to_string());
            }
        }
    }
    None
}

pub fn detect_platform_and_type(url: &str) -> Option<(Platform, ContentType)> {
    let platform = if url.contains("deezer.com") {
        Platform::Deezer
    } else if url.contains("qobuz.com") {
        Platform::Qobuz
    } else if url.contains("tidal.com") {
        Platform::Tidal
    } else {
        return None;
    };

    let content_type = if url.contains("/video/") {
        ContentType::Video
    } else if url.contains("/artist/") || url.contains("/interpreter/") {
        ContentType::Artist
    } else if url.contains("/label/") {
        ContentType::Label
    } else if url.contains("/track/") {
        ContentType::Track
    } else if url.contains("/album/") {
        ContentType::Album
    } else if url.contains("/playlist/") {
        ContentType::Playlist
    } else if !url.is_empty() && url.bytes().all(|b| b.is_ascii_digit()) {
        ContentType::Track
    } else {
        return None;
    };

    Some((platform, content_type))
}

/// The download URL to actually work with, plus what it is. A share link
/// (`link.deezer.com/s/…`) carries no `/track/` segment for `detect_platform_and_type`
/// to read, so it is resolved through one redirect-following GET first.
pub async fn detect_or_resolve(url: &str) -> MhResult<(String, Platform, ContentType)> {
    if let Some((platform, content_type)) = detect_platform_and_type(url) {
        return Ok((url.to_string(), platform, content_type));
    }

    use crate::services::common::share_links::{canonical_pipeline_url, resolve_share_url};
    let resolved = resolve_share_url(url).await.map_err(|e| {
        MhError::Other(format!(
            "Could not detect platform/type for URL: {url} — {e}"
        ))
    })?;
    let canonical = canonical_pipeline_url(&resolved).ok_or_else(|| {
        MhError::Other(format!(
            "{url} resolves to {:?} {:?} {} — MediaHarbor downloads that through a \
             different engine, so paste its own link instead",
            resolved.platform, resolved.kind, resolved.id
        ))
    })?;
    let (platform, content_type) = detect_platform_and_type(&canonical).ok_or_else(|| {
        MhError::Other(format!(
            "{url} resolved to {canonical}, which is still not a downloadable link"
        ))
    })?;
    Ok((canonical, platform, content_type))
}

/// Every URL shape the three pipeline services publish, compiled once.
///
/// The tables are fixed at build time, but `extract_platform_id` sits on the
/// playback path — the Deezer and Qobuz playback providers call it on every track
/// change — so building a fresh `Vec<Regex>` per call put the regex compiler in the
/// middle of starting a song.
static ID_PATTERNS: LazyLock<HashMap<(Platform, ContentType), Vec<Regex>>> = LazyLock::new(|| {
    use ContentType::*;
    use Platform::*;
    let table: &[(Platform, ContentType, &[&str])] = &[
        (
            Deezer,
            Track,
            &[r"deezer\.com/(?:[a-z]{2}/)?track/(\d+)", r"^(\d+)$"],
        ),
        (Deezer, Album, &[r"deezer\.com/(?:[a-z]{2}/)?album/(\d+)"]),
        (
            Deezer,
            Playlist,
            &[r"deezer\.com/(?:[a-z]{2}/)?playlist/(\d+)"],
        ),
        (Deezer, Artist, &[r"deezer\.com/(?:[a-z]{2}/)?artist/(\d+)"]),
        (
            Qobuz,
            Track,
            &[
                r"(?:play|open)\.qobuz\.com/track/(\w+)",
                r"qobuz\.com/[a-z-]+/album/[^/]+/(\d+)",
                r"^(\d+)$",
            ],
        ),
        (
            Qobuz,
            Album,
            &[
                r"(?:play|open)\.qobuz\.com/album/(\w+)",
                r"qobuz\.com/[a-z-]+/album/[^/]+/(\w+)",
                r"^(\w+)$",
            ],
        ),
        (
            Qobuz,
            Playlist,
            &[
                r"(?:play|open)\.qobuz\.com/playlist/(\d+)",
                r"qobuz\.com/[a-z-]+/playlist/[^/]+/(\d+)",
                r"^(\d+)$",
            ],
        ),
        (
            Qobuz,
            Artist,
            &[
                r"(?:play|open)\.qobuz\.com/artist/(\d+)",
                r"qobuz\.com/[a-z-]+/interpreter/[^/]+/(\d+)",
                r"qobuz\.com/[a-z-]+/artist/(\d+)",
            ],
        ),
        (Qobuz, Label, &[r"qobuz\.com/[a-z-]+/label/[^/]+/(\d+)"]),
        (
            Tidal,
            Track,
            &[
                r"tidal\.com/(?:[a-z]{2}/)?(?:browse/)?track/(\d+)",
                r"^(\d+)$",
            ],
        ),
        (
            Tidal,
            Album,
            &[r"tidal\.com/(?:[a-z]{2}/)?(?:browse/)?album/(\d+)"],
        ),
        (
            Tidal,
            Playlist,
            &[r"tidal\.com/(?:[a-z]{2}/)?(?:browse/)?playlist/([a-z0-9-]+)"],
        ),
        (
            Tidal,
            Artist,
            &[r"tidal\.com/(?:[a-z]{2}/)?(?:browse/)?artist/(\d+)"],
        ),
        (
            Tidal,
            Video,
            &[r"tidal\.com/(?:[a-z]{2}/)?(?:browse/)?video/(\d+)"],
        ),
    ];
    table
        .iter()
        .map(|(platform, content_type, patterns)| {
            let compiled = patterns
                .iter()
                .filter_map(|p| Regex::new(p).ok())
                .collect::<Vec<_>>();
            ((*platform, *content_type), compiled)
        })
        .collect()
});

pub fn extract_platform_id(
    url: &str,
    platform: Platform,
    content_type: ContentType,
) -> Option<String> {
    let patterns = ID_PATTERNS.get(&(platform, content_type))?;
    extract_id(url, patterns)
}

use super::build_album_folder;
use crate::services::common::download::BatchOutcome;

pub type ProgressFn = Box<dyn Fn(u64, u64) + Send + Sync>;

/// Everything one track's download needs. A struct rather than a tenth positional
/// argument: the list had already outgrown readability at eight, and both the
/// record prefetched with the release and the run's shared cover cache had to join
/// it.
pub struct TrackJob<'a> {
    pub track_id: &'a str,
    pub quality: u8,
    pub dest: &'a Path,
    pub settings: &'a Settings,
    pub on_progress: ProgressFn,
    pub on_log: SharedLog,
    pub placement: TrackPlacement,
    pub dedup: Option<&'a DedupLedger>,
    /// The service's own record for this track, already fetched with the release.
    /// `None` for a bare track download, where there is no release to have fetched.
    pub record: Option<&'a Value>,
    /// The release payload the record came from, for the album-level fields a
    /// per-track endpoint nests under `album` but a release listing does not.
    pub release: Option<&'a Value>,
    pub covers: &'a CoverCache,
}

pub(crate) type SharedProgress = Arc<dyn Fn(u64, u64) + Send + Sync>;
pub(crate) type SharedLog = Arc<dyn Fn(String) + Send + Sync>;
/// Reports which item of a collection is starting, so the UI can show "3 / 12".
pub(crate) type SharedItem = Arc<dyn Fn(u32, u32, &str) + Send + Sync>;
/// Reports the quality the service actually served, so the download row can stop
/// showing the tier that was only requested.
pub(crate) type SharedQuality = Arc<dyn Fn(String) + Send + Sync>;

/// How often byte progress is allowed to reach the UI.
///
/// The callback below fires once per HTTP chunk — a few thousand times for one FLAC —
/// and each call ends in a serialized Tauri event and a React render. The heartbeat
/// already re-emits the last value every 300 ms, so nothing is lost by coalescing to
/// this interval; it only stops the transfer's chunk size from setting the UI's frame
/// rate.
const PROGRESS_EMIT_INTERVAL_MS: u64 = 100;

/// Maps one track's byte progress onto the whole batch, and folds its bytes into the
/// run-wide counter that transfer speed is computed from.
fn divided_progress<F>(
    track_index: usize,
    track_total: usize,
    bytes: Arc<crate::downloads::ByteProgress>,
    last: Arc<std::sync::atomic::AtomicU64>,
    parent: F,
) -> impl Fn(u64, u64) + Send + Sync + 'static
where
    F: Fn(u64, u64) + Send + Sync + 'static,
{
    use std::sync::atomic::{AtomicU64, Ordering};
    let scale: u64 = 1_000_000;
    let n = track_total.max(1) as u64;
    let offset = track_index as u64;
    let seen = AtomicU64::new(0);
    let started = std::time::Instant::now();
    let last_emit_ms = AtomicU64::new(u64::MAX);
    move |done, total| {
        let prev = seen.swap(done, Ordering::Relaxed);
        bytes
            .done
            .fetch_add(done.saturating_sub(prev), Ordering::Relaxed);
        let track_frac = (done * scale).checked_div(total).unwrap_or(0);
        let overall = (offset * scale + track_frac) / n;
        last.store(overall, Ordering::Relaxed);

        let now_ms = started.elapsed().as_millis() as u64;
        let previous = last_emit_ms.load(Ordering::Relaxed);
        let due =
            previous == u64::MAX || now_ms.saturating_sub(previous) >= PROGRESS_EMIT_INTERVAL_MS;
        if due || (total > 0 && done >= total) {
            last_emit_ms.store(now_ms, Ordering::Relaxed);
            parent(overall, scale);
        }
    }
}

/// The scale `divided_progress` reports against: a whole batch is `PROGRESS_SCALE`.
const PROGRESS_SCALE: u64 = 1_000_000;

/// Runs a track's work while re-emitting its last progress value on a timer.
///
/// Bytes are only part of a download: metadata lookups, lyrics, cover art, tagging
/// and re-encoding all move none. Without a heartbeat the bar and the speed/ETA
/// readout sit frozen through every one of them — which on a short track is most of
/// the wall-clock, and is why the pipeline looked stalled next to the native Apple
/// Music engine, whose own tick loop has always done this.
async fn with_heartbeat<T>(
    fut: impl std::future::Future<Output = T>,
    last: Arc<std::sync::atomic::AtomicU64>,
    on_progress: SharedProgress,
) -> T {
    use std::sync::atomic::Ordering;
    tokio::pin!(fut);
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(300));
    tick.tick().await;
    loop {
        tokio::select! {
            done = &mut fut => return done,
            _ = tick.tick() => on_progress(last.load(Ordering::Relaxed), PROGRESS_SCALE),
        }
    }
}

pub async fn download_url(
    url: &str,
    settings: &Settings,
    client: &dyn TrackSourceClient,
    on_progress: impl Fn(u64, u64) + Clone + Send + Sync + 'static,
    on_log: impl Fn(String) + Clone + Send + Sync + 'static,
    on_item: impl Fn(u32, u32, &str) + Clone + Send + Sync + 'static,
    on_quality: impl Fn(String) + Clone + Send + Sync + 'static,
    bytes: Arc<crate::downloads::ByteProgress>,
    dedup: Option<&DedupLedger>,
) -> MhResult<BatchOutcome> {
    let (resolved_url, platform, content_type) = detect_or_resolve(url).await?;
    let url: &str = &resolved_url;

    on_log(format!("Detected: {:?} {:?}", platform, content_type));

    let base_dir = if settings.create_platform_subfolders {
        PathBuf::from(&settings.download_location).join(client.platform_name())
    } else {
        PathBuf::from(&settings.download_location)
    };
    tokio::fs::create_dir_all(&base_dir).await?;

    let on_progress: SharedProgress = Arc::new(on_progress);
    let on_log: SharedLog = Arc::new(on_log);
    let on_item: SharedItem = Arc::new(on_item);
    let on_quality: SharedQuality = Arc::new(on_quality);

    download_collection(
        client,
        url,
        content_type,
        &base_dir,
        settings,
        on_progress,
        on_log,
        on_item,
        on_quality,
        bytes,
        dedup,
    )
    .await
}

#[async_trait::async_trait]
pub trait TrackSourceClient: Send + Sync {
    fn platform_name(&self) -> &'static str;

    fn requested_quality(&self, settings: &Settings) -> u8;

    fn extract_id(&self, url: &str, content_type: ContentType) -> Option<String>;

    fn album_url(&self, album_id: &str) -> String;

    fn album_quality_format(&self, album: &AlbumInfo, settings: &Settings) -> (String, String);

    fn playlist_quality_format(&self, settings: &Settings) -> (String, String);

    /// What the service will actually serve, asked once before the folder is named.
    /// The requested tier is only a preference: naming a release after it is how a
    /// hi-res request answered with `LOSSLESS` ended up in a `24-bit` folder.
    /// `None` falls back to the requested-tier guess.
    async fn probe_release_quality(
        &self,
        _first_track_id: &str,
        _quality: u8,
        _settings: &Settings,
    ) -> Option<(String, String)> {
        None
    }

    async fn get_album_tracks(&self, album_id: &str) -> MhResult<AlbumInfo>;
    async fn get_playlist_tracks(&self, playlist_id: &str) -> MhResult<PlaylistInfo>;
    async fn get_artist_albums(
        &self,
        artist_id: &str,
        settings: &Settings,
    ) -> MhResult<Vec<String>>;

    async fn download_track(&self, job: TrackJob<'_>) -> MhResult<TrackOutcome>;

    async fn after_album(&self, _album_id: &str, _dest_dir: &Path, _settings: &Settings) {}

    #[allow(clippy::too_many_arguments)]
    async fn download_label(
        &self,
        url: &str,
        base_dir: &Path,
        settings: &Settings,
        on_progress: SharedProgress,
        on_log: SharedLog,
        on_item: SharedItem,
        on_quality: SharedQuality,
        bytes: Arc<crate::downloads::ByteProgress>,
        dedup: Option<&DedupLedger>,
    ) -> MhResult<()>;

    async fn download_video(
        &self,
        url: &str,
        base_dir: &Path,
        settings: &Settings,
        on_progress: SharedProgress,
        on_log: SharedLog,
        dedup: Option<&DedupLedger>,
    ) -> MhResult<()>;
}

pub(crate) fn extract_or_err<C: TrackSourceClient + ?Sized>(
    client: &C,
    url: &str,
    content_type: ContentType,
) -> MhResult<String> {
    let word = match content_type {
        ContentType::Track => "track",
        ContentType::Album => "album",
        ContentType::Playlist => "playlist",
        ContentType::Artist => "artist",
        ContentType::Label => "label",
        ContentType::Video => "video",
    };
    client.extract_id(url, content_type).ok_or_else(|| {
        MhError::Other(format!(
            "Could not extract {} {} ID from: {}",
            client.platform_name(),
            word,
            url
        ))
    })
}

/// Opens the folder a collection lands in and records it on the outcome.
async fn open_collection_dir(
    dest_dir: &Path,
    track_count: usize,
    outcome: &mut BatchOutcome,
) -> MhResult<()> {
    tokio::fs::create_dir_all(dest_dir).await?;
    outcome.dest_dir = Some(dest_dir.to_string_lossy().into_owned());
    outcome.total = track_count as u32;
    Ok(())
}

/// Writes the collection's `.m3u8`, when the setting is on and anything landed.
async fn write_collection_playlist(
    dest_dir: &Path,
    title: &str,
    entries: &[PlaylistEntry],
    settings: &Settings,
    on_log: &SharedLog,
) {
    if settings.save_playlist_file && !entries.is_empty() {
        write_playlist_file(dest_dir, title, entries, |m| on_log(m)).await;
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn download_collection(
    client: &dyn TrackSourceClient,
    url: &str,
    content_type: ContentType,
    base_dir: &Path,
    settings: &Settings,
    on_progress: SharedProgress,
    on_log: SharedLog,
    on_item: SharedItem,
    on_quality: SharedQuality,
    bytes: Arc<crate::downloads::ByteProgress>,
    dedup: Option<&DedupLedger>,
) -> MhResult<BatchOutcome> {
    let quality = client.requested_quality(settings);
    let covers = CoverCache::default();
    let mut outcome = BatchOutcome::default();
    match content_type {
        ContentType::Track => {
            let id = extract_or_err(client, url, ContentType::Track)?;
            on_log(format!("Track: {}", id));
            tokio::fs::create_dir_all(base_dir).await?;
            outcome.total = 1;
            outcome.dest_dir = Some(base_dir.to_string_lossy().into_owned());
            on_item(1, 1, "");
            let last = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let track = with_heartbeat(
                client.download_track(TrackJob {
                    track_id: &id,
                    quality,
                    dest: base_dir,
                    settings,
                    on_progress: Box::new(divided_progress(
                        0,
                        1,
                        bytes.clone(),
                        last.clone(),
                        progress_to_fn(on_progress.clone()),
                    )),
                    on_log: on_log.clone(),
                    placement: TrackPlacement::Loose,
                    dedup,
                    record: None,
                    release: None,
                    covers: &covers,
                }),
                last.clone(),
                on_progress.clone(),
            )
            .await?;
            outcome.record(&track);
        }
        ContentType::Album => {
            let id = extract_or_err(client, url, ContentType::Album)?;
            let resolving = Arc::new(std::sync::atomic::AtomicU64::new(0));
            on_item(0, 0, "Resolving release…");
            let album_info = with_heartbeat(
                client.get_album_tracks(&id),
                resolving.clone(),
                on_progress.clone(),
            )
            .await?;
            let (album_quality, album_format) = probed_quality(
                client,
                album_info.tracks.first(),
                quality,
                settings,
                &on_log,
                &on_quality,
            )
            .await
            .unwrap_or_else(|| client.album_quality_format(&album_info, settings));
            let dest_dir = make_collection_dir(
                base_dir,
                settings,
                CollectionNaming {
                    title: &album_info.title,
                    artist: &album_info.artist,
                    year: &album_info.year,
                    genre: &album_info.genre,
                    label: &album_info.label,
                    quality: &album_quality,
                    format: &album_format,
                },
            );
            let total = album_info.tracks.len();
            open_collection_dir(&dest_dir, total, &mut outcome).await?;
            on_log(format!(
                "Album: {} — {} ({} tracks)",
                album_info.title, album_info.artist, total
            ));
            let disc_dirs = settings.disc_subdirectories
                && (album_info.number_of_volumes > 1
                    || album_info.tracks.iter().any(|t| t.disc > 1));
            let entries = download_track_sequence(
                client,
                TrackSequence {
                    tracks: &album_info.tracks,
                    dest_dir: &dest_dir,
                    disc_dirs,
                    placement: TrackPlacement::Release,
                    release: Some(&album_info.album_record),
                },
                SequenceCtx {
                    quality,
                    settings,
                    covers: &covers,
                    dedup,
                    bytes: bytes.clone(),
                    on_progress: on_progress.clone(),
                    on_log: on_log.clone(),
                    on_item: on_item.clone(),
                },
                &mut outcome,
            )
            .await;
            write_collection_playlist(&dest_dir, &album_info.title, &entries, settings, &on_log)
                .await;
            client.after_album(&id, &dest_dir, settings).await;
        }
        ContentType::Playlist => {
            let id = extract_or_err(client, url, ContentType::Playlist)?;
            let resolving = Arc::new(std::sync::atomic::AtomicU64::new(0));
            on_item(0, 0, "Resolving playlist…");
            let info = with_heartbeat(
                client.get_playlist_tracks(&id),
                resolving.clone(),
                on_progress.clone(),
            )
            .await?;
            let (playlist_quality, playlist_format) = probed_quality(
                client,
                info.tracks.first(),
                quality,
                settings,
                &on_log,
                &on_quality,
            )
            .await
            .unwrap_or_else(|| client.playlist_quality_format(settings));
            let dest_dir = make_collection_dir(
                base_dir,
                settings,
                CollectionNaming {
                    title: &info.title,
                    artist: &info.artist,
                    year: "",
                    genre: "",
                    label: "",
                    quality: &playlist_quality,
                    format: &playlist_format,
                },
            );
            let total = info.tracks.len();
            open_collection_dir(&dest_dir, total, &mut outcome).await?;
            on_log(format!("Playlist: {} ({} tracks)", info.title, total));
            let entries = download_track_sequence(
                client,
                TrackSequence {
                    tracks: &info.tracks,
                    dest_dir: &dest_dir,
                    disc_dirs: false,
                    placement: TrackPlacement::Playlist,
                    release: None,
                },
                SequenceCtx {
                    quality,
                    settings,
                    covers: &covers,
                    dedup,
                    bytes: bytes.clone(),
                    on_progress: on_progress.clone(),
                    on_log: on_log.clone(),
                    on_item: on_item.clone(),
                },
                &mut outcome,
            )
            .await;
            write_collection_playlist(&dest_dir, &info.title, &entries, settings, &on_log).await;
        }
        ContentType::Artist => {
            let id = extract_or_err(client, url, ContentType::Artist)?;
            let album_ids = client.get_artist_albums(&id, settings).await?;
            on_log(format!("Artist: {} albums", album_ids.len()));
            for album_id in &album_ids {
                let album_url = client.album_url(album_id);
                match Box::pin(download_collection(
                    client,
                    &album_url,
                    ContentType::Album,
                    base_dir,
                    settings,
                    on_progress.clone(),
                    on_log.clone(),
                    on_item.clone(),
                    on_quality.clone(),
                    bytes.clone(),
                    dedup,
                ))
                .await
                {
                    Ok(album) => outcome.absorb(album),
                    Err(e) => {
                        on_log(format!("  album {} failed: {}", album_id, e));
                        outcome.total += 1;
                        outcome.record_failure(album_id, album_id, e);
                    }
                }
            }
        }
        ContentType::Label => {
            client
                .download_label(
                    url,
                    base_dir,
                    settings,
                    on_progress,
                    on_log,
                    on_item,
                    on_quality,
                    bytes,
                    dedup,
                )
                .await?;
        }
        ContentType::Video => {
            outcome.total = 1;
            on_item(1, 1, "");
            client
                .download_video(url, base_dir, settings, on_progress, on_log, dedup)
                .await?;
            outcome.record_success();
        }
    }
    Ok(outcome)
}

/// Asks the service what it will really serve, and says so when the answer differs
/// from what the fallback would have guessed. A probe failure is not fatal — the
/// folder just falls back to the requested tier, exactly as before.
async fn probed_quality<C: TrackSourceClient + ?Sized>(
    client: &C,
    first_track: Option<&TrackRow>,
    quality: u8,
    settings: &Settings,
    on_log: &SharedLog,
    on_quality: &SharedQuality,
) -> Option<(String, String)> {
    let id = &first_track?.id;
    let probed = client.probe_release_quality(id, quality, settings).await;
    if let Some((label, format)) = probed.as_ref() {
        on_quality(format!("{label} · {format}"));
    } else {
        on_log(
            "  · could not confirm the served quality up front; naming this release after the \
             requested tier"
                .to_string(),
        );
    }
    probed
}

/// What a run of tracks shares: where they land and how they are tagged.
struct TrackSequence<'a> {
    tracks: &'a [TrackRow],
    dest_dir: &'a Path,
    disc_dirs: bool,
    placement: TrackPlacement,
    release: Option<&'a Value>,
}

/// The plumbing every track in a sequence needs, threaded through unchanged.
struct SequenceCtx<'a> {
    quality: u8,
    settings: &'a Settings,
    covers: &'a CoverCache,
    dedup: Option<&'a DedupLedger>,
    bytes: Arc<crate::downloads::ByteProgress>,
    on_progress: SharedProgress,
    on_log: SharedLog,
    on_item: SharedItem,
}

/// Downloads every track of a release or playlist in order, recording each
/// result on `outcome`. A track that fails is logged and skipped, never fatal.
async fn download_track_sequence(
    client: &dyn TrackSourceClient,
    seq: TrackSequence<'_>,
    ctx: SequenceCtx<'_>,
    outcome: &mut BatchOutcome,
) -> Vec<PlaylistEntry> {
    let total = seq.tracks.len();
    let mut entries: Vec<PlaylistEntry> = Vec::new();

    for (i, row) in seq.tracks.iter().enumerate() {
        let track_dir = if seq.disc_dirs {
            let d = seq.dest_dir.join(format!("Disc {}", row.disc));
            let _ = tokio::fs::create_dir_all(&d).await;
            d
        } else {
            seq.dest_dir.to_path_buf()
        };
        let label = track_label_of(row);
        (ctx.on_log)(format!("Track {}/{}: {}", i + 1, total, label));
        (ctx.on_item)((i + 1) as u32, total as u32, &label);
        let last = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let p = divided_progress(
            i,
            total,
            ctx.bytes.clone(),
            last.clone(),
            progress_to_fn(ctx.on_progress.clone()),
        );
        match with_heartbeat(
            client.download_track(TrackJob {
                track_id: &row.id,
                quality: ctx.quality,
                dest: &track_dir,
                settings: ctx.settings,
                on_progress: Box::new(p),
                on_log: ctx.on_log.clone(),
                placement: seq.placement,
                dedup: ctx.dedup,
                record: Some(&row.record),
                release: seq.release,
                covers: ctx.covers,
            }),
            last.clone(),
            ctx.on_progress.clone(),
        )
        .await
        {
            Ok(track) => {
                outcome.record(&track);
                entries.push(entry_from_tagged_file(track.into_path()));
            }
            Err(e) => {
                (ctx.on_log)(format!("  failed: {}", e));
                outcome.record_failure(&row.id, &row.id, e);
            }
        }
    }
    entries
}

/// The display name for a track, falling back to its id.
fn track_label_of(row: &TrackRow) -> String {
    let label = row.label.trim();
    if label.is_empty() {
        row.id.clone()
    } else {
        label.to_string()
    }
}

pub(crate) fn progress_to_fn(p: SharedProgress) -> impl Fn(u64, u64) + Send + Sync + 'static {
    move |done, total| p(done, total)
}

/// The fields that name a release folder. A struct rather than seven
/// positional `&str`s, any two of which could be swapped and still compile.
struct CollectionNaming<'a> {
    title: &'a str,
    artist: &'a str,
    year: &'a str,
    genre: &'a str,
    label: &'a str,
    quality: &'a str,
    format: &'a str,
}

fn make_collection_dir(base: &Path, settings: &Settings, naming: CollectionNaming<'_>) -> PathBuf {
    let (quality, format) =
        super::converted_quality_format(settings, naming.quality, naming.format);
    let folder_name = build_album_folder(
        &settings.filepaths_folder_format,
        naming.artist,
        naming.title,
        naming.year,
        naming.genre,
        naming.label,
        &quality,
        &format,
        super::FolderNaming::from_settings(settings),
    );
    base.join(folder_name)
}

#[cfg(test)]
mod url_tests {
    use super::{detect_platform_and_type, extract_platform_id, ContentType, Platform};

    fn resolve(url: &str) -> Option<(Platform, ContentType, String)> {
        let (p, c) = detect_platform_and_type(url)?;
        let id = extract_platform_id(url, p, c)?;
        Some((p, c, id))
    }

    /// The Qobuz web player answers on both hosts, but only `play.` was ever matched:
    /// an `open.qobuz.com` link was detected as Qobuz and then yielded no id, so the
    /// download failed after the UI had already said "Detected: Qobuz Album".
    #[test]
    fn both_qobuz_web_player_hosts_resolve() {
        for host in ["play.qobuz.com", "open.qobuz.com"] {
            assert_eq!(
                resolve(&format!("https://{host}/album/0060254706622")),
                Some((
                    Platform::Qobuz,
                    ContentType::Album,
                    "0060254706622".to_string()
                )),
                "{host} album"
            );
            assert_eq!(
                resolve(&format!("https://{host}/track/12345678")),
                Some((Platform::Qobuz, ContentType::Track, "12345678".to_string())),
                "{host} track"
            );
            assert_eq!(
                resolve(&format!("https://{host}/playlist/987654")),
                Some((Platform::Qobuz, ContentType::Playlist, "987654".to_string())),
                "{host} playlist"
            );
            assert_eq!(
                resolve(&format!("https://{host}/artist/2070")),
                Some((Platform::Qobuz, ContentType::Artist, "2070".to_string())),
                "{host} artist"
            );
        }
    }

    /// The storefront URLs carry a locale and a slug and must keep working.
    #[test]
    fn the_qobuz_storefront_shape_still_resolves() {
        assert_eq!(
            resolve("https://www.qobuz.com/us-en/album/some-slug/0060254706622"),
            Some((
                Platform::Qobuz,
                ContentType::Album,
                "0060254706622".to_string()
            ))
        );
        assert_eq!(
            resolve("https://www.qobuz.com/us-en/interpreter/some-artist/2070"),
            Some((Platform::Qobuz, ContentType::Artist, "2070".to_string()))
        );
    }
}

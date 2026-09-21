use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::RwLock;

use crate::defaults::Settings;
use crate::downloads::dedup::{quality_rank, DedupLedger};
use crate::downloads::native_common::{
    cover_for_track, dest_dir_for, disc_dir_for, native_track_name, remux_or_keep,
    run_progress_tick_loop, settle_cover_file, NativeTrack, ReleaseFolder, TickSlice,
};
use crate::downloads::BatchProgress;
use crate::errors::{MhError, MhResult};
use crate::services::common::download::BatchOutcome;
use crate::services::common::lyrics::{
    lyrics_wanted, prefers_ttml, resolve_track_lyrics, write_chosen_sidecar, write_sidecars,
    FoundLyrics,
};
use crate::services::common::pipeline::converter::{AudioCodec, ConversionSource};
use crate::services::common::pipeline::downloader::{
    excluded_tags, existing_final_file_for, explicit_suffix, maybe_convert, short_rip_reason,
    CommonTrackFields,
};
use crate::services::common::pipeline::tagger::{one, tag_file, TrackMetadata};
use crate::services::common::playlist_file::{write_playlist_file, PlaylistEntry};
use crate::services::spotify::lyrics::fetch_color_lyrics;
use crate::services::spotify::session::{
    LibrespotService, SpotifyAlbumDownloadMeta, SpotifyTrackDownloadMeta,
};

const SUPPORTED_QUALITIES: &[&str] = &["aac-high", "aac-medium"];

const SPOTIFY_RATE_LIMIT_ABORT_STREAK: u32 = 2;

const SPOTIFY_FREE_RECOVERY_MIN: u64 = 4;

pub fn is_supported_quality(q: &str) -> bool {
    SUPPORTED_QUALITIES.contains(&q)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotifyLinkKind {
    Track,
    Album,
    Playlist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotifyUrlSupport {
    Supported(SpotifyLinkKind),
    Unsupported(&'static str),
    NotSpotify,
}

pub fn classify_spotify_url(url: &str) -> SpotifyUrlSupport {
    use SpotifyUrlSupport::*;
    let lower = url.to_lowercase();
    let is_spotify = lower.contains("open.spotify.com") || lower.starts_with("spotify:");
    if !is_spotify {
        return NotSpotify;
    }
    for kind in ["episode", "show", "podcast"] {
        if url.contains(&format!("/{kind}/")) || url.starts_with(&format!("spotify:{kind}:")) {
            return Unsupported("podcast / episode");
        }
    }
    if url.contains("/video/") || url.starts_with("spotify:video:") {
        return Unsupported("music video");
    }
    if url.contains("/audiobook/") || url.contains("/chapter/") {
        return Unsupported("audiobook");
    }
    if url.contains("/artist/") || url.starts_with("spotify:artist:") {
        return Unsupported("artist top-tracks");
    }
    match parse_spotify_url(url) {
        Some((kind, _)) => match kind.as_str() {
            "track" => Supported(SpotifyLinkKind::Track),
            "album" => Supported(SpotifyLinkKind::Album),
            "playlist" => Supported(SpotifyLinkKind::Playlist),
            _ => Unsupported("unknown link kind"),
        },
        None => Unsupported("unrecognised URL"),
    }
}

pub fn parse_spotify_url(url: &str) -> Option<(String, String)> {
    for kind in &["track", "album", "playlist"] {
        let uri_prefix = format!("spotify:{}:", kind);
        if let Some(rest) = url.strip_prefix(&uri_prefix) {
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if !id.is_empty() {
                return Some(((*kind).to_string(), id));
            }
        }
        let needle = format!("/{}/", kind);
        if let Some(idx) = url.find(&needle) {
            let after = &url[idx + needle.len()..];
            let id: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if !id.is_empty() {
                return Some(((*kind).to_string(), id));
            }
        }
    }
    None
}

fn extension_for_content_type(_ct: &str) -> &'static str {
    "m4a"
}

fn parse_bitrate_param(ct: &str) -> Option<u32> {
    let lower = ct.to_ascii_lowercase();
    let needle = "bitrate=";
    let start = lower.find(needle)? + needle.len();
    let tail = &lower[start..];
    let end = tail
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(tail.len());
    tail[..end].parse::<u32>().ok().map(|bps| bps / 1000)
}

fn labels_for_content_type(
    ct: &str,
    requested: &str,
    fallback_quality_label: &str,
    fallback_format_label: &str,
) -> (String, String) {
    let lower = ct.to_ascii_lowercase();
    let delivered_kbps = parse_bitrate_param(ct);
    if lower.contains("mp4") || lower.contains("aac") {
        let q = match (delivered_kbps, requested) {
            (Some(k), _) => format!("{} kbps", k),
            (None, "aac-medium") => "128 kbps".into(),
            _ => "256 kbps".into(),
        };
        (q, "AAC".into())
    } else {
        (
            fallback_quality_label.to_string(),
            fallback_format_label.to_string(),
        )
    }
}

fn ffmpeg_muxer_for_ext(ext: &str) -> &'static str {
    match ext {
        "ogg" => "ogg",
        "mp3" => "mp3",
        "m4a" => "mp4",
        _ => "mp4",
    }
}

fn non_empty(s: &str) -> Option<String> {
    (!s.trim().is_empty()).then(|| s.to_string())
}

fn spotify_name_fields(
    meta: &SpotifyTrackDownloadMeta,
    primary_artist: &str,
    quality_label: &str,
    format_label: &str,
) -> CommonTrackFields {
    CommonTrackFields {
        title: meta.title.clone(),
        artist: primary_artist.to_string(),
        albumartist: meta.album_artist.clone(),
        album: meta.album.clone(),
        track_num: meta.track_number.unwrap_or(0),
        disc_num: meta.disc_number.unwrap_or(1),
        tracktotal: meta.total_tracks.map(|n| n.to_string()).unwrap_or_default(),
        disctotal: meta.disc_total.map(|n| n.to_string()).unwrap_or_default(),
        year: meta.year.clone().unwrap_or_default(),
        genre: meta.genre.clone().unwrap_or_default(),
        explicit: explicit_suffix(meta.explicit),
        isrc: meta.isrc.clone().unwrap_or_default(),
        label: meta.label.clone().unwrap_or_default(),
        date: meta
            .date
            .clone()
            .or_else(|| meta.year.clone())
            .unwrap_or_default(),
        quality_label: quality_label.to_string(),
        format_label: format_label.to_string(),
        composer: String::new(),
    }
}

impl NativeTrack for SpotifyTrackDownloadMeta {
    fn track_metadata(&self) -> TrackMetadata {
        TrackMetadata {
            title: non_empty(&self.title),
            artist: self.artists.clone(),
            album: non_empty(&self.album),
            album_artist: one(non_empty(&self.album_artist)),
            year: self.date.clone().or_else(|| self.year.clone()),
            genre: one(self.genre.clone()),
            track_number: self.track_number,
            disc_number: self.disc_number,
            total_tracks: self.total_tracks,
            total_discs: self.disc_total,
            isrc: self.isrc.clone(),
            upc: self.upc.clone(),
            copyright: self.copyright.clone(),
            label: self.label.clone(),
            ..Default::default()
        }
    }
}

/// Fetches Spotify's timed lyrics for one track. The error is returned rather than
/// logged here so the caller can attribute it to the track it was downloading.
async fn spotify_lyrics_for(
    librespot: &Arc<RwLock<LibrespotService>>,
    track_id: &str,
) -> MhResult<Option<crate::services::common::lyrics::WordLyrics>> {
    let Some(token) = librespot.read().await.cached_access_token() else {
        return Err(MhError::Auth(
            "no Spotify access token cached; cannot fetch lyrics".into(),
        ));
    };
    fetch_color_lyrics(&token, track_id).await
}

async fn resolve_track_ids_for_url(
    librespot: &Arc<RwLock<LibrespotService>>,
    user_data: &Path,
    url: &str,
) -> MhResult<(
    Vec<String>,
    Option<SpotifyAlbumDownloadMeta>,
    Option<String>,
)> {
    let (kind, id) = parse_spotify_url(url)
        .ok_or_else(|| MhError::Other(format!("Unrecognised Spotify URL: {url}")))?;
    match kind.as_str() {
        "track" => Ok((vec![id], None, None)),
        "album" => {
            let mut svc = librespot.write().await;
            let album = svc.get_album_download_metadata(&id).await?;
            let ids = album.track_ids.clone();
            Ok((ids, Some(album), None))
        }
        "playlist" => {
            let library = crate::services::spotify::library::SpotifyLibrary::from_parts(
                librespot.clone(),
                user_data,
            )?;
            let (name, ids) = library.playlist_track_ids(&id).await?;
            Ok((ids, None, Some(name)))
        }

        other => Err(MhError::Unsupported(format!(
            "Native Spotify backend can't download {other} links."
        ))),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn download_with_native_spotify(
    settings: &Settings,
    librespot: Arc<RwLock<LibrespotService>>,
    license_limiter: std::sync::Arc<crate::services::spotify::rate_limit::LicenseRateLimiter>,
    venv_python: Option<PathBuf>,
    user_data: PathBuf,
    url: &str,
    quality: Option<&str>,
    on_progress: impl Fn(BatchProgress) + Send + 'static,
    on_log: impl Fn(String) + Send + 'static,
    cancel: Arc<AtomicBool>,
    dedup: Option<&DedupLedger>,
) -> MhResult<BatchOutcome> {
    if let SpotifyUrlSupport::Unsupported(kind) = classify_spotify_url(url) {
        return Err(MhError::Unsupported(format!(
            "Native Spotify downloader can't handle {kind} links. \
             Open Settings → Spotify → Downloader and switch to Votify to download this link."
        )));
    }

    let quality = quality
        .filter(|q| is_supported_quality(q))
        .unwrap_or_else(|| {
            let s: &str = &settings.spotify_native_quality;
            if is_supported_quality(s) {
                s
            } else {
                "aac-high"
            }
        })
        .to_string();

    on_log(format!("Native Spotify: resolving {url}"));

    let (track_ids, album_meta, playlist_name) =
        resolve_track_ids_for_url(&librespot, &user_data, url).await?;
    if track_ids.is_empty() {
        return Err(MhError::Other(
            "No tracks found at this Spotify URL.".into(),
        ));
    }
    let total = track_ids.len();
    on_log(format!("Native Spotify: {} track(s) queued", total));

    let format_label = "AAC";
    let quality_label = match quality.as_str() {
        "aac-high" => "256 kbps",
        "aac-medium" => "128 kbps",
        _ => "Spotify",
    };

    let base_dir = if settings.create_platform_subfolders {
        Path::new(&settings.download_location).join("Spotify")
    } else {
        PathBuf::from(&settings.download_location)
    };
    let is_collection = track_ids.len() > 1;

    let mut dest_dir = match (is_collection, &album_meta, &playlist_name) {
        (true, Some(album), _) => dest_dir_for(
            settings,
            &base_dir,
            &ReleaseFolder {
                album_artist: &album.artist,
                album: &album.title,
                year: album.year.as_deref().unwrap_or(""),
                label: album.label.as_deref().unwrap_or(""),
                quality_label,
                format: format_label,
                ..Default::default()
            },
        ),
        (true, None, Some(name)) if !name.is_empty() => dest_dir_for(
            settings,
            &base_dir,
            &ReleaseFolder {
                album: name,
                quality_label,
                format: format_label,
                ..Default::default()
            },
        ),
        _ => base_dir.clone(),
    };
    tokio::fs::create_dir_all(&dest_dir).await?;

    let http = crate::http_client::build_ua_client("MediaHarbor/native-spotify")?;

    let mut cover_cache: std::collections::HashMap<String, Option<Bytes>> =
        std::collections::HashMap::new();
    let mut shared_cover_path: Option<PathBuf> = None;
    let exclude_tags = excluded_tags(settings);
    let mut outcome = BatchOutcome::new(total);
    let mut playlist_entries: Vec<PlaylistEntry> = Vec::new();
    let mut consecutive_rate_limits = 0u32;
    let requested_rank = quality_rank("spotify", &quality);

    for (i, tid) in track_ids.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(MhError::Cancelled);
        }
        let completed = outcome.settled();
        if let Some(d) = dedup {
            if d.should_skip("spotify", tid, requested_rank) {
                on_log(format!(
                    "[{}/{}] skipped — already downloaded",
                    i + 1,
                    total
                ));
                outcome.record_skip();
                on_progress(BatchProgress::item_settled(
                    i,
                    total,
                    String::new(),
                    outcome.settled(),
                ));
                continue;
            }
        }
        let track_meta_res = {
            let mut svc = librespot.write().await;
            svc.get_track_download_metadata(tid).await
        };
        let mut track_meta = match track_meta_res {
            Ok(m) => m,
            Err(e) => {
                on_log(format!("  ✗ metadata fetch failed for {tid}: {e}"));
                outcome.record_failure(tid, tid, e);
                continue;
            }
        };
        if let Some(album) = &album_meta {
            track_meta.cover_url = track_meta
                .cover_url
                .clone()
                .or_else(|| album.cover_url.clone());
            if track_meta.label.is_none() {
                track_meta.label = album.label.clone();
            }
        }

        let label = format!(
            "{} - {}",
            track_meta.artists.first().map(String::as_str).unwrap_or(""),
            track_meta.title,
        );
        on_log(format!("[{}/{}] {}", i + 1, total, label));
        on_log(format!(
            "  meta source=gid title='{}' artist='{}' cover={}",
            track_meta.title,
            track_meta.artists.first().map(String::as_str).unwrap_or(""),
            if track_meta.cover_url.is_some() {
                "yes"
            } else {
                "no"
            },
        ));
        on_progress(BatchProgress::item_started(
            i,
            total,
            label.clone(),
            completed,
        ));

        if settings.spotify_synced_lyrics_only {
            let primary_artist = track_meta
                .artists
                .first()
                .cloned()
                .unwrap_or_else(|| track_meta.album_artist.clone());
            let basename = native_track_name(
                settings,
                &spotify_name_fields(&track_meta, &primary_artist, quality_label, format_label),
            );
            let target = dest_dir.join(format!("{basename}.m4a"));
            let found = match spotify_lyrics_for(&librespot, tid).await {
                Ok(word) => FoundLyrics {
                    word,
                    ..Default::default()
                }
                .filled(),
                Err(e) => {
                    on_log(format!("  ⚠ lyrics lookup failed: {e}"));
                    FoundLyrics::default()
                }
            };
            let found = resolve_track_lyrics(
                found,
                &track_meta.title,
                &primary_artist,
                None,
                settings,
                &on_log,
            )
            .await;
            if found.is_empty() {
                let reason = "no lyrics published for this track".to_string();
                on_log(format!("  ✗ {label}: {reason}"));
                outcome.record_failure(tid, &label, MhError::Other(reason));
                continue;
            }
            let sidecars = found.sidecars(Some(&track_meta.title), Some(&primary_artist));
            write_chosen_sidecar(&target, &sidecars, prefers_ttml(settings), &on_log).await;
            outcome.record_success();
            on_progress(BatchProgress::item_settled(
                i,
                total,
                label,
                outcome.settled(),
            ));
            continue;
        }

        let slice_start = (i as f32 / total as f32) * 100.0;
        let slice_ceiling = ((i as f32 + 0.9) / total as f32) * 100.0;
        let bp = crate::downloads::ByteProgress::default();
        license_limiter.acquire(Some(&*cancel)).await?;
        let stream_result = {
            let mut svc = librespot.write().await;
            let fut = svc.get_track_stream_for_download(
                tid,
                &quality,
                venv_python.as_deref(),
                Some(&bp),
                &settings.spotify_audio_download_mode,
            );
            tokio::pin!(fut);
            run_progress_tick_loop(
                fut,
                &bp,
                TickSlice {
                    start: slice_start,
                    ceiling: slice_ceiling,
                    label: &label,
                    completed,
                    total: total as u32,
                },
                &cancel,
                &on_progress,
            )
            .await?
        };
        let (bytes, content_type) = match stream_result {
            Ok(pair) => pair,
            Err(MhError::RateLimited(detail)) => {
                consecutive_rate_limits += 1;
                on_log(format!(
                    "  ✗ Spotify rate-limited ({consecutive_rate_limits}/{SPOTIFY_RATE_LIMIT_ABORT_STREAK}): {detail}"
                ));
                if consecutive_rate_limits >= SPOTIFY_RATE_LIMIT_ABORT_STREAK {
                    return Err(MhError::RateLimited(format!(
                        "Spotify rate-limited this account. Stopped after {}/{} \
                         tracks to avoid a temporary block — downloaded tracks are kept; \
                         try again in ~{SPOTIFY_FREE_RECOVERY_MIN} min.",
                        outcome.settled(),
                        total
                    )));
                }
                outcome.record_failure(tid, &label, format!("rate-limited: {detail}"));
                continue;
            }
            Err(e) => {
                on_log(format!("  ✗ stream failed for {label}: {e}"));
                outcome.record_failure(tid, &label, e);
                continue;
            }
        };

        on_progress(BatchProgress::at(
            slice_ceiling,
            label.clone(),
            completed,
            total as u32,
        ));

        let (actual_quality_label, actual_format_label) =
            labels_for_content_type(&content_type, &quality, quality_label, format_label);
        on_log(format!(
            "  delivered {} ({})",
            actual_quality_label, actual_format_label
        ));

        if !is_collection && completed == 0 {
            let candidate = dest_dir_for(
                settings,
                &base_dir,
                &ReleaseFolder {
                    album_artist: &track_meta.album_artist,
                    album: &track_meta.album,
                    year: track_meta.year.as_deref().unwrap_or(""),
                    genre: track_meta.genre.as_deref().unwrap_or(""),
                    label: track_meta.label.as_deref().unwrap_or(""),
                    quality_label: &actual_quality_label,
                    format: &actual_format_label,
                },
            );
            if candidate != base_dir {
                tokio::fs::create_dir_all(&candidate).await.ok();
                dest_dir = candidate;
            }
        }

        let ext = extension_for_content_type(&content_type);
        let primary_artist = track_meta
            .artists
            .first()
            .cloned()
            .unwrap_or_else(|| track_meta.album_artist.clone());
        let basename = native_track_name(
            settings,
            &spotify_name_fields(
                &track_meta,
                &primary_artist,
                &actual_quality_label,
                &actual_format_label,
            ),
        );
        let track_dir = disc_dir_for(
            settings,
            &dest_dir,
            track_meta.disc_number.unwrap_or(1),
            track_meta.disc_total,
        );
        if track_dir != dest_dir {
            tokio::fs::create_dir_all(&track_dir).await?;
        }
        let mut final_path = track_dir.join(format!("{basename}.{ext}"));
        if let Some(existing) =
            existing_final_file_for(&final_path, settings, settings.spotify_overwrite).await
        {
            on_log(format!(
                "  · already on disk, keeping {}",
                existing.display()
            ));
            outcome.record_skip();
            playlist_entries.push(PlaylistEntry {
                path: existing,
                title: track_meta.title.clone(),
                artist: primary_artist.clone(),
                duration_secs: None,
            });
            on_progress(BatchProgress::item_settled(
                i,
                total,
                label,
                outcome.settled(),
            ));
            continue;
        }
        let raw_path = track_dir.join(format!("{basename}.raw.{ext}"));
        if let Err(e) = tokio::fs::write(&raw_path, &bytes).await {
            on_log(format!("  ✗ write failed for {label}: {e}"));
            outcome.record_failure(tid, &label, e);
            continue;
        }

        let cover_url = track_meta
            .cover_group
            .as_ref()
            .and_then(|g| {
                crate::services::spotify::session::pick_cover_url_sized(
                    g,
                    &settings.spotify_cover_size,
                )
            })
            .or_else(|| track_meta.cover_url.clone());
        let muxer = ffmpeg_muxer_for_ext(ext);
        remux_or_keep(&raw_path, &final_path, muxer, ext == "mp3", &on_log).await;

        if let Some(reason) = short_rip_reason(&final_path, None, None).await {
            let _ = tokio::fs::remove_file(&final_path).await;
            let _ = tokio::fs::remove_file(&raw_path).await;
            on_log(format!("  ✗ {label}: {reason}"));
            outcome.record_failure(tid, &label, MhError::Other(reason));
            continue;
        }

        match maybe_convert(
            final_path.clone(),
            settings,
            ConversionSource::with_codec(AudioCodec::Aac),
        )
        .await
        {
            Ok((converted, note, _)) => {
                if let Some(n) = note {
                    on_log(format!("  {n}"));
                }
                final_path = converted;
            }
            Err(e) => {
                on_log(format!("  ✗ conversion failed for {label}: {e}"));
                outcome.record_failure(tid, &label, e);
                continue;
            }
        }

        let (cover_path, shared_cover) = cover_for_track(
            &http,
            &mut cover_cache,
            cover_url,
            &dest_dir,
            &base_dir,
            &basename,
            &mut shared_cover_path,
            settings,
            &on_log,
        )
        .await;
        let found = if lyrics_wanted(settings) {
            let found = match spotify_lyrics_for(&librespot, tid).await {
                Ok(word) => FoundLyrics {
                    word,
                    ..Default::default()
                }
                .filled(),
                Err(e) => {
                    on_log(format!("  ⚠ lyrics lookup failed: {e}"));
                    FoundLyrics::default()
                }
            };
            let found = resolve_track_lyrics(
                found,
                &track_meta.title,
                &primary_artist,
                None,
                settings,
                &on_log,
            )
            .await;
            found
        } else {
            FoundLyrics::default()
        };

        let mut meta_for_tags = track_meta.track_metadata();
        if settings.embed_lyrics {
            meta_for_tags.lyrics = found.plain.clone();
        }
        match tag_file(
            &final_path,
            &meta_for_tags,
            settings
                .embed_cover
                .then_some(cover_path.as_deref())
                .flatten(),
            &exclude_tags,
        )
        .await
        {
            Ok(()) => on_log(format!("  ✓ tagged → {}", final_path.display())),
            Err(e) => on_log(format!("  ⚠ tagging failed: {e}")),
        }
        if !settings.spotify_no_synced_lyrics_file {
            let sidecars = found.sidecars(Some(&track_meta.title), Some(&primary_artist));
            write_sidecars(&final_path, &sidecars, settings, &on_log).await;
        }
        settle_cover_file(cover_path, shared_cover, settings, &on_log).await;

        if let Some(d) = dedup {
            d.record(
                "spotify",
                tid,
                &final_path.to_string_lossy(),
                requested_rank,
            );
        }
        playlist_entries.push(PlaylistEntry {
            path: final_path.clone(),
            title: track_meta.title.clone(),
            artist: primary_artist.clone(),
            duration_secs: None,
        });
        outcome.record_success();
        consecutive_rate_limits = 0;
        on_progress(BatchProgress::item_settled(
            i,
            total,
            label,
            outcome.settled(),
        ));
    }

    if let Some(p) = shared_cover_path {
        if settings.save_album_cover {
            on_log(format!("  ✓ cover → {}", p.display()));
        } else {
            let _ = tokio::fs::remove_file(&p).await;
        }
    }

    if settings.save_playlist_file {
        if let Some(name) = playlist_name.as_deref() {
            write_playlist_file(&dest_dir, name, &playlist_entries, &on_log).await;
        }
    }

    let outcome = outcome.require_any("Spotify")?.with_dest(&dest_dir);
    on_log(outcome.failure_report("Native Spotify"));
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_supported() {
        assert!(matches!(
            classify_spotify_url("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"),
            SpotifyUrlSupport::Supported(SpotifyLinkKind::Track)
        ));
        assert!(matches!(
            classify_spotify_url("spotify:album:1DFixLWuPkv3KT3TnV35m3"),
            SpotifyUrlSupport::Supported(SpotifyLinkKind::Album)
        ));
        assert!(matches!(
            classify_spotify_url("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M"),
            SpotifyUrlSupport::Supported(SpotifyLinkKind::Playlist)
        ));
    }

    #[test]
    fn classify_unsupported() {
        assert!(matches!(
            classify_spotify_url("https://open.spotify.com/episode/512ojhOuo1ktJprKbVcKyQ"),
            SpotifyUrlSupport::Unsupported(_)
        ));
        assert!(matches!(
            classify_spotify_url("https://open.spotify.com/show/0XwhKMnXzVUBe0CkFEgGqx"),
            SpotifyUrlSupport::Unsupported(_)
        ));
        assert!(matches!(
            classify_spotify_url("https://open.spotify.com/artist/4tZwfgrHOc3mvqYlEYSvVi"),
            SpotifyUrlSupport::Unsupported(_)
        ));
        assert!(matches!(
            classify_spotify_url("https://open.spotify.com/audiobook/7iHfbu1YPACw6oZPAFJtqe"),
            SpotifyUrlSupport::Unsupported(_)
        ));
    }

    #[test]
    fn classify_not_spotify() {
        assert!(matches!(
            classify_spotify_url("https://music.apple.com/us/album/abc/12345"),
            SpotifyUrlSupport::NotSpotify
        ));
    }

    #[test]
    fn quality_recognised() {
        for q in SUPPORTED_QUALITIES {
            assert!(is_supported_quality(q), "{q}");
        }
        assert!(!is_supported_quality("flac"));
        assert!(!is_supported_quality("vorbis-high"));
        assert!(!is_supported_quality("mp3-320"));
    }

    #[test]
    fn labels_use_delivered_bitrate_when_present() {
        let (q, f) =
            labels_for_content_type("audio/aac; bitrate=256000", "aac-high", "256 kbps", "AAC");
        assert_eq!(q, "256 kbps");
        assert_eq!(f, "AAC");
        let (q2, f2) =
            labels_for_content_type("audio/aac; bitrate=128000", "aac-medium", "128 kbps", "AAC");
        assert_eq!(q2, "128 kbps");
        assert_eq!(f2, "AAC");
    }

    /// `{format}` already names the codec, so `{quality}` must not — otherwise a
    /// template of `{format} {quality}` reads "AAC AAC 256 kbps".
    #[test]
    fn a_quality_label_never_repeats_its_own_codec() {
        for (content_type, requested) in [
            ("audio/aac; bitrate=256000", "aac-high"),
            ("audio/mp4", "aac-high"),
            ("audio/mp4", "aac-medium"),
        ] {
            let (quality, format) =
                labels_for_content_type(content_type, requested, "256 kbps", "AAC");
            assert!(
                !quality.contains(&format),
                "{content_type} renders {quality:?} beside codec {format:?}"
            );
        }
    }

    #[test]
    fn labels_fall_back_to_request_when_no_bitrate_param() {
        let (q, f) = labels_for_content_type("audio/mp4", "aac-medium", "128 kbps", "AAC");
        assert_eq!(q, "128 kbps");
        assert_eq!(f, "AAC");
    }
}

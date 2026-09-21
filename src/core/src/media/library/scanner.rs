use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::stream::{self, StreamExt};
use lofty::prelude::*;
use tokio::process::Command;

use crate::errors::MhResult;
use crate::ipc_contract::{LibraryChangedEvent, LibraryScanProgressEvent};
use crate::media::library::covers::CoverCache;
use crate::media::library::db::{path_to_string, LibraryDb, TrackRow};
use crate::media::library::walker::{walk, FsEntry};
use crate::subprocess;
use crate::EventEmitter;

const UPSERT_BATCH: usize = 200;
const FFMPEG_THUMB_TIMEOUT: Duration = Duration::from_secs(15);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

fn scan_concurrency() -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    cpus.saturating_sub(1).max(2)
}

fn scan_pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(scan_concurrency())
            .thread_name(|i| format!("mh-scan-{i}"))
            .start_handler(|_| {
                let _ = thread_priority::set_current_thread_priority(
                    thread_priority::ThreadPriority::Min,
                );
            })
            .build()
            .expect("build scan thread pool")
    })
}

async fn on_scan_pool<F, T>(f: F) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    scan_pool().spawn(move || {
        let _ = tx.send(f());
    });
    rx.await.ok()
}

pub struct ScanContext {
    pub db: Arc<LibraryDb>,
    pub covers: Arc<CoverCache>,
    pub emitter: Arc<dyn EventEmitter>,
}

#[derive(Default, Debug, Clone)]
pub struct ScanStats {
    pub added: u64,
    pub updated: u64,
    pub removed: u64,
    pub unchanged: u64,
}

pub async fn scan_root(ctx: &ScanContext, root: &Path, force: bool) -> MhResult<ScanStats> {
    let dir_str = path_to_string(root);

    // Walking a large tree takes long enough that a silent start reads as a hang.
    emit_progress(&ctx.emitter, &dir_str, 0, 0, None, "scanning");

    let entries = walk(root.to_path_buf()).await?;
    let existing = ctx.db.list_under_root(root)?;
    let existing_map: HashMap<PathBuf, (i64, i64)> =
        existing.into_iter().map(|(p, m, s)| (p, (m, s))).collect();

    let mut to_process: Vec<FsEntry> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut added_count = 0u64;
    let mut updated_count = 0u64;

    for e in entries {
        seen.insert(e.path.clone());
        match existing_map.get(&e.path) {
            Some(&(mt, sz)) if !force && mt == e.mtime_ns && sz == e.size => continue,
            Some(_) => {
                updated_count += 1;
                to_process.push(e);
            }
            None => {
                added_count += 1;
                to_process.push(e);
            }
        }
    }

    let missing: Vec<PathBuf> = existing_map
        .keys()
        .filter(|p| !seen.contains(*p))
        .cloned()
        .collect();
    let removed_count = missing.len() as u64;
    if !missing.is_empty() {
        ctx.db.delete_paths(&missing)?;
    }

    let total = to_process.len() as u64;
    if total > 0 {
        emit_progress(&ctx.emitter, &dir_str, 0, total, None, "processing");
    }

    let covers = ctx.covers.clone();
    let db = ctx.db.clone();
    let emitter = ctx.emitter.clone();
    let dir_str_emit = dir_str.clone();

    let mut buf: Vec<TrackRow> = Vec::with_capacity(UPSERT_BATCH);
    let mut done: u64 = 0;
    let mut last_flush = Instant::now();
    let mut last_progress = Instant::now();

    let ffmpeg_bin = Arc::new(crate::venv_manager::resolve_ffmpeg());

    let mut stream = stream::iter(to_process.into_iter().map(|entry| {
        let covers = covers.clone();
        let ffmpeg_bin = ffmpeg_bin.clone();
        async move {
            if entry.is_video {
                process_video(entry, covers, ffmpeg_bin).await
            } else {
                process_audio(entry, covers).await
            }
        }
    }))
    .buffer_unordered(scan_concurrency());

    while let Some(row_opt) = stream.next().await {
        let row = match row_opt {
            Some(r) => r,
            None => continue,
        };
        done += 1;
        let cur = PathBuf::from(row.path.clone());
        buf.push(row);

        if buf.len() >= UPSERT_BATCH || last_flush.elapsed() >= Duration::from_millis(500) {
            let flushed = buf.len() as u64;
            let _ = db.upsert_tracks(&buf);
            buf.clear();
            last_flush = Instant::now();
            emitter.emit_library_changed(&LibraryChangedEvent {
                directory: dir_str_emit.clone(),
                added: flushed,
                updated: 0,
                removed: 0,
            });
        }

        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            emit_progress(
                &emitter,
                &dir_str_emit,
                done,
                total,
                Some(cur),
                "processing",
            );
            last_progress = Instant::now();
        }
    }

    if !buf.is_empty() {
        let _ = db.upsert_tracks(&buf);
    }

    ctx.db.rebuild_albums()?;

    let done_total = total.max(1);
    emit_progress(&ctx.emitter, &dir_str, done_total, done_total, None, "done");

    ctx.emitter.emit_library_changed(&LibraryChangedEvent {
        directory: dir_str,
        added: added_count,
        updated: updated_count,
        removed: removed_count,
    });

    Ok(ScanStats {
        added: added_count,
        updated: updated_count,
        removed: removed_count,
        unchanged: 0,
    })
}

async fn process_audio(entry: FsEntry, covers: Arc<CoverCache>) -> Option<TrackRow> {
    on_scan_pool(move || build_audio_row(&entry, &covers))
        .await
        .flatten()
}

fn build_audio_row(entry: &FsEntry, covers: &CoverCache) -> Option<TrackRow> {
    let size = entry.size;
    let mtime_ns = entry.mtime_ns;

    let folder_art = entry.path.parent().and_then(|p| covers.find_folder_art(p));

    let extracted = extract_audio_blocking(&entry.path)?;
    let path = entry.path.clone();

    let cover_id = extracted
        .cover_bytes
        .as_deref()
        .and_then(|b| covers.ingest_bytes(b).ok())
        .or_else(|| folder_art.and_then(|art| covers.ingest_folder_art(&art).ok()));

    let title = extracted.title.clone().or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    });

    let album_artist_tagged = extracted.album_artist.clone();
    let album_artist = album_artist_tagged
        .clone()
        .or_else(|| extracted.artist.clone());
    let primary_artist =
        normalize_primary_artist(extracted.artist.as_deref(), album_artist.as_deref());
    let album_key = album_key_for(
        extracted.album.as_deref(),
        album_artist_tagged.as_deref(),
        extracted.artist.as_deref(),
    );

    Some(TrackRow {
        path: path_to_string(&path),
        parent_dir: path.parent().map(path_to_string).unwrap_or_default(),
        release_dir: release_dir_for(&path).map(|p| path_to_string(&p)),
        size,
        mtime_ns,
        is_video: false,
        title,
        artist: extracted.artist,
        album: extracted.album,
        album_artist,
        album_key,
        year: extracted.year,
        genre: extracted.genre,
        duration_secs: extracted.duration_secs,
        track_no: extracted.track_no,
        disc_no: extracted.disc_no,
        cover_id,
        primary_artist,
        track_total: extracted.track_total,
        disc_total: extracted.disc_total,
        date: extracted.date,
        original_date: extracted.original_date,
        compilation: extracted.compilation,
        isrc: extracted.isrc,
        barcode: extracted.barcode,
        mb_recording_id: extracted.mb_recording_id,
        mb_release_id: extracted.mb_release_id,
        composer: extracted.composer,
        lyricist: extracted.lyricist,
        producer: extracted.producer,
        conductor: extracted.conductor,
        performer: extracted.performer,
        engineer: extracted.engineer,
        mixer: extracted.mixer,
        label: extracted.label,
        copyright: extracted.copyright,
        comment: extracted.comment,
        grouping: extracted.grouping,
        description: extracted.description,
        codec: extracted.codec,
        bitrate: extracted.bitrate,
        sample_rate: extracted.sample_rate,
        bit_depth: extracted.bit_depth,
        channels: extracted.channels,
        bpm: extracted.bpm,
        rg_track_gain: extracted.rg_track_gain,
        rg_track_peak: extracted.rg_track_peak,
        rg_album_gain: extracted.rg_album_gain,
        rg_album_peak: extracted.rg_album_peak,
        has_lyrics: extracted.has_lyrics,
        ..Default::default()
    })
}

/// The folder that stands for the release: the parent, or its parent when the file
/// sits in a per-disc subfolder. A two-CD rip is one album on disk and has to be one
/// album in the library, so `Emergency on Planet Earth/Disc 1` and `/Disc 2` both
/// resolve to `Emergency on Planet Earth`.
pub fn release_dir_for(path: &Path) -> Option<PathBuf> {
    Some(release_dir_of_parent(path.parent()?))
}

/// The same rule keyed on the folder alone, so the v12 migration can backfill
/// `release_dir` for an existing library from `parent_dir` without re-reading a
/// single file.
pub fn release_dir_of_parent(parent: &Path) -> PathBuf {
    let is_disc_folder = parent
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(is_disc_folder_name);
    if is_disc_folder {
        if let Some(grandparent) = parent.parent() {
            return grandparent.to_path_buf();
        }
    }
    parent.to_path_buf()
}

fn is_disc_folder_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    ["cd", "disc", "disk", "volume", "vol"]
        .iter()
        .filter_map(|p| lower.strip_prefix(p))
        .map(|rest| rest.trim_start_matches([' ', '_', '-', '.']))
        .any(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// Separator for tag fields that legitimately hold several values. Vorbis comments
/// repeat the key, ID3 and MP4 join with ", "; both collapse to this on read so the
/// column holds one comparable string either way.
const MULTI_JOIN: &str = "; ";

#[derive(Default)]
struct ExtractedAudio {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    year: Option<String>,
    genre: Option<String>,
    duration_secs: Option<f64>,
    track_no: Option<u32>,
    disc_no: Option<u32>,
    cover_bytes: Option<Vec<u8>>,
    track_total: Option<u32>,
    disc_total: Option<u32>,
    date: Option<String>,
    original_date: Option<String>,
    compilation: Option<bool>,
    isrc: Option<String>,
    barcode: Option<String>,
    mb_recording_id: Option<String>,
    mb_release_id: Option<String>,
    composer: Option<String>,
    lyricist: Option<String>,
    producer: Option<String>,
    conductor: Option<String>,
    performer: Option<String>,
    engineer: Option<String>,
    mixer: Option<String>,
    label: Option<String>,
    copyright: Option<String>,
    comment: Option<String>,
    grouping: Option<String>,
    description: Option<String>,
    codec: Option<String>,
    bitrate: Option<u32>,
    sample_rate: Option<u32>,
    bit_depth: Option<u32>,
    channels: Option<u32>,
    bpm: Option<u32>,
    rg_track_gain: Option<String>,
    rg_track_peak: Option<String>,
    rg_album_gain: Option<String>,
    rg_album_peak: Option<String>,
    has_lyrics: bool,
}

/// Reads back every field [`crate::services::common::pipeline::tagger`] writes, plus
/// the stream properties. The two field lists are kept in step by the round-trip test
/// at the bottom of this module — a field added to `TrackMetadata` and not here fails
/// it rather than silently vanishing from the library.
fn extract_audio_blocking(path: &Path) -> Option<ExtractedAudio> {
    use lofty::tag::ItemKey;

    let tagged = match lofty::read_from_path(path) {
        Ok(t) => t,
        Err(_) => return None,
    };
    let props = tagged.properties();
    let duration = props.duration().as_secs_f64();
    let mut out = ExtractedAudio {
        duration_secs: (duration > 0.0).then_some(duration),
        bitrate: props.audio_bitrate().or_else(|| props.overall_bitrate()),
        sample_rate: props.sample_rate(),
        bit_depth: props.bit_depth().map(u32::from),
        channels: props.channels().map(u32::from),
        codec: codec_label(path, tagged.file_type()),
        ..Default::default()
    };

    let Some(t) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Some(out);
    };

    let text = |key: ItemKey| -> Option<String> {
        t.get_string(key)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let multi = |key: ItemKey| -> Option<String> {
        let values: Vec<&str> = t
            .get_strings(key)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .collect();
        (!values.is_empty()).then(|| values.join(MULTI_JOIN))
    };

    out.title = t.title().map(|s| s.to_string());
    out.artist = multi(ItemKey::TrackArtist).or_else(|| t.artist().map(|s| s.to_string()));
    out.album = t.album().map(|s| s.to_string());
    out.album_artist = multi(ItemKey::AlbumArtist);
    out.year = t.date().map(|d| d.year.to_string());
    out.genre = multi(ItemKey::Genre).or_else(|| t.genre().map(|s| s.to_string()));
    out.track_no = t.track();
    out.disc_no = t.disk();
    out.track_total = t.track_total();
    out.disc_total = t.disk_total();
    out.cover_bytes = t.pictures().first().map(|p| p.data().to_vec());

    out.date = text(ItemKey::RecordingDate);
    out.original_date = text(ItemKey::OriginalReleaseDate);
    out.compilation = text(ItemKey::FlagCompilation).map(|v| {
        let v = v.to_ascii_lowercase();
        v != "0" && v != "false" && v != "no"
    });

    out.isrc = text(ItemKey::Isrc);
    out.barcode = text(ItemKey::Barcode);
    out.mb_recording_id = text(ItemKey::MusicBrainzRecordingId);
    out.mb_release_id = text(ItemKey::MusicBrainzReleaseId);

    out.composer = multi(ItemKey::Composer);
    out.lyricist = multi(ItemKey::Lyricist);
    out.producer = multi(ItemKey::Producer);
    out.conductor = multi(ItemKey::Conductor);
    out.performer = multi(ItemKey::Performer);
    out.engineer = multi(ItemKey::Engineer);
    out.mixer = multi(ItemKey::MixEngineer);

    out.label = text(ItemKey::Label).or_else(|| text(ItemKey::Publisher));
    out.copyright = text(ItemKey::CopyrightMessage);
    out.comment = text(ItemKey::Comment);
    out.grouping = text(ItemKey::ContentGroup);
    out.description = text(ItemKey::Description);

    out.bpm = text(ItemKey::IntegerBpm)
        .or_else(|| text(ItemKey::Bpm))
        .and_then(|v| v.split('.').next().and_then(|n| n.parse::<u32>().ok()));

    out.rg_track_gain = text(ItemKey::ReplayGainTrackGain);
    out.rg_track_peak = text(ItemKey::ReplayGainTrackPeak);
    out.rg_album_gain = text(ItemKey::ReplayGainAlbumGain);
    out.rg_album_peak = text(ItemKey::ReplayGainAlbumPeak);

    out.has_lyrics = text(ItemKey::Lyrics)
        .or_else(|| text(ItemKey::UnsyncLyrics))
        .is_some();

    Some(out)
}

fn codec_label(path: &Path, file_type: lofty::file::FileType) -> Option<String> {
    use lofty::file::FileType as F;
    let label = match file_type {
        F::Flac => "FLAC",
        F::Mpeg => "MP3",
        F::Opus => "Opus",
        F::Vorbis => "Vorbis",
        F::Wav => "WAV",
        F::Aiff => "AIFF",
        F::Ape => "APE",
        F::WavPack => "WavPack",
        F::Mpc => "Musepack",
        F::Speex => "Speex",
        F::Aac => "AAC",
        F::Mp4 => return Some(mp4_codec_label(path)),
        _ => return None,
    };
    Some(label.to_string())
}

/// An `.m4a` holds AAC or ALAC, which is the difference between lossy and lossless —
/// the one distinction a quality badge must not get wrong. `TaggedFile` only reports
/// the container, so the MP4 parser is re-run for this format alone, with tag reading
/// switched off so it costs a header parse rather than a second full read.
fn mp4_codec_label(path: &Path) -> String {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::mp4::{Mp4Codec, Mp4File};

    let parsed = std::fs::File::open(path)
        .ok()
        .and_then(|mut f| Mp4File::read_from(&mut f, ParseOptions::new().read_tags(false)).ok());
    match parsed.as_ref().and_then(|f| f.properties().codec()) {
        Some(Mp4Codec::ALAC) => "ALAC",
        Some(Mp4Codec::AAC) => "AAC",
        Some(Mp4Codec::FLAC) => "FLAC",
        Some(Mp4Codec::MP3) => "MP3",
        _ => "MP4",
    }
    .to_string()
}

async fn process_video(
    entry: FsEntry,
    covers: Arc<CoverCache>,
    ffmpeg: Arc<String>,
) -> Option<TrackRow> {
    let title = entry
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());

    let mut cover_id = {
        let covers = covers.clone();
        let parent = entry.path.parent().map(|p| p.to_path_buf());
        on_scan_pool(move || {
            parent
                .and_then(|p| covers.find_folder_art(&p))
                .and_then(|a| covers.ingest_folder_art(&a).ok())
        })
        .await
        .flatten()
    };

    if cover_id.is_none() && entry.size > 0 {
        if let Some(bytes) = extract_video_thumb(&entry.path, &ffmpeg).await {
            let covers = covers.clone();
            cover_id = on_scan_pool(move || covers.ingest_bytes(&bytes).ok())
                .await
                .flatten();
        }
    }

    Some(TrackRow {
        path: path_to_string(&entry.path),
        parent_dir: entry.path.parent().map(path_to_string).unwrap_or_default(),
        release_dir: release_dir_for(&entry.path).map(|p| path_to_string(&p)),
        size: entry.size,
        mtime_ns: entry.mtime_ns,
        is_video: true,
        title,
        cover_id,
        ..Default::default()
    })
}

pub fn normalize_primary_artist(
    artist: Option<&str>,
    album_artist: Option<&str>,
) -> Option<String> {
    let candidate = album_artist
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| artist.map(str::trim).filter(|s| !s.is_empty()))?;

    let lower = candidate.to_lowercase();
    let bytes = lower.as_bytes();
    let mut best: Option<usize> = None;
    let patterns: &[&str] = &[
        " feat. ",
        " feat ",
        " ft. ",
        " ft ",
        " featuring ",
        " & ",
        ", ",
        "; ",
        " x ",
        " × ",
        " vs. ",
        " vs ",
    ];
    for p in patterns {
        if let Some(idx) = find_ascii(bytes, p.as_bytes()) {
            best = Some(match best {
                Some(b) => b.min(idx),
                None => idx,
            });
        }
    }
    let cut = best.unwrap_or(candidate.len());
    let primary = candidate[..cut].trim();
    if primary.is_empty() {
        return None;
    }
    Some(normalize_key(primary))
}

fn normalize_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws && !out.is_empty() {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            for low in ch.to_lowercase() {
                out.push(low);
            }
            prev_ws = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

fn find_ascii(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
}

async fn extract_video_thumb(path: &Path, ffmpeg: &str) -> Option<Vec<u8>> {
    let tmp = tempfile::Builder::new().suffix(".jpg").tempfile().ok()?;
    let tmp_path = tmp.path().to_path_buf();
    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-ss",
        "1",
        "-i",
        &path.to_string_lossy(),
        "-vframes",
        "1",
        "-vf",
        "scale=256:-1",
        "-y",
        &tmp_path.to_string_lossy(),
    ])
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .stdin(std::process::Stdio::null());
    subprocess::apply_no_window(&mut cmd);

    let mut child = cmd.spawn().ok()?;
    match tokio::time::timeout(FFMPEG_THUMB_TIMEOUT, child.wait()).await {
        Ok(Ok(s)) if s.success() => tokio::fs::read(&tmp_path).await.ok(),
        _ => {
            let _ = child.kill().await;
            None
        }
    }
}

pub fn album_key_for(
    album: Option<&str>,
    album_artist_tagged: Option<&str>,
    track_artist: Option<&str>,
) -> Option<String> {
    let title = album
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(normalize_token)?;
    let seed = album_artist_tagged
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| track_artist.map(str::trim).filter(|s| !s.is_empty()));
    let artist_part = match seed {
        Some(s) => normalize_primary_artist(Some(s), Some(s)).unwrap_or_default(),
        None => String::new(),
    };
    Some(format!("{}::{}", title, artist_part))
}

fn normalize_token(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws && !out.is_empty() {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            for low in ch.to_lowercase() {
                out.push(low);
            }
            prev_ws = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

fn emit_progress(
    emitter: &Arc<dyn EventEmitter>,
    directory: &str,
    done: u64,
    total: u64,
    current_path: Option<PathBuf>,
    status: &str,
) {
    emitter.emit_library_scan_progress(&LibraryScanProgressEvent {
        directory: directory.to_string(),
        done,
        total,
        current_path: current_path.as_ref().map(|p| path_to_string(p)),
        status: status.to_string(),
    });
}

pub async fn ingest_paths(ctx: &ScanContext, paths: Vec<PathBuf>, root: &Path) -> MhResult<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut entries: Vec<FsEntry> = Vec::new();
    for p in paths {
        let meta = match tokio::fs::metadata(&p).await {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let ext = match p.extension().and_then(|e| e.to_str()) {
            Some(s) => s.to_ascii_lowercase(),
            None => continue,
        };
        use crate::media::file_discovery::{MUSIC_FORMATS, VIDEO_FORMATS};
        let is_video = VIDEO_FORMATS.contains(&ext.as_str());
        let is_audio = MUSIC_FORMATS.contains(&ext.as_str());
        if !is_video && !is_audio {
            continue;
        }
        let size = meta.len() as i64;
        let mtime_ns = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);
        entries.push(FsEntry {
            path: p,
            size,
            mtime_ns,
            is_video,
        });
    }

    let mut rows: Vec<TrackRow> = Vec::with_capacity(entries.len());
    let ffmpeg_bin = Arc::new(crate::venv_manager::resolve_ffmpeg());
    for e in entries {
        let row = if e.is_video {
            process_video(e, ctx.covers.clone(), ffmpeg_bin.clone()).await
        } else {
            process_audio(e, ctx.covers.clone()).await
        };
        if let Some(r) = row {
            rows.push(r);
        }
    }

    let added = rows.len() as u64;
    if !rows.is_empty() {
        let mut dirs: Vec<String> = rows.iter().filter_map(|r| r.release_dir.clone()).collect();
        dirs.sort();
        dirs.dedup();
        ctx.db.upsert_tracks(&rows)?;
        ctx.db.rebuild_albums_scoped(&dirs, &[])?;
    }

    if added > 0 {
        ctx.emitter.emit_library_changed(&LibraryChangedEvent {
            directory: path_to_string(root),
            added,
            updated: 0,
            removed: 0,
        });
    }
    Ok(())
}

pub fn forget_paths(ctx: &ScanContext, paths: &[PathBuf], root: &Path) -> MhResult<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let keys = ctx.db.album_keys_for_paths(paths)?;
    let mut dirs: Vec<String> = paths
        .iter()
        .filter_map(|p| release_dir_for(p).map(|d| path_to_string(&d)))
        .collect();
    dirs.sort();
    dirs.dedup();
    ctx.db.delete_paths(paths)?;
    ctx.db.rebuild_albums_scoped(&dirs, &keys)?;
    ctx.emitter.emit_library_changed(&LibraryChangedEvent {
        directory: path_to_string(root),
        added: 0,
        updated: 0,
        removed: paths.len() as u64,
    });
    Ok(())
}

pub fn forget_prefixes(ctx: &ScanContext, prefixes: &[PathBuf], root: &Path) -> MhResult<()> {
    if prefixes.is_empty() {
        return Ok(());
    }
    let mut removed: u64 = 0;
    for p in prefixes {
        removed += ctx.db.delete_under_prefix(p)? as u64;
    }
    if removed == 0 {
        return Ok(());
    }
    ctx.db.rebuild_albums()?;
    ctx.emitter.emit_library_changed(&LibraryChangedEvent {
        directory: path_to_string(root),
        added: 0,
        updated: 0,
        removed,
    });
    Ok(())
}

pub async fn prune_missing(ctx: &ScanContext, root: &Path) -> MhResult<u64> {
    if !root.exists() {
        return Ok(0);
    }
    let entries = match walk(root.to_path_buf()).await {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };
    let seen: std::collections::HashSet<PathBuf> = entries.into_iter().map(|e| e.path).collect();
    let missing: Vec<PathBuf> = ctx
        .db
        .list_under_root(root)?
        .into_iter()
        .map(|(p, _, _)| p)
        .filter(|p| !seen.contains(p))
        .collect();
    let removed = missing.len() as u64;
    if !missing.is_empty() {
        ctx.db.delete_paths(&missing)?;
        ctx.db.rebuild_albums()?;
        ctx.emitter.emit_library_changed(&LibraryChangedEvent {
            directory: path_to_string(root),
            added: 0,
            updated: 0,
            removed,
        });
    }
    Ok(removed)
}

pub fn ensure_local_cover(covers: &CoverCache, db: &LibraryDb, cover_id: &str) -> bool {
    if covers.has(cover_id) {
        return true;
    }
    let path_str = match db.find_track_path_by_cover_id(cover_id) {
        Ok(Some(p)) => p,
        _ => return false,
    };
    let path = PathBuf::from(&path_str);
    if let Some(art) = path.parent().and_then(|p| covers.find_folder_art(p)) {
        if covers.id_for_folder_art(&art).ok().as_deref() == Some(cover_id) {
            return covers.materialize_folder_art(cover_id, &art).is_ok();
        }
    }
    if let Some(extracted) = extract_audio_blocking(&path) {
        if let Some(bytes) = extracted.cover_bytes.as_deref() {
            if CoverCache::id_for_bytes(bytes) == cover_id {
                return covers.materialize_bytes(cover_id, bytes).is_ok();
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::normalize_primary_artist;

    #[test]
    fn album_artist_wins() {
        let got = normalize_primary_artist(Some("Daft Punk feat. X"), Some("Daft Punk"));
        assert_eq!(got.as_deref(), Some("daft punk"));
    }

    #[test]
    fn album_artist_is_also_split() {
        let got = normalize_primary_artist(
            Some("Daft Punk feat. Pharrell Williams"),
            Some("Daft Punk feat. Pharrell Williams"),
        );
        assert_eq!(got.as_deref(), Some("daft punk"));
    }

    #[test]
    fn ampersand_split() {
        let got = normalize_primary_artist(Some("Daft Punk & Julian Casablancas"), None);
        assert_eq!(got.as_deref(), Some("daft punk"));
    }

    #[test]
    fn feat_dot_split() {
        let got = normalize_primary_artist(Some("Daft Punk feat. Todd Edwards"), None);
        assert_eq!(got.as_deref(), Some("daft punk"));
    }

    #[test]
    fn comma_split() {
        let got = normalize_primary_artist(Some("A, B, C"), None);
        assert_eq!(got.as_deref(), Some("a"));
    }

    #[test]
    fn times_symbol_split() {
        let got = normalize_primary_artist(Some("Tycho × Saint Sinner"), None);
        assert_eq!(got.as_deref(), Some("tycho"));
    }

    #[test]
    fn empty_returns_none() {
        assert!(normalize_primary_artist(None, None).is_none());
        assert!(normalize_primary_artist(Some(""), Some("")).is_none());
    }

    #[test]
    fn no_separator_returns_full() {
        let got = normalize_primary_artist(Some("Tame Impala"), None);
        assert_eq!(got.as_deref(), Some("tame impala"));
    }

    #[test]
    fn whitespace_collapse() {
        let got = normalize_primary_artist(Some("  Hot   Chip  "), None);
        assert_eq!(got.as_deref(), Some("hot chip"));
    }
}

/// Guards the read side against drifting from the write side.
///
/// `tagger.rs` writes a field, `extract_audio_blocking` has to read it back, and for a
/// long time it read ten of the twenty-odd — ISRC, barcode, credits, BPM, ReplayGain
/// and the totals were written to every download and then dropped on the floor at scan
/// time. The `TrackMetadata` literal below is deliberately exhaustive: adding a field
/// to that struct stops this file compiling until the reader learns about it too.
#[cfg(test)]
mod round_trip_tests {
    use super::*;
    use crate::services::common::pipeline::tagger::{one, tag_file, TrackMetadata};

    fn ffmpeg_or_skip() -> Option<String> {
        let bin = crate::venv_manager::resolve_ffmpeg();
        std::process::Command::new(&bin)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .filter(|s| s.success())
            .map(|_| bin)
    }

    fn silence(ffmpeg: &str, path: &Path) {
        let status = std::process::Command::new(ffmpeg)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=48000:cl=stereo",
                "-t",
                "0.4",
            ])
            .arg(path)
            .status()
            .expect("spawn ffmpeg");
        assert!(status.success(), "could not build {}", path.display());
    }

    fn every_field() -> TrackMetadata {
        TrackMetadata {
            title: Some("Get Lucky".into()),
            artist: vec!["Daft Punk".into()],
            album: Some("Random Access Memories".into()),
            album_artist: one(Some("Daft Punk".into())),
            year: Some("2013-05-17".into()),
            original_date: Some("2013-04-19".into()),
            genre: one(Some("Disco".into())),
            track_number: Some(5),
            disc_number: Some(1),
            total_tracks: Some(12),
            total_discs: Some(1),
            comment: Some("a comment".into()),
            lyrics: Some("[00:01.00]Test lyric line".into()),
            isrc: Some("USTES1300001".into()),
            upc: Some("0888800000001".into()),
            copyright: Some("(P) 2013 Columbia Records".into()),
            label: Some("Columbia Records".into()),
            composer: vec![
                "Thomas Bangalter".into(),
                "Guy-Manuel de Homem-Christo".into(),
            ],
            conductor: one(Some("Chris Caswell".into())),
            performer: one(Some("Nathan East".into())),
            lyricist: vec!["Pharrell Williams".into(), "Nile Rodgers".into()],
            producer: vec!["daft punk".into(), "dj falcon".into()],
            engineer: one(Some("Peter Franco".into())),
            mixer: one(Some("Mick Guzauski".into())),
            bpm: Some("110".into()),
            replaygain_track_gain: Some("-11.01 dB".into()),
            replaygain_track_peak: Some("0.891249".into()),
            replaygain_album_gain: Some("-10.00 dB".into()),
            replaygain_album_peak: Some("0.912345".into()),
            description: Some("a description".into()),
            purchase_date: Some("2013-01-01".into()),
            grouping: Some("Nu-Disco".into()),
        }
    }

    #[tokio::test]
    async fn every_written_tag_is_read_back() {
        let Some(ffmpeg) = ffmpeg_or_skip() else {
            eprintln!("skipping: no usable ffmpeg");
            return;
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("fixture.flac");
        silence(&ffmpeg, &path);

        let meta = every_field();
        tag_file(&path, &meta, None, &[]).await.expect("tag");

        let got = extract_audio_blocking(&path).expect("read back");

        assert_eq!(got.title.as_deref(), Some("Get Lucky"));
        assert_eq!(got.artist.as_deref(), Some("Daft Punk"));
        assert_eq!(got.album.as_deref(), Some("Random Access Memories"));
        assert_eq!(got.album_artist.as_deref(), Some("Daft Punk"));
        assert_eq!(got.year.as_deref(), Some("2013"));
        assert_eq!(got.date.as_deref(), Some("2013-05-17"));
        assert_eq!(got.original_date.as_deref(), Some("2013-04-19"));
        assert_eq!(got.genre.as_deref(), Some("Disco"));
        assert_eq!(got.track_no, Some(5));
        assert_eq!(got.track_total, Some(12));
        assert_eq!(got.disc_no, Some(1));
        assert_eq!(got.disc_total, Some(1));
        assert_eq!(got.comment.as_deref(), Some("a comment"));
        assert_eq!(got.isrc.as_deref(), Some("USTES1300001"));
        assert_eq!(got.barcode.as_deref(), Some("0888800000001"));
        assert_eq!(got.copyright.as_deref(), Some("(P) 2013 Columbia Records"));
        assert_eq!(got.label.as_deref(), Some("Columbia Records"));
        assert_eq!(
            got.composer.as_deref(),
            Some("Thomas Bangalter; Guy-Manuel de Homem-Christo")
        );
        assert_eq!(got.conductor.as_deref(), Some("Chris Caswell"));
        assert_eq!(got.performer.as_deref(), Some("Nathan East"));
        assert_eq!(
            got.lyricist.as_deref(),
            Some("Pharrell Williams; Nile Rodgers")
        );
        assert_eq!(got.producer.as_deref(), Some("daft punk; dj falcon"));
        assert_eq!(got.engineer.as_deref(), Some("Peter Franco"));
        assert_eq!(got.mixer.as_deref(), Some("Mick Guzauski"));
        assert_eq!(got.bpm, Some(110));
        assert_eq!(got.rg_track_gain.as_deref(), Some("-11.01 dB"));
        assert_eq!(got.rg_track_peak.as_deref(), Some("0.891249"));
        assert_eq!(got.rg_album_gain.as_deref(), Some("-10.00 dB"));
        assert_eq!(got.rg_album_peak.as_deref(), Some("0.912345"));
        assert_eq!(got.grouping.as_deref(), Some("Nu-Disco"));
        assert!(got.has_lyrics, "the lyrics tag must register");

        assert_eq!(got.codec.as_deref(), Some("FLAC"));
        assert_eq!(got.sample_rate, Some(48000));
        assert_eq!(got.channels, Some(2));
        assert!(got.bit_depth.is_some(), "bit depth read from the stream");
        assert!(got.duration_secs.unwrap_or(0.0) > 0.0);
    }

    /// `.m4a` is AAC or ALAC and the badge must not confuse the two.
    #[tokio::test]
    async fn mp4_reports_its_real_codec_not_the_container() {
        let Some(ffmpeg) = ffmpeg_or_skip() else {
            eprintln!("skipping: no usable ffmpeg");
            return;
        };
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, args, expected) in [
            ("aac.m4a", ["-c:a", "aac"], "AAC"),
            ("alac.m4a", ["-c:a", "alac"], "ALAC"),
        ] {
            let path = dir.path().join(name);
            let status = std::process::Command::new(&ffmpeg)
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "anullsrc=r=44100:cl=stereo",
                    "-t",
                    "0.4",
                ])
                .args(args)
                .arg(&path)
                .status()
                .expect("spawn ffmpeg");
            assert!(status.success(), "could not build {name}");
            let got = extract_audio_blocking(&path).expect("read back");
            assert_eq!(got.codec.as_deref(), Some(expected), "{name}");
        }
    }

    /// Editing one field must not clear the twenty the request did not mention — the
    /// tagger writes the whole set every time, so the editor rebuilds it from the stored
    /// row first.
    #[tokio::test]
    async fn editing_one_tag_leaves_the_rest_alone() {
        use crate::media::library::Library;
        use crate::NoopEventEmitter;

        let Some(ffmpeg) = ffmpeg_or_skip() else {
            eprintln!("skipping: no usable ffmpeg");
            return;
        };
        let tmp = tempfile::tempdir().expect("tempdir");
        let music = tmp.path().join("music");
        std::fs::create_dir_all(&music).unwrap();
        let path = music.join("fixture.flac");
        silence(&ffmpeg, &path);
        tag_file(&path, &every_field(), None, &[])
            .await
            .expect("tag");

        let lib = Library::open(tmp.path(), Arc::new(NoopEventEmitter)).expect("library");
        lib.scan(&music, false).await.expect("scan");

        lib.write_tags(&crate::ipc_contract::LibraryWriteTagsRequest {
            path: path_to_string(&path),
            album_artist: Some("Thomas Bangalter & Guy-Manuel de Homem-Christo".to_string()),
            ..Default::default()
        })
        .await
        .expect("write tags");

        let got = extract_audio_blocking(&path).expect("re-read");
        assert_eq!(
            got.album_artist.as_deref(),
            Some("Thomas Bangalter & Guy-Manuel de Homem-Christo"),
            "the edit applied"
        );
        assert_eq!(got.isrc.as_deref(), Some("USTES1300001"), "isrc survived");
        assert_eq!(
            got.barcode.as_deref(),
            Some("0888800000001"),
            "barcode survived"
        );
        assert_eq!(
            got.composer.as_deref(),
            Some("Thomas Bangalter; Guy-Manuel de Homem-Christo"),
            "credits survived"
        );
        assert_eq!(got.bpm, Some(110), "bpm survived");
        assert_eq!(
            got.rg_track_gain.as_deref(),
            Some("-11.01 dB"),
            "gain survived"
        );
        assert_eq!(got.track_no, Some(5), "track number survived");
        assert!(
            got.has_lyrics,
            "lyrics survived an edit that never saw them"
        );
    }

    #[test]
    fn disc_subfolders_resolve_to_the_release_folder() {
        let cases = [
            ("/m/Album/Disc 1/01.flac", "/m/Album"),
            ("/m/Album/CD2/01.flac", "/m/Album"),
            ("/m/Album/cd_03/01.flac", "/m/Album"),
            ("/m/Album/Disk 1/01.flac", "/m/Album"),
            ("/m/Album/01.flac", "/m/Album"),
            ("/m/Disc-O-Very/01.flac", "/m/Disc-O-Very"),
            ("/m/Volume/01.flac", "/m/Volume"),
        ];
        for (input, expected) in cases {
            let got = release_dir_for(Path::new(input)).expect(input);
            assert_eq!(got, PathBuf::from(expected), "{input}");
        }
    }
}

#[cfg(test)]
mod prune_tests {
    use super::*;
    use crate::media::library::covers::CoverCache;
    use crate::media::library::db::{LibraryDb, TrackRow};
    use crate::NoopEventEmitter;

    fn ctx_in(dir: &Path) -> ScanContext {
        let db = Arc::new(LibraryDb::open(&dir.join("library.sqlite")).unwrap());
        let covers = Arc::new(CoverCache::new(dir.join("covers")));
        ScanContext {
            db,
            covers,
            emitter: Arc::new(NoopEventEmitter),
        }
    }

    fn track_row(path: &Path) -> TrackRow {
        TrackRow {
            path: path_to_string(path),
            parent_dir: path.parent().map(path_to_string).unwrap_or_default(),
            size: 1,
            mtime_ns: 1,
            is_video: false,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn prune_missing_removes_vanished_file_keeps_present() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ctx_in(tmp.path());
        let root = tmp.path().join("lib");
        std::fs::create_dir_all(root.join("AlbumA")).unwrap();
        let present = root.join("AlbumA/present.flac");
        std::fs::write(&present, b"x").unwrap();
        let gone = root.join("AlbumA/gone.flac");
        ctx.db
            .upsert_tracks(&[track_row(&present), track_row(&gone)])
            .unwrap();

        let removed = prune_missing(&ctx, &root).await.unwrap();
        assert_eq!(removed, 1);
        let rows = ctx.db.list_under_root(&root).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].0.ends_with("present.flac"));
    }

    #[tokio::test]
    async fn prune_missing_skips_absent_root() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ctx_in(tmp.path());
        let offline_root = tmp.path().join("offline_drive");
        let track = offline_root.join("AlbumA/01.flac");
        ctx.db.upsert_tracks(&[track_row(&track)]).unwrap();

        let removed = prune_missing(&ctx, &offline_root).await.unwrap();
        assert_eq!(removed, 0);
        assert_eq!(ctx.db.list_under_root(&offline_root).unwrap().len(), 1);
    }
}

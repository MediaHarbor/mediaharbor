use bytes::Bytes;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::defaults::Settings;
use crate::downloads::BatchProgress;
use crate::downloads::ByteProgress;
use crate::errors::{MhError, MhResult};
use crate::services::common::pipeline::downloader::{track_vars, CommonTrackFields};

/// Turn raw byte counters into a percentage, speed and ETA for one slice of a
/// multi-step download. Not service-specific: every native engine ticks the
/// same way.
pub(crate) fn tick_progress(
    bp: &ByteProgress,
    meter: &mut crate::downloads::SpeedMeter,
    slice_start: f32,
    slice_ceiling: f32,
    anim: &mut f32,
) -> (f32, Option<f64>, Option<f64>) {
    let done = bp.done.load(Ordering::Relaxed);
    let total_bytes = bp.total.load(Ordering::Relaxed);
    let speed = meter.sample(done).filter(|s| *s > 0.0);
    if total_bytes > 0 {
        let frac = (done as f64 / total_bytes as f64).min(0.999) as f32;
        let pct = slice_start + frac * (slice_ceiling - slice_start);
        let eta = speed.map(|s| total_bytes.saturating_sub(done) as f64 / s);
        (pct, speed, eta)
    } else {
        *anim += (slice_ceiling - *anim) * 0.12;
        (*anim, speed, None)
    }
}

/// The release fields `filepaths_folder_format` can reference. Kept as a struct so
/// adding a template variable does not grow another positional argument.
#[derive(Debug, Default, Clone)]
pub(crate) struct ReleaseFolder<'a> {
    pub album_artist: &'a str,
    pub album: &'a str,
    pub year: &'a str,
    pub genre: &'a str,
    pub label: &'a str,
    pub quality_label: &'a str,
    pub format: &'a str,
}

pub(crate) fn dest_dir_for(settings: &Settings, base: &Path, r: &ReleaseFolder<'_>) -> PathBuf {
    let (quality_label, format) = crate::services::common::pipeline::converted_quality_format(
        settings,
        r.quality_label,
        r.format,
    );
    let folder = crate::services::common::pipeline::build_album_folder(
        &settings.filepaths_folder_format,
        r.album_artist,
        r.album,
        r.year,
        r.genre,
        r.label,
        &quality_label,
        &format,
        crate::services::common::pipeline::FolderNaming::from_settings(settings),
    );
    base.join(folder)
}

/// Mirrors the rule the Tidal/Qobuz/Deezer orchestrator applies: a `Disc N`
/// subdirectory only when the release genuinely spans more than one disc.
pub(crate) fn disc_dir_for(
    settings: &Settings,
    album_dir: &Path,
    disc_number: u32,
    disc_total: Option<u32>,
) -> PathBuf {
    let multi_disc = disc_total.map(|n| n > 1).unwrap_or(false) || disc_number > 1;
    if settings.disc_subdirectories && multi_disc {
        album_dir.join(format!("Disc {}", disc_number.max(1)))
    } else {
        album_dir.to_path_buf()
    }
}

/// Renders a native-ripper filename through the same template the pipeline services
/// use, so `filepaths_track_format` applies to every service rather than just three.
pub(crate) fn native_track_name(settings: &Settings, fields: &CommonTrackFields) -> String {
    let template = if settings.filepaths_track_format.trim().is_empty() {
        "{tracknumber:02}. {artist} - {title}"
    } else {
        settings.filepaths_track_format.as_str()
    };
    let (quality_label, format_label) = crate::services::common::pipeline::converted_quality_format(
        settings,
        &fields.quality_label,
        &fields.format_label,
    );
    let fields = &CommonTrackFields {
        quality_label,
        format_label,
        ..fields.clone()
    };
    let stem = crate::services::common::pipeline::build_file_name(
        template,
        &track_vars(fields),
        settings.filepaths_restrict_characters,
        settings.filepaths_truncate_to as usize,
    );
    if stem.is_empty() {
        crate::services::common::pipeline::safe_name(&fields.title)
    } else {
        stem
    }
}

/// `basename` is `None` when the tracks have a folder to themselves, which gets one
/// `cover.jpg` for the release rather than a copy per track.
pub fn cover_file_name(basename: Option<&str>, cover_bytes: &[u8]) -> String {
    let ext = if cover_bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else {
        "jpg"
    };
    match basename {
        Some(b) => format!("{b}.cover.{ext}"),
        None => format!("cover.{ext}"),
    }
}

pub(crate) async fn write_cover_file(
    dest_dir: &Path,
    basename: Option<&str>,
    cover_bytes: Option<&[u8]>,
    on_log: impl Fn(String),
) -> Option<PathBuf> {
    let bytes = cover_bytes?;
    let p = dest_dir.join(cover_file_name(basename, bytes));
    if let Err(e) = tokio::fs::write(&p, bytes).await {
        on_log(format!("  ⚠ cover write failed: {e}"));
        None
    } else {
        Some(p)
    }
}

pub(crate) struct TickSlice<'a> {
    pub start: f32,
    pub ceiling: f32,
    pub label: &'a str,
    pub completed: u32,
    pub total: u32,
}

pub(crate) async fn run_progress_tick_loop<T, F>(
    mut fut: Pin<&mut F>,
    bp: &ByteProgress,
    slice: TickSlice<'_>,
    cancel: &Arc<AtomicBool>,
    on_progress: impl Fn(BatchProgress),
) -> MhResult<T>
where
    F: std::future::Future<Output = T>,
{
    let mut anim = slice.start;
    let mut meter = crate::downloads::SpeedMeter::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(300));
    tick.tick().await;
    loop {
        tokio::select! {
            res = &mut fut => break Ok(res),
            _ = tick.tick() => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(MhError::Cancelled);
                }
                let (percent, speed, eta) =
                    tick_progress(bp, &mut meter, slice.start, slice.ceiling, &mut anim);
                on_progress(BatchProgress {
                    percent,
                    current_track: slice.label.to_string(),
                    completed: slice.completed,
                    total: slice.total,
                    speed,
                    eta,
                    ..Default::default()
                });
            }
        }
    }
}

pub(crate) const MIN_PLAUSIBLE_MEDIA_BYTES: u64 = 1024;

pub(crate) trait NativeTrack: Sync {
    fn track_metadata(&self) -> crate::services::common::pipeline::tagger::TrackMetadata;
}

/// Remuxes the raw stream into its final container. Metadata and cover art are
/// written afterwards by the shared lofty tagger, so ffmpeg only has to move bytes.
pub(crate) async fn remux(
    raw_in: &Path,
    out_path: &Path,
    muxer: &str,
    id3v2_v3: bool,
) -> MhResult<()> {
    let ffmpeg = crate::venv_manager::resolve_ffmpeg();
    let mut cmd = tokio::process::Command::new(&ffmpeg);
    cmd.arg("-y").arg("-loglevel").arg("error");
    cmd.arg("-i").arg(raw_in);
    cmd.arg("-map").arg("0:a");
    cmd.arg("-c").arg("copy");
    if id3v2_v3 {
        cmd.arg("-id3v2_version").arg("3");
    }

    cmd.arg("-f").arg(muxer).arg(out_path);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    crate::subprocess::apply_no_window(&mut cmd);

    let out = cmd
        .output()
        .await
        .map_err(|e| MhError::Subprocess(format!("ffmpeg spawn failed: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(MhError::Subprocess(format!(
            "ffmpeg remux failed (exit {}): {}",
            out.status.code().unwrap_or(-1),
            err.trim()
        )));
    }

    let produced = tokio::fs::metadata(out_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if produced < MIN_PLAUSIBLE_MEDIA_BYTES {
        let _ = tokio::fs::remove_file(out_path).await;
        return Err(MhError::Subprocess(format!(
            "ffmpeg remux produced only {produced} bytes from a {} byte source, so the output \
             holds no audio. ffmpeg said: {}",
            tokio::fs::metadata(raw_in)
                .await
                .map(|m| m.len())
                .unwrap_or(0),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod folder_tests {
    use super::*;

    fn settings(disc_subdirs: bool) -> Settings {
        Settings {
            disc_subdirectories: disc_subdirs,
            ..Settings::default()
        }
    }

    #[test]
    fn a_single_disc_release_gets_no_disc_folder() {
        let s = settings(true);
        let album = Path::new("/music/Album");
        assert_eq!(disc_dir_for(&s, album, 1, Some(1)), album);
        assert_eq!(disc_dir_for(&s, album, 1, None), album);
    }

    #[test]
    fn a_multi_disc_release_splits_by_disc() {
        let s = settings(true);
        let album = Path::new("/music/Album");
        assert_eq!(disc_dir_for(&s, album, 1, Some(2)), album.join("Disc 1"));
        assert_eq!(disc_dir_for(&s, album, 2, Some(2)), album.join("Disc 2"));
        assert_eq!(disc_dir_for(&s, album, 2, None), album.join("Disc 2"));
    }

    #[test]
    fn the_setting_switches_it_off() {
        let s = settings(false);
        let album = Path::new("/music/Album");
        assert_eq!(disc_dir_for(&s, album, 2, Some(3)), album);
    }

    #[test]
    fn the_folder_template_reaches_genre_and_label() {
        let s = Settings {
            filepaths_folder_format: "{albumartist} - {album} [{genre}] ({label})".into(),
            ..Settings::default()
        };
        let dir = dest_dir_for(
            &s,
            Path::new("/music"),
            &ReleaseFolder {
                album_artist: "Artist",
                album: "Album",
                genre: "Jazz",
                label: "Blue Note",
                ..Default::default()
            },
        );
        assert_eq!(dir, Path::new("/music/Artist - Album [Jazz] (Blue Note)"));
    }
}

#[cfg(test)]
mod cover_name_tests {
    use super::cover_file_name;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0];
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G'];

    #[test]
    fn a_release_folder_gets_one_cover_for_the_whole_album() {
        assert_eq!(cover_file_name(None, JPEG), "cover.jpg");
        assert_eq!(cover_file_name(None, PNG), "cover.png");
    }

    #[test]
    fn loose_tracks_sharing_a_directory_keep_their_own_cover() {
        assert_eq!(
            cover_file_name(Some("01. Daft Punk - Give Life Back to Music"), JPEG),
            "01. Daft Punk - Give Life Back to Music.cover.jpg"
        );
    }
}

/// The cover image at `url`, or `None` for anything that is not a usable response.
pub(crate) async fn fetch_cover(client: &reqwest::Client, url: Option<&str>) -> Option<Bytes> {
    let url = url?;
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.bytes().await.ok()
}

/// Remux the raw stream into its final container, keeping the raw bytes if ffmpeg
/// cannot.
///
/// A failed remux is not a failed download — what is on disk decodes, it just wears
/// the wrong container — so the stream is moved into place rather than discarded.
pub(crate) async fn remux_or_keep(
    raw_path: &Path,
    final_path: &Path,
    muxer: &str,
    id3v2_v3: bool,
    on_log: impl Fn(String),
) {
    match remux(raw_path, final_path, muxer, id3v2_v3).await {
        Ok(()) => {
            let _ = tokio::fs::remove_file(raw_path).await;
        }
        Err(e) => {
            on_log(format!(
                "  ⚠ ffmpeg remux failed ({e}); keeping the raw stream"
            ));
            let _ = tokio::fs::rename(raw_path, final_path).await;
        }
    }
}

/// The cover file this track should be tagged against, plus whether it is the
/// release-wide one.
///
/// Every track on a release points at the same image, so the bytes are fetched once
/// per URL and memoised. When the tracks land in their own folder that folder gets a
/// single shared file; a track filed straight into the base directory gets its own.
pub(crate) async fn cover_for_track(
    http: &reqwest::Client,
    cover_cache: &mut HashMap<String, Option<Bytes>>,
    cover_url: Option<String>,
    dest_dir: &Path,
    base_dir: &Path,
    basename: &str,
    shared_cover_path: &mut Option<PathBuf>,
    settings: &Settings,
    on_log: impl Fn(String) + Copy,
) -> (Option<PathBuf>, bool) {
    let cover_bytes: Option<Bytes> = match cover_url {
        Some(curl) => {
            if !cover_cache.contains_key(&curl) {
                let b = fetch_cover(http, Some(&curl)).await;
                cover_cache.insert(curl.clone(), b);
            }
            cover_cache.get(&curl).cloned().flatten()
        }
        None => None,
    };

    let shared_cover = dest_dir != base_dir;
    let cover_path = if shared_cover {
        if shared_cover_path.is_none() {
            *shared_cover_path =
                write_cover_file(dest_dir, None, cover_bytes.as_deref(), on_log).await;
        }
        if settings.save_cover {
            write_cover_file(dest_dir, Some(basename), cover_bytes.as_deref(), on_log).await;
        }
        shared_cover_path.clone()
    } else {
        write_cover_file(dest_dir, Some(basename), cover_bytes.as_deref(), on_log).await
    };
    (cover_path, shared_cover)
}

/// What becomes of a per-track cover file once tagging has read it: kept when the
/// user asked for cover files on disk, deleted when it only ever existed to be
/// embedded. The release-wide file is settled once at the end of the run instead.
pub(crate) async fn settle_cover_file(
    cover_path: Option<PathBuf>,
    shared_cover: bool,
    settings: &Settings,
    on_log: impl Fn(String),
) {
    if shared_cover {
        return;
    }
    match cover_path {
        Some(p) if settings.save_cover => on_log(format!("  ✓ cover → {}", p.display())),
        Some(p) => {
            let _ = tokio::fs::remove_file(&p).await;
        }
        None => {}
    }
}

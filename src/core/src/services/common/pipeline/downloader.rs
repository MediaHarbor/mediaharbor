use futures_util::StreamExt;
use reqwest::header::HeaderMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::defaults::Settings;
use crate::downloads::dedup::DedupLedger;
use crate::errors::{MhError, MhResult};
use crate::services::common::lyrics::{write_sidecars, LyricsSidecars};
use crate::services::common::pipeline::converter::{
    convert_audio, parse_codec, plan_conversion, probe_audio, ConversionPlan, ConversionSettings,
    ConversionSource, ProbedAudio,
};
use crate::services::common::pipeline::orchestrator::TrackJob;
use crate::services::common::pipeline::tagger::{tag_file, TrackMetadata};
use crate::services::common::pipeline::TrackOutcome;

pub use crate::http_client::{is_transient_http_error, retry_transient};

/// Everything that can end a track download before a byte is fetched.
pub fn preflight(
    job: &TrackJob<'_>,
    platform: &str,
    label: &str,
    flags: &[&str],
    rank: i64,
) -> MhResult<Option<TrackOutcome>> {
    if let Some(rec) = job.record {
        for flag in flags {
            if rec[*flag].as_bool() == Some(false) {
                return Err(MhError::NotFound(format!(
                    "{label} lists this track but will not stream it here — it is not \
                     available in this region or on this account"
                )));
            }
        }
    }
    if let Some(d) = job.dedup {
        if let Some(existing) = d.existing_path(platform, job.track_id, rank) {
            (job.on_log)(format!(
                "  ↷ skipped — an equal or better copy is already on disk, so \
                 nothing was downloaded and this file keeps its original \
                 quality: {existing}"
            ));
            return Ok(Some(TrackOutcome::Skipped(PathBuf::from(existing))));
        }
    }
    Ok(None)
}

/// Everything the shared download tail needs.
pub struct FinalizeTrack<'a> {
    pub dest: PathBuf,
    pub platform: &'a str,
    pub track_id: &'a str,
    pub rank: i64,
    pub served_rank: i64,
    pub source: ConversionSource,
    pub duration: Option<f64>,
    pub metadata: &'a TrackMetadata,
    pub embed_cover: bool,
    pub cover_tmp: Option<&'a TrackCover>,
    pub sidecars: LyricsSidecars,
    pub settings: &'a Settings,
    pub dedup: Option<&'a DedupLedger>,
    pub on_log: &'a (dyn Fn(String) + Send + Sync),
}

/// Convert, reject a short rip, tag, write sidecars, record the served quality.
pub async fn finalize_track(f: FinalizeTrack<'_>) -> MhResult<TrackOutcome> {
    let cover_path = if f.embed_cover {
        f.cover_tmp.map(|t| t.path())
    } else {
        None
    };

    let (dest, note, probed) = maybe_convert(f.dest, f.settings, f.source).await?;
    if let Some(n) = note {
        (f.on_log)(format!("  {n}"));
    }

    if let Some(reason) = short_rip_reason(&dest, f.duration, probed.as_ref()).await {
        let _ = tokio::fs::remove_file(&dest).await;
        (f.on_log)(format!("  ✗ {reason}"));
        return Err(MhError::Other(reason));
    }

    tag_file(&dest, f.metadata, cover_path, &excluded_tags(f.settings)).await?;
    write_sidecars(&dest, &f.sidecars, f.settings, f.on_log).await;

    if let Some(d) = f.dedup {
        d.record(
            f.platform,
            f.track_id,
            &dest.to_string_lossy(),
            f.served_rank.min(f.rank),
        );
    }
    Ok(TrackOutcome::Downloaded(dest))
}

/// A tag count read off a service payload: absent and empty both mean "unknown".
pub fn count_of(s: &str) -> Option<u32> {
    if s.is_empty() {
        None
    } else {
        s.parse().ok()
    }
}

/// Why a finished file is not the track it claims to be, if it isn't.
///
/// A truncated transfer otherwise gets tagged, filed, and written into the dedup
/// ledger as a completed download — so it is never retried and the user is left
/// with a partial track that looks finished. The native Apple Music engine has
/// always checked this; the pipeline never did.
///
/// `expected_secs` comes from the service's own record. Without it only the byte
/// floor applies, because there is nothing honest to compare against.
///
/// `probed` lets a caller that has already measured this exact file hand the result
/// in; `maybe_convert` probes whatever it leaves behind, so the pipeline services
/// pass that through instead of parsing the same file a second time.
pub async fn short_rip_reason(
    path: &Path,
    expected_secs: Option<f64>,
    probed: Option<&ProbedAudio>,
) -> Option<String> {
    let landed = tokio::fs::metadata(path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if landed < crate::downloads::native_common::MIN_PLAUSIBLE_MEDIA_BYTES {
        return Some(format!(
            "only {landed} bytes landed, so the file holds no audio"
        ));
    }
    let expected = expected_secs.filter(|s| *s > 1.0)?;
    let got_ms = match probed {
        Some(p) => p.duration_ms?,
        None => {
            let probe_path = path.to_path_buf();
            tokio::task::spawn_blocking(move || probe_audio(&probe_path))
                .await
                .ok()?
                .duration_ms?
        }
    };
    let got = got_ms as f64 / 1000.0;
    if got * 10.0 < expected * 9.0 {
        return Some(format!(
            "only {got:.1}s of audio landed but the track is {expected:.1}s — the \
             download was truncated"
        ));
    }
    None
}

/// Downloads land on a `.part` sibling and are renamed only once the transfer is
/// complete. Without it a cancelled or failed download leaves a truncated file at
/// the final path — which the skip-existing check would then mistake for a finished
/// track and never repair.
pub(crate) fn part_path(dest: &Path) -> PathBuf {
    let mut p = dest.as_os_str().to_os_string();
    p.push(".part");
    PathBuf::from(p)
}

pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    headers: Option<&HeaderMap>,
    on_progress: impl Fn(u64, u64),
) -> MhResult<()> {
    const MAX_ATTEMPTS: u32 = 3;
    let is_transient = |e: &MhError| matches!(e, MhError::Network(_)) || is_transient_http_error(e);
    retry_transient(MAX_ATTEMPTS, is_transient, || async {
        let mut req = client.get(url);
        if let Some(h) = headers {
            req = req.headers(h.clone());
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(MhError::Other(format!(
                "HTTP {} for {}",
                resp.status().as_u16(),
                url
            )));
        }

        let total = resp.content_length().unwrap_or(0);
        let mut stream = resp.bytes_stream();
        let part = part_path(dest);
        let mut file = File::create(&part).await?;
        let mut downloaded: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    let _ = tokio::fs::remove_file(&part).await;
                    return Err(e.into());
                }
            };
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            on_progress(downloaded, total);
        }
        file.flush().await?;
        drop(file);
        tokio::fs::rename(&part, dest).await?;
        Ok(())
    })
    .await
}

pub fn excluded_tags(settings: &Settings) -> Vec<String> {
    if settings.meta_exclude_tags_check && !settings.excluded_tags.is_empty() {
        settings
            .excluded_tags
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    } else {
        vec![]
    }
}

/// Which cover files a download leaves on disk. The two are independent: a release
/// folder can hold per-track sidecars, and a playlist folder can hold one shared cover.
#[derive(Debug, Clone, Copy)]
pub struct CoverFiles {
    /// `<track stem>.cover.jpg` beside each track.
    pub per_track: bool,
    /// One `cover.jpg` for the folder the tracks landed in.
    pub album: bool,
}

impl CoverFiles {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            per_track: settings.save_cover,
            album: settings.save_album_cover,
        }
    }

    pub fn any(self) -> bool {
        self.per_track || self.album
    }
}

/// One track's cover art, held both ways it is needed.
///
/// The tagger embeds from a path and the sidecar writer writes bytes, so a single
/// fetch serves both. Keeping the bytes is what stops `save_cover_file` reading the
/// temp file back off disk for something the cache already had in memory.
pub struct TrackCover {
    tmp: tempfile::NamedTempFile,
    bytes: Vec<u8>,
}

impl TrackCover {
    pub fn path(&self) -> &Path {
        self.tmp.path()
    }
}

/// The cover for this track, or `None` when nothing would be done with it.
///
/// The "is a cover wanted at all" rule lives here rather than at each call site: it is
/// `embed_cover` or any cover file, and spelling it out per service is how the Tidal
/// video path ended up fetching nothing when only the album-cover setting was on —
/// while still passing `CoverFiles::from_settings` to the writer.
pub async fn cover_tmp_for(
    client: &reqwest::Client,
    cover_url: Option<&str>,
    settings: &Settings,
    covers: &crate::services::common::pipeline::CoverCache,
) -> Option<TrackCover> {
    let url = cover_url?;
    if !settings.embed_cover && !CoverFiles::from_settings(settings).any() {
        return None;
    }
    let bytes = crate::services::common::pipeline::tagger::cover_bytes_cached(client, url, covers)
        .await
        .ok()?;
    let tmp = crate::services::common::pipeline::tagger::cover_temp_file(&bytes).ok()?;
    Some(TrackCover { tmp, bytes })
}

/// Writes the cover art next to the track, typed from its own magic bytes.
///
/// The folder cover is written once rather than once per track: this runs inside the
/// per-track loop, so a 20-track album used to rewrite the same JPEG 20 times.
pub async fn save_cover_file(
    track_dest: &Path,
    cover: Option<&TrackCover>,
    files: CoverFiles,
    track_stem: Option<&str>,
) {
    if !files.any() {
        return;
    }
    let Some(bytes) = cover.map(|c| c.bytes.as_slice()) else {
        return;
    };

    if files.per_track {
        if let Some(stem) = track_stem {
            let name = crate::downloads::native_common::cover_file_name(Some(stem), bytes);
            let _ = tokio::fs::write(track_dest.join(name), &bytes).await;
        }
    }

    if files.album {
        let name = crate::downloads::native_common::cover_file_name(None, bytes);
        let path = track_dest.join(name);
        let already = tokio::fs::metadata(&path)
            .await
            .map(|m| m.len() == bytes.len() as u64)
            .unwrap_or(false);
        if !already {
            let _ = tokio::fs::write(&path, &bytes).await;
        }
    }
}

/// Re-encodes the finished download when the user asked for a different format.
/// Runs *before* tagging in every pipeline so lofty, not ffmpeg, owns the tags
/// and cover art of whatever container comes out.
///
/// Returns the resulting path plus an optional note worth showing the user —
/// either the reason the re-encode was skipped, or an advisory about a request
/// that will not do what it sounds like.
pub async fn maybe_convert(
    dest_path: PathBuf,
    settings: &Settings,
    source: ConversionSource,
) -> MhResult<(PathBuf, Option<String>, Option<ProbedAudio>)> {
    if !settings.conversion_check {
        return Ok((dest_path, None, None));
    }

    let target_codec = parse_codec(&settings.conversion_codec).ok_or_else(|| {
        MhError::Config(format!(
            "unknown conversion output format {:?}; expected one of FLAC, ALAC, MP3, AAC, OPUS, VORBIS",
            settings.conversion_codec
        ))
    })?;

    let conv_settings = ConversionSettings {
        codec: target_codec,
        sampling_rate: settings.conversion_sampling_rate,
        bit_depth: settings.conversion_bit_depth,
        lossy_bitrate: Some(settings.conversion_lossy_bitrate),
    };

    let probe_path = dest_path.clone();
    let probed = tokio::task::spawn_blocking(move || probe_audio(&probe_path))
        .await
        .unwrap_or_default();

    let advisory = match plan_conversion(&source, &probed, &conv_settings) {
        ConversionPlan::Skip(reason) => {
            return Ok((dest_path, Some(format!("ⓘ {reason}")), Some(probed)))
        }
        ConversionPlan::Encode { note } => note,
    };

    let new_dest = dest_path.with_extension(target_codec.container());
    let ffmpeg = crate::venv_manager::resolve_ffmpeg();
    convert_audio(&dest_path, &new_dest, &conv_settings, &ffmpeg).await?;

    let probe_path = new_dest.clone();
    let landed = tokio::task::spawn_blocking(move || probe_audio(&probe_path))
        .await
        .unwrap_or_default();
    let measured = match landed.bitrate_kbps {
        Some(kbps) => format!(" at {kbps} kbps"),
        None => String::new(),
    };
    let note = match advisory {
        Some(a) => format!("⚠ {a}"),
        None => format!("✓ re-encoded to {}{measured}", target_codec.label()),
    };

    Ok((new_dest, Some(note), Some(landed)))
}

#[derive(Debug, Clone, Default)]
pub struct CommonTrackFields {
    pub title: String,
    pub artist: String,
    pub albumartist: String,
    pub album: String,
    pub track_num: u32,
    pub disc_num: u32,
    pub tracktotal: String,
    pub disctotal: String,
    pub year: String,
    pub genre: String,
    pub explicit: String,
    pub isrc: String,
    pub label: String,
    pub date: String,
    pub quality_label: String,
    pub format_label: String,
    pub composer: String,
}

/// The `{explicit}` template value. Tidal, Qobuz and Deezer each spelled this
/// suffix inline; the native rippers left it empty, so a default template that
/// ships with `{explicit}` in it silently rendered nothing on Spotify and Apple.
pub fn explicit_suffix(is_explicit: bool) -> String {
    if is_explicit {
        " (Explicit)".to_string()
    } else {
        String::new()
    }
}

pub fn track_vars(f: &CommonTrackFields) -> HashMap<&'static str, String> {
    let mut vars: HashMap<&'static str, String> = HashMap::new();
    vars.insert("title", f.title.clone());
    vars.insert("artist", f.artist.clone());
    vars.insert("albumartist", f.albumartist.clone());
    vars.insert("album", f.album.clone());
    vars.insert(
        "tracknumber",
        if f.track_num == 0 {
            String::new()
        } else {
            f.track_num.to_string()
        },
    );
    vars.insert("discnumber", f.disc_num.to_string());
    vars.insert("tracktotal", f.tracktotal.clone());
    vars.insert("disctotal", f.disctotal.clone());
    vars.insert("year", f.year.clone());
    vars.insert("genre", f.genre.clone());
    vars.insert("explicit", f.explicit.clone());
    vars.insert("isrc", f.isrc.clone());
    vars.insert("label", f.label.clone());
    vars.insert("date", f.date.clone());
    vars.insert("composer", f.composer.clone());
    vars.insert("quality", f.quality_label.clone());
    vars.insert("format", f.format_label.clone());
    vars
}

/// The file this download would land on, when one is already there. Checked against
/// the converted extension too, so turning conversion on does not make every track
/// look missing and get fetched again.
///
/// The dedup ledger is not enough on its own: it only knows what this install
/// downloaded, so a file restored from a backup — or kept across a library reset —
/// is invisible to it and used to be silently overwritten.
pub async fn existing_final_file(dest_path: &Path, settings: &Settings) -> Option<PathBuf> {
    existing_final_file_for(dest_path, settings, settings.pipeline_overwrite).await
}

/// As `existing_final_file`, for engines that carry their own overwrite switch
/// (`spotify_overwrite`, `apple_overwrite`) rather than `pipeline_overwrite`.
pub async fn existing_final_file_for(
    dest_path: &Path,
    settings: &Settings,
    overwrite: bool,
) -> Option<PathBuf> {
    if overwrite {
        return None;
    }
    let mut candidates = vec![dest_path.to_path_buf()];
    if settings.conversion_check {
        if let Some(codec) = parse_codec(&settings.conversion_codec) {
            candidates.push(dest_path.with_extension(codec.container()));
        }
    }
    for c in candidates {
        if tokio::fs::try_exists(&c).await.unwrap_or(false) {
            return Some(c);
        }
    }
    None
}

pub async fn resolve_track_dest(
    dest: &Path,
    file_stem: &str,
    ext: &str,
    settings: &Settings,
    fields: &CommonTrackFields,
    placement: crate::services::common::pipeline::TrackPlacement,
) -> MhResult<(PathBuf, PathBuf)> {
    let file_name = format!("{}.{}", file_stem, ext);
    let track_dest = if !placement.in_collection() {
        let (quality_label, format_label) =
            crate::services::common::pipeline::converted_quality_format(
                settings,
                &fields.quality_label,
                &fields.format_label,
            );
        let folder = crate::services::common::pipeline::build_album_folder(
            &settings.filepaths_folder_format,
            &fields.albumartist,
            &fields.album,
            &fields.year,
            &fields.genre,
            &fields.label,
            &quality_label,
            &format_label,
            crate::services::common::pipeline::FolderNaming::from_settings(settings),
        );
        let d = dest.join(folder);
        tokio::fs::create_dir_all(&d).await?;
        d
    } else {
        dest.to_path_buf()
    };
    let dest_path = track_dest.join(&file_name);
    Ok((track_dest, dest_path))
}

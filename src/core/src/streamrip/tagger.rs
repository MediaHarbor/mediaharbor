use std::path::Path;
use std::process::Stdio;
use tempfile::NamedTempFile;
use tokio::process::Command;

use crate::errors::{MhError, MhResult};

#[derive(Debug, Default, Clone)]
pub struct TrackMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub total_tracks: Option<u32>,
    pub total_discs: Option<u32>,
    pub comment: Option<String>,
    pub lyrics: Option<String>,
    pub isrc: Option<String>,
    pub upc: Option<String>,
    pub copyright: Option<String>,
    pub label: Option<String>,
    pub composer: Option<String>,
    pub conductor: Option<String>,
    pub performer: Option<String>,
    pub lyricist: Option<String>,
    pub producer: Option<String>,
    pub engineer: Option<String>,
    pub mixer: Option<String>,
    pub description: Option<String>,
    pub purchase_date: Option<String>,
    pub grouping: Option<String>,
}

pub async fn download_cover_art(
    client: &reqwest::Client,
    url: &str,
) -> MhResult<NamedTempFile> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(MhError::Other(format!(
            "cover art download failed: HTTP {}",
            resp.status().as_u16()
        )));
    }
    let bytes = resp.bytes().await?;
    if bytes.is_empty() {
        return Err(MhError::Other("cover art download returned empty body".into()));
    }

    let mut tmp = NamedTempFile::new()?;
    use std::io::Write;
    tmp.write_all(&bytes)?;
    Ok(tmp)
}

pub async fn tag_file(
    path: &Path,
    metadata: &TrackMetadata,
    cover_path: Option<&Path>,
    exclude_tags: &[String],
    ffmpeg: &str,
) -> MhResult<()> {

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let tmp_out = {
        let ext_str = if ext.is_empty() {
            "tmp".to_string()
        } else {
            ext.clone()
        };
        let mut p = path.to_path_buf();
        p.set_extension(format!("tagged.tmp.{}", ext_str));
        p
    };

    let excluded: std::collections::HashSet<String> =
        exclude_tags.iter().map(|s| s.to_lowercase()).collect();

    let track_str = build_num_str(
        metadata.track_number,
        metadata.total_tracks,
        &excluded,
        "tracknumber",
    );
    let disc_str = build_num_str(
        metadata.disc_number,
        metadata.total_discs,
        &excluded,
        "discnumber",
    );

    let mut meta_args: Vec<String> = Vec::new();

    let field_map: &[(&str, &str)] = &[
        ("title", "title"),
        ("artist", "artist"),
        ("album", "album"),
        ("album_artist", "album_artist"),
        ("year", "date"),
        ("genre", "genre"),
        ("isrc", "ISRC"),
        ("comment", "comment"),
        ("copyright", "copyright"),
        ("lyrics", "LYRICS"),
        ("label", "organization"),
        ("upc", "BARCODE"),
        ("composer", "composer"),
        ("conductor", "conductor"),
        ("performer", "performer"),
        ("lyricist", "lyricist"),
        ("producer", "producer"),
        ("engineer", "engineer"),
        ("mixer", "mixer"),
        ("description", "description"),
        ("purchase_date", "purchase_date"),
        ("grouping", "grouping"),
    ];

    for (field, ff_key) in field_map {
        if excluded.contains(*field) {
            continue;
        }
        let value = match *field {
            "title" => metadata.title.as_deref(),
            "artist" => metadata.artist.as_deref(),
            "album" => metadata.album.as_deref(),
            "album_artist" => metadata.album_artist.as_deref(),
            "year" => metadata.year.as_deref(),
            "genre" => metadata.genre.as_deref(),
            "isrc" => metadata.isrc.as_deref(),
            "comment" => metadata.comment.as_deref(),
            "copyright" => metadata.copyright.as_deref(),
            "lyrics" => metadata.lyrics.as_deref(),
            "label" => metadata.label.as_deref(),
            "upc" => metadata.upc.as_deref(),
            "composer" => metadata.composer.as_deref(),
            "conductor" => metadata.conductor.as_deref(),
            "performer" => metadata.performer.as_deref(),
            "lyricist" => metadata.lyricist.as_deref(),
            "producer" => metadata.producer.as_deref(),
            "engineer" => metadata.engineer.as_deref(),
            "mixer" => metadata.mixer.as_deref(),
            "description" => metadata.description.as_deref(),
            "purchase_date" => metadata.purchase_date.as_deref(),
            "grouping" => metadata.grouping.as_deref(),
            _ => None,
        };
        if let Some(v) = value {
            if !v.is_empty() {
                meta_args.push("-metadata".into());
                meta_args.push(format!("{}={}", ff_key, v));
            }
        }
    }

    if let Some(ref ts) = track_str {
        meta_args.push("-metadata".into());
        meta_args.push(format!("track={}", ts));
    }
    if let Some(ref ds) = disc_str {
        meta_args.push("-metadata".into());
        meta_args.push(format!("disc={}", ds));
    }

    let mut args: Vec<String> = vec![
        "-y".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        path.to_string_lossy().into_owned(),
    ];

    if let Some(cover) = cover_path {
        let cover_str = cover.to_string_lossy().into_owned();
        args.push("-i".into());
        args.push(cover_str);
        args.push("-map".into());
        args.push("0:a".into());
        args.push("-map".into());
        args.push("1:v".into());

        match ext.as_str() {
            "mp3" => {
                args.extend([
                    "-c:a".into(), "copy".into(),
                    "-c:v".into(), "copy".into(),
                    "-id3v2_version".into(), "3".into(),
                    "-metadata:s:v".into(), "title=Album cover".into(),
                    "-metadata:s:v".into(), "comment=Cover (front)".into(),
                ]);
            }
            "flac" => {
                args.extend([
                    "-c:a".into(), "copy".into(),
                    "-c:v".into(), "mjpeg".into(),
                    "-q:v".into(), "2".into(),
                    "-disposition:v".into(), "attached_pic".into(),
                    "-metadata:s:v".into(), "title=Album cover".into(),
                    "-metadata:s:v".into(), "comment=Cover (front)".into(),
                ]);
            }
            "ogg" | "opus" => {
                args.extend([
                    "-c:a".into(), "copy".into(),
                    "-c:v".into(), "copy".into(),
                    "-disposition:v".into(), "attached_pic".into(),
                    "-metadata:s:v".into(), "title=Album cover".into(),
                    "-metadata:s:v".into(), "comment=Cover (front)".into(),
                ]);
            }
            _ => {
                args.extend(["-c:a".into(), "copy".into(), "-c:v".into(), "copy".into()]);
            }
        }
    } else {
        args.extend(["-map".into(), "0:a".into(), "-c:a".into(), "copy".into()]);
    }

    args.extend(meta_args);
    args.push(tmp_out.to_string_lossy().into_owned());

    let status = Command::new(ffmpeg)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await
        .map_err(|e| MhError::Subprocess(format!("failed to spawn ffmpeg: {}", e)))?;

    if !status.success() {
        let _ = tokio::fs::remove_file(&tmp_out).await;
        return Err(MhError::Subprocess(format!(
            "ffmpeg tagger failed with code {}",
            status.code().unwrap_or(-1)
        )));
    }

    tokio::fs::remove_file(path).await?;
    tokio::fs::rename(&tmp_out, path).await?;

    Ok(())
}

fn build_num_str(
    num: Option<u32>,
    total: Option<u32>,
    excluded: &std::collections::HashSet<String>,
    field: &str,
) -> Option<String> {
    if excluded.contains(field) {
        return None;
    }
    num.map(|n| match total {
        Some(t) => format!("{}/{}", n, t),
        None => n.to_string(),
    })
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use futures::stream::{self, StreamExt};
use image::imageops::FilterType;
use lofty::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::errors::{MhError, MhResult};
use crate::media::file_discovery::{discover_files, VIDEO_FORMATS};
use crate::subprocess;

const CACHE_VERSION: u32 = 2;

#[cfg(target_os = "windows")]
const SCAN_CONCURRENCY: usize = 4;
#[cfg(not(target_os = "windows"))]
const SCAN_CONCURRENCY: usize = 8;

const FFMPEG_THUMB_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub duration_secs: Option<f64>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub cover_thumbnail: Option<String>,
    pub is_video: bool,
    pub file_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub title: String,
    pub artist: String,
    pub year: Option<String>,
    pub tracks: Vec<MediaItem>,
}

pub struct CacheManager {
    pub dir: PathBuf,
    pub version: u32,
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    items: Vec<MediaItem>,
}

impl CacheManager {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, version: CACHE_VERSION }
    }

    fn cache_path(&self, scan_dir: &Path) -> PathBuf {
        use md5::{Digest, Md5};
        let mut hasher = Md5::new();
        hasher.update(scan_dir.to_string_lossy().as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        self.dir.join(format!("{}.json", hash))
    }

    pub fn load(&self, dir: &Path) -> Option<Vec<MediaItem>> {
        let path = self.cache_path(dir);
        let data = std::fs::read_to_string(&path).ok()?;
        let cf: CacheFile = serde_json::from_str(&data).ok()?;
        if cf.version != self.version {
            return None;
        }
        Some(cf.items)
    }

    pub fn save(&self, dir: &Path, items: &[MediaItem]) {
        let path = self.cache_path(dir);
        let _ = std::fs::create_dir_all(&self.dir);
        let cf = CacheFile { version: self.version, items: items.to_vec() };
        if let Ok(json) = serde_json::to_string(&cf) {
            let _ = std::fs::write(&path, json);
        }
    }
}

pub async fn scan_directory(dir: &Path, cache: &CacheManager, force: bool) -> MhResult<Vec<MediaItem>> {
    let cached = cache.load(dir).unwrap_or_default();
    if !force && !cached.is_empty() {
        return Ok(cached);
    }

    let files = discover_files(dir)?;

    let prior: HashMap<PathBuf, MediaItem> = cached
        .into_iter()
        .map(|item| (item.path.clone(), item))
        .collect();

    let items: Vec<MediaItem> = stream::iter(files.into_iter().map(|path| {
        let prior_entry = prior.get(&path).cloned();
        async move {
            let is_video = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| VIDEO_FORMATS.contains(&e.to_lowercase().as_str()))
                .unwrap_or(false);

            if is_video {
                let cur_size = tokio::fs::metadata(&path).await.map(|m| m.len()).ok();
                if let (Some(prev), Some(size)) = (prior_entry.as_ref(), cur_size) {
                    if prev.is_video && prev.file_size == size {
                        return Some(prev.clone());
                    }
                }
                process_video_file(&path, "ffmpeg").await.ok()
            } else {
                process_audio_file(&path).await.ok()
            }
        }
    }))
    .buffer_unordered(SCAN_CONCURRENCY)
    .filter_map(|x| async move { x })
    .collect()
    .await;

    cache.save(dir, &items);
    Ok(items)
}

pub fn organize_into_albums(items: &[MediaItem]) -> Vec<Album> {
    let mut map: HashMap<String, Album> = HashMap::new();
    let mut seen_paths: HashMap<String, std::collections::HashSet<PathBuf>> = HashMap::new();

    for item in items {
        if item.is_video {
            continue;
        }
        let album_title = item.album.clone().unwrap_or_else(|| "Unknown".into());
        let album_artist = item.artist.clone().unwrap_or_else(|| "Unknown".into());

        let norm_album = album_title.trim().to_lowercase();
        let norm_artist = album_artist.trim().to_lowercase();
        let key = format!("{}::{}", norm_album, norm_artist);

        let album = map.entry(key.clone()).or_insert_with(|| Album {
            title: album_title.clone(),
            artist: album_artist.clone(),
            year: item.year.clone(),
            tracks: Vec::new(),
        });
        let paths = seen_paths.entry(key).or_insert_with(std::collections::HashSet::new);

        if paths.insert(item.path.clone()) {
            album.tracks.push(item.clone());
        }
    }

    map.into_values().collect()
}

pub async fn process_audio_file(path: &Path) -> MhResult<MediaItem> {
    let _file_size_hint = tokio::fs::metadata(path).await?.len();
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();

    let path_owned = path.to_path_buf();
    let result: MhResult<MediaItem> = tokio::task::spawn_blocking(move || {
        let tagged_file = lofty::read_from_path(&path_owned)
            .map_err(|e| MhError::Other(format!("lofty: {}", e)))?;

        let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());

        let (
            title_tag,
            artist,
            album,
            year,
            genre,
            track_number,
            disc_number,
            cover_thumbnail,
        ) = if let Some(t) = tag {
            let cover_bytes: Option<Vec<u8>> = t.pictures().first().map(|pic| pic.data().to_vec());

            let thumb = cover_bytes.as_ref().and_then(|b| resize_thumbnail(b).ok());

            (
                t.title().map(|s| s.to_string()),
                t.artist().map(|s| s.to_string()),
                t.album().map(|s| s.to_string()),
                t.year().map(|y| y.to_string()),
                t.genre().map(|s| s.to_string()),
                t.track(),
                t.disk(),
                thumb,
            )
        } else {
            (None, None, None, None, None, None, None, None)
        };

        let duration_secs = tagged_file.properties().duration().as_secs_f64();
        let duration_secs = if duration_secs > 0.0 { Some(duration_secs) } else { None };

        let file_size = std::fs::metadata(&path_owned)?.len();

        Ok(MediaItem {
            path: path_owned,
            title: title_tag.unwrap_or(title),
            artist,
            album,
            year,
            genre,
            duration_secs,
            track_number,
            disc_number,
            cover_thumbnail,
            is_video: false,
            file_size,
        })
    })
    .await
    .map_err(|e| MhError::Other(format!("spawn_blocking panic: {}", e)))?;

    result
}

pub async fn process_video_file(path: &Path, ffmpeg: &str) -> MhResult<MediaItem> {
    let file_size = tokio::fs::metadata(path).await?.len();
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();

    let mut item = MediaItem {
        path: path.to_path_buf(),
        title,
        artist: None,
        album: None,
        year: None,
        genre: None,
        duration_secs: None,
        track_number: None,
        disc_number: None,
        cover_thumbnail: None,
        is_video: true,
        file_size,
    };

    if file_size == 0 {
        return Ok(item);
    }

    let tmp = tempfile::Builder::new()
        .suffix(".jpg")
        .tempfile()
        .map_err(MhError::Io)?;
    let tmp_path = tmp.path().to_path_buf();

    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-ss", "1",
        "-i", &path.to_string_lossy(),
        "-vframes", "1",
        "-vf", "scale=200:-1",
        "-y",
        &tmp_path.to_string_lossy(),
    ])
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .stdin(std::process::Stdio::null());
    subprocess::apply_no_window(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return Ok(item),
    };

    let success = match tokio::time::timeout(FFMPEG_THUMB_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => status.success(),
        _ => {
            let _ = child.kill().await;
            false
        }
    };

    if success {
        item.cover_thumbnail = tokio::fs::read(&tmp_path)
            .await
            .ok()
            .and_then(|b| resize_thumbnail(&b).ok());
    }

    Ok(item)
}

fn resize_thumbnail(data: &[u8]) -> MhResult<String> {
    let img = image::load_from_memory(data)
        .map_err(|e| MhError::Other(format!("image decode: {}", e)))?;
    let resized = img.resize(200, 200, FilterType::Lanczos3);

    let mut buf = std::io::Cursor::new(Vec::new());
    resized
        .write_to(&mut buf, image::ImageFormat::Jpeg)
        .map_err(|e| MhError::Other(format!("image encode: {}", e)))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(buf.into_inner()))
}

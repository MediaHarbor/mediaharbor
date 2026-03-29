use std::path::{Path, PathBuf};
use rayon::prelude::*;

use crate::errors::{MhError, MhResult};

pub const VIDEO_FORMATS: &[&str] = &["mkv", "mp4", "flv", "avi", "mov", "webm"];
pub const MUSIC_FORMATS: &[&str] = &["opus", "flac", "mp3", "aac", "m4a", "wav"];

pub fn is_media_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_lowercase();
        VIDEO_FORMATS.contains(&ext_lower.as_str()) || MUSIC_FORMATS.contains(&ext_lower.as_str())
    } else {
        false
    }
}

pub fn discover_files(dir: &Path) -> MhResult<Vec<PathBuf>> {
    collect_all(dir)
}

pub fn discover_music_files(dir: &Path) -> MhResult<Vec<PathBuf>> {
    Ok(collect_all(dir)?
        .into_iter()
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| MUSIC_FORMATS.contains(&e.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .collect())
}

pub fn discover_video_files(dir: &Path) -> MhResult<Vec<PathBuf>> {
    Ok(collect_all(dir)?
        .into_iter()
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| VIDEO_FORMATS.contains(&e.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .collect())
}

fn collect_all(dir: &Path) -> MhResult<Vec<PathBuf>> {
    let mut all_files: Vec<PathBuf> = Vec::new();
    walk_sync(dir, &mut all_files)?;

    let media: Vec<PathBuf> = all_files
        .into_par_iter()
        .filter(|p| is_media_file(p))
        .collect();

    Ok(media)
}

fn walk_sync(dir: &Path, out: &mut Vec<PathBuf>) -> MhResult<()> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| MhError::Io(e))?;

    for entry in entries {
        let entry = entry.map_err(|e| MhError::Io(e))?;
        let path = entry.path();
        let ft = entry.file_type().map_err(|e| MhError::Io(e))?;
        if ft.is_dir() {
            walk_sync(&path, out)?;
        } else if ft.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

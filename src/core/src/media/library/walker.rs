use std::path::{Path, PathBuf};

use crate::errors::{MhError, MhResult};
use crate::media::file_discovery::{MUSIC_FORMATS, VIDEO_FORMATS};

#[derive(Debug, Clone)]
pub struct FsEntry {
    pub path: PathBuf,
    pub size: i64,
    pub mtime_ns: i64,
    pub is_video: bool,
}

const SKIP_DIRS: &[&str] = &[
    "@eaDir",
    "$RECYCLE.BIN",
    "node_modules",
    ".git",
    "System Volume Information",
    "Thumbs.db",
    // Game and app data. A single Minecraft resource pack contributed four thousand
    // .ogg sound effects to a scan of ~/Downloads, which is both a useless library and
    // the reason a rescan took minutes.
    ".minecraft",
    "minecraftWorlds",
    "resource_packs",
    "behavior_packs",
    "assets",
    "AppData",
    "Library",
];

pub async fn walk(root: PathBuf) -> MhResult<Vec<FsEntry>> {
    tokio::task::spawn_blocking(move || walk_blocking(&root))
        .await
        .map_err(|e| MhError::Other(format!("walker join: {}", e)))?
}

fn walk_blocking(root: &Path) -> MhResult<Vec<FsEntry>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let walker = jwalk::WalkDir::new(root)
        .skip_hidden(true)
        .parallelism(jwalk::Parallelism::RayonNewPool(4))
        .process_read_dir(|_depth, _path, _read_dir_state, children| {
            children.retain(|res| {
                let Ok(entry) = res else { return true };
                let name = entry.file_name().to_string_lossy();
                if name.starts_with('.') {
                    return false;
                }
                if entry.file_type().is_dir() {
                    !SKIP_DIRS.iter().any(|s| name.eq_ignore_ascii_case(s))
                } else {
                    true
                }
            });
        });

    let mut out: Vec<FsEntry> = Vec::new();
    for res in walker {
        let entry = match res {
            Ok(e) => e,
            Err(_) => continue,
        };
        let ft = entry.file_type();
        if !ft.is_file() {
            continue;
        }
        let path = entry.path();
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(s) => s.to_ascii_lowercase(),
            None => continue,
        };
        let is_video = VIDEO_FORMATS.contains(&ext.as_str());
        let is_audio = MUSIC_FORMATS.contains(&ext.as_str());
        if !is_video && !is_audio {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if name.ends_with(".tmp") || name.ends_with(".part") || name.ends_with(".crdownload") {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = meta.len() as i64;
        let mtime_ns = mtime_to_ns(&meta);
        out.push(FsEntry {
            path,
            size,
            mtime_ns,
            is_video,
        });
    }
    Ok(out)
}

fn mtime_to_ns(meta: &std::fs::Metadata) -> i64 {
    use std::time::UNIX_EPOCH;
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

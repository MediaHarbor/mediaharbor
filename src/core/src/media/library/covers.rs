use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use image::imageops::FilterType;

use crate::errors::{MhError, MhResult};

/// Covers already cached at the previous 256px are left alone rather than re-rendered:
/// discarding the cache made the first scan after an upgrade re-encode every thumbnail
/// in the library at once, which is a lot of work to sharpen art the user is not looking
/// at. New art renders at this size and the cache converges as the library changes.
const COVER_SIZE: u32 = 640;
const FOLDER_ART_NAMES: &[&str] = &[
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "folder.jpg",
    "folder.jpeg",
    "folder.png",
    "front.jpg",
    "front.jpeg",
    "front.png",
    "album.jpg",
    "album.png",
];

pub fn is_folder_art_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    FOLDER_ART_NAMES.contains(&lower.as_str())
}

pub struct CoverCache {
    pub dir: PathBuf,
    folder_memo: Mutex<HashMap<PathBuf, Option<PathBuf>>>,
}

impl CoverCache {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Self {
            dir,
            folder_memo: Mutex::new(HashMap::new()),
        }
    }

    pub fn path_for(&self, cover_id: &str) -> PathBuf {
        self.dir.join(format!("{}.jpg", cover_id))
    }

    pub fn find_folder_art(&self, parent: &Path) -> Option<PathBuf> {
        {
            let memo = self.folder_memo.lock().unwrap();
            if let Some(v) = memo.get(parent) {
                return v.clone();
            }
        }
        let found = scan_folder_art(parent);
        self.folder_memo
            .lock()
            .unwrap()
            .insert(parent.to_path_buf(), found.clone());
        found
    }

    pub fn ingest_folder_art(&self, art_path: &Path) -> MhResult<String> {
        let cover_id = self.id_for_folder_art(art_path)?;
        self.materialize_folder_art(&cover_id, art_path)?;
        Ok(cover_id)
    }

    pub fn ingest_bytes(&self, bytes: &[u8]) -> MhResult<String> {
        let cover_id = Self::id_for_bytes(bytes);
        self.materialize_bytes(&cover_id, bytes)?;
        Ok(cover_id)
    }

    pub fn has(&self, cover_id: &str) -> bool {
        self.path_for(cover_id).exists()
    }

    pub fn id_for_bytes(bytes: &[u8]) -> String {
        short_hex(blake3::hash(bytes).as_bytes())
    }

    pub fn id_for_folder_art(&self, art_path: &Path) -> MhResult<String> {
        let meta = std::fs::metadata(art_path).map_err(MhError::Io)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(art_path.to_string_lossy().as_bytes());
        hasher.update(&meta.len().to_le_bytes());
        if let Ok(mt) = meta.modified() {
            if let Ok(d) = mt.duration_since(std::time::UNIX_EPOCH) {
                hasher.update(&d.as_nanos().to_le_bytes());
            }
        }
        Ok(short_hex(hasher.finalize().as_bytes()))
    }

    pub fn materialize_bytes(&self, cover_id: &str, bytes: &[u8]) -> MhResult<()> {
        let out_path = self.path_for(cover_id);
        if !out_path.exists() {
            write_resized(bytes, &out_path)?;
        }
        Ok(())
    }

    pub fn materialize_folder_art(&self, cover_id: &str, art_path: &Path) -> MhResult<()> {
        let out_path = self.path_for(cover_id);
        if !out_path.exists() {
            let bytes = std::fs::read(art_path).map_err(MhError::Io)?;
            write_resized(&bytes, &out_path)?;
        }
        Ok(())
    }
}

fn scan_folder_art(parent: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(parent).ok()?;
    let mut by_lower: HashMap<String, PathBuf> = HashMap::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        by_lower.insert(name, e.path());
    }
    for cand in FOLDER_ART_NAMES {
        if let Some(p) = by_lower.get(*cand) {
            return Some(p.clone());
        }
    }
    None
}

fn write_resized(bytes: &[u8], out: &Path) -> MhResult<()> {
    let img = crate::media::image_pipeline::decode_image(bytes)?;
    let resized = img.resize(COVER_SIZE, COVER_SIZE, FilterType::Lanczos3);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(MhError::Io)?;
    }
    let mut file = std::fs::File::create(out).map_err(MhError::Io)?;
    crate::media::image_pipeline::encode_jpeg(&resized, &mut file)
}

fn short_hex(bytes: &[u8]) -> String {
    hex::encode(&bytes[..bytes.len().min(8)])
}

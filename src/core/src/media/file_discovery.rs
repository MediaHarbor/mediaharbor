use std::path::Path;

pub const VIDEO_FORMATS: &[&str] = &["mkv", "mp4", "m4v", "flv", "avi", "mov", "webm"];
pub const MUSIC_FORMATS: &[&str] = &[
    "opus", "flac", "mp3", "aac", "m4a", "m4b", "ogg", "oga", "wav", "aiff", "aif", "ape", "wv",
    "mpc", "spx", "tta",
];

pub fn is_media_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => {
            let ext = ext.to_lowercase();
            VIDEO_FORMATS.contains(&ext.as_str()) || MUSIC_FORMATS.contains(&ext.as_str())
        }
        None => false,
    }
}

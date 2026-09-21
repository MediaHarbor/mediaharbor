pub fn is_flatpak() -> bool {
    std::env::var("FLATPAK_ID").is_ok()
}

pub fn is_snap() -> bool {
    std::env::var("SNAP").is_ok()
}

pub fn is_sandboxed() -> bool {
    is_flatpak() || is_snap()
}

pub fn snap_download_dir() -> Option<std::path::PathBuf> {
    std::env::var("SNAP_REAL_HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join("Downloads"))
}

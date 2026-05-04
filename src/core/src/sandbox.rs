pub fn is_flatpak() -> bool {
    std::env::var("FLATPAK_ID").is_ok()
}

pub fn is_snap() -> bool {
    std::env::var("SNAP").is_ok()
}

pub fn is_sandboxed() -> bool {
    is_flatpak() || is_snap()
}

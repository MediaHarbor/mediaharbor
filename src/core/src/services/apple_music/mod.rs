/// The `Origin`/`Referer` every AMP request carries. Declared once: it was two
/// constants with the same value in the same directory.
pub(crate) const APPLE_MUSIC_HOMEPAGE: &str = "https://music.apple.com";

pub mod api;
pub mod downloader;
pub mod gamdl;
pub mod http_stream;
pub mod library;
pub mod lyrics;
pub mod meta;
pub mod native_engine;
pub mod playback;
pub mod playback_provider;
pub mod search;
pub mod telemetry;
pub mod wrapper;

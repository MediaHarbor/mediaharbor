use async_trait::async_trait;

use crate::errors::MhResult;
use crate::{defaults::Settings, ipc_contract};

/// Resolves a media URL into something the WebView can play.
///
/// The sibling of [`SearchProvider`](super::search::SearchProvider) and
/// [`DownloadProvider`](super::download::DownloadProvider). Unlike those two,
/// the arms reach for ten different `BackendState` fields plus two of its
/// methods, so the whole state is passed rather than a narrower context.
#[async_trait]
pub trait PlaybackProvider: Send + Sync {
    async fn play(
        &self,
        req: &ipc_contract::PlayMediaRequest,
        settings: &Settings,
        state: &crate::BackendState,
    ) -> MhResult<ipc_contract::PlayMediaResponse>;
}

/// Which service a `play_media` request is for.
///
/// Separate from `ServicePlatform` because the IPC field is a bare string that
/// has accepted several spellings per service since before that enum existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackTarget {
    Youtube,
    YtMusic,
    Spotify,
    Tidal,
    Deezer,
    Qobuz,
    AppleMusic,
    Radio,
}

impl PlaybackTarget {
    /// Every spelling `play_media` has ever answered to. Narrowing this list
    /// would silently route a request to the local-file fallback instead.
    pub fn from_platform(s: &str) -> Option<Self> {
        match crate::services::common::library::normalize_platform_key(s).as_str() {
            "youtube" => Some(Self::Youtube),
            "youtubemusic" | "youtube_music" | "ytmusic" | "yt_music" => Some(Self::YtMusic),
            "spotify" => Some(Self::Spotify),
            "tidal" => Some(Self::Tidal),
            "deezer" => Some(Self::Deezer),
            "qobuz" => Some(Self::Qobuz),
            "applemusic" | "apple_music" => Some(Self::AppleMusic),
            "radio" => Some(Self::Radio),
            _ => None,
        }
    }
}

pub mod apple_music;
pub mod common;
pub mod deezer;
pub mod qobuz;
pub mod radio;
pub mod spotify;
pub mod tidal;
pub mod youtube;
pub mod ytmusic;

pub use common::lyrics::LyricsProvider;
pub use common::{
    DownloadContext, DownloadProvider, PlaybackProvider, PlaybackTarget, SearchContext,
    SearchProvider,
};

use crate::ipc_contract::SearchPlatform;

pub fn search_provider(platform: SearchPlatform) -> Option<Box<dyn SearchProvider>> {
    match platform {
        SearchPlatform::Deezer => Some(Box::new(deezer::search::DeezerSearch)),
        SearchPlatform::Qobuz => Some(Box::new(qobuz::search::QobuzSearch)),
        SearchPlatform::AppleMusic => Some(Box::new(apple_music::search::AppleMusicSearch)),
        SearchPlatform::YoutubeMusic => Some(Box::new(ytmusic::search::YtMusicSearch)),
        SearchPlatform::Youtube => Some(Box::new(youtube::search::YoutubeSearch)),
        SearchPlatform::Tidal => Some(Box::new(tidal::search::TidalSearch)),
        SearchPlatform::Spotify => Some(Box::new(spotify::search::SpotifySearch)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadTarget {
    YtMusic,
    YtVideo,
    Spotify,
    AppleMusic,
    Qobuz,
    Deezer,
    Tidal,
}

pub fn download_provider(
    target: DownloadTarget,
    state: &crate::BackendState,
) -> Box<dyn DownloadProvider> {
    match target {
        DownloadTarget::YtMusic => {
            Box::new(youtube::downloader::YtMusicDownloader::from_state(state))
        }
        DownloadTarget::YtVideo => {
            Box::new(youtube::downloader::YtVideoDownloader::from_state(state))
        }
        DownloadTarget::Spotify => {
            Box::new(spotify::downloader::SpotifyDownloader::from_state(state))
        }
        DownloadTarget::AppleMusic => Box::new(apple_music::downloader::AppleMusicDownloader),
        DownloadTarget::Qobuz => Box::new(qobuz::downloader::QobuzDownloader::from_state(state)),
        DownloadTarget::Deezer => Box::new(deezer::downloader::DeezerDownloader),
        DownloadTarget::Tidal => Box::new(tidal::downloader::TidalDownloader),
    }
}

pub fn playback_provider(target: PlaybackTarget) -> Box<dyn PlaybackProvider> {
    match target {
        PlaybackTarget::Youtube => Box::new(youtube::playback_provider::YoutubePlayback),
        PlaybackTarget::YtMusic => Box::new(ytmusic::playback_provider::YtMusicPlayback),
        PlaybackTarget::Spotify => Box::new(spotify::playback_provider::SpotifyPlayback),
        PlaybackTarget::Tidal => Box::new(tidal::playback_provider::TidalPlayback),
        PlaybackTarget::Deezer => Box::new(deezer::playback_provider::DeezerPlayback),
        PlaybackTarget::Qobuz => Box::new(qobuz::playback_provider::QobuzPlayback),
        PlaybackTarget::AppleMusic => Box::new(apple_music::playback_provider::AppleMusicPlayback),
        PlaybackTarget::Radio => Box::new(radio::playback_provider::RadioPlayback),
    }
}

/// Keyed on [`PlaybackTarget`] rather than a raw string so the platform aliases are
/// parsed in one place — `PlaybackTarget::from_platform` already knows every spelling.
pub fn lyrics_provider(target: PlaybackTarget) -> Option<Box<dyn LyricsProvider>> {
    match target {
        PlaybackTarget::Tidal => Some(Box::new(tidal::lyrics::TidalLyrics)),
        PlaybackTarget::Deezer => Some(Box::new(deezer::lyrics::DeezerLyrics)),
        PlaybackTarget::Spotify => Some(Box::new(spotify::lyrics::SpotifyLyrics)),
        PlaybackTarget::AppleMusic => Some(Box::new(apple_music::lyrics::AppleMusicLyrics)),
        PlaybackTarget::YtMusic => Some(Box::new(ytmusic::lyrics::YtMusicLyrics)),
        // Qobuz exposes no lyrics endpoint; YouTube and radio have no track identity to look up.
        PlaybackTarget::Qobuz | PlaybackTarget::Youtube | PlaybackTarget::Radio => None,
    }
}

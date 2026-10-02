use async_trait::async_trait;

use crate::ipc_contract::{GetLyricsRequest, GetLyricsResponse};
use crate::services::common::lyrics::{empty_response, LyricsProvider};

pub struct AppleMusicLyrics;

#[async_trait]
impl LyricsProvider for AppleMusicLyrics {
    async fn fetch(
        &self,
        req: &GetLyricsRequest,
        _settings: &crate::defaults::Settings,
        state: &crate::BackendState,
    ) -> GetLyricsResponse {
        let apple = state.apple_music.read().await;
        if !apple.is_configured() {
            return empty_response();
        }
        match apple.fetch_lyrics(&req.url).await {
            Some((synced, plain, word_synced)) => GetLyricsResponse {
                synced,
                plain,
                word_synced,
            },
            None => empty_response(),
        }
    }
}

use async_trait::async_trait;

use crate::ipc_contract::{GetLyricsRequest, GetLyricsResponse};
use crate::services::common::lyrics::{
    empty_response, fetch_public_word_lyrics, found_to_response, lyric_deezer_client,
    parse_deezer_gw_lyrics, LyricsProvider,
};
use crate::services::common::pipeline::orchestrator::{extract_platform_id, ContentType, Platform};

pub struct DeezerLyrics;

fn track_id(url: &str) -> Option<String> {
    extract_platform_id(url, Platform::Deezer, ContentType::Track)
}

#[async_trait]
impl LyricsProvider for DeezerLyrics {
    async fn fetch(
        &self,
        req: &GetLyricsRequest,
        settings: &crate::defaults::Settings,
        _state: &crate::BackendState,
    ) -> GetLyricsResponse {
        // Signed out: the public endpoint still serves word timings, if the user opted in.
        if settings.deezer_arl.is_empty() {
            if !settings.deezer_lrc_public_fallback {
                return empty_response();
            }
            let Some(track_id) = track_id(&req.url) else {
                return empty_response();
            };
            let lyrics = fetch_public_word_lyrics(&track_id).await;
            return GetLyricsResponse {
                synced: lyrics.as_ref().and_then(|l| l.to_lrc()),
                plain: None,
                word_synced: lyrics.as_ref().and_then(|l| l.to_json()),
            };
        }

        let (Some(client), Some(track_id)) = (
            lyric_deezer_client(&settings.deezer_arl).await,
            track_id(&req.url),
        ) else {
            return empty_response();
        };
        let (word, gw) = tokio::join!(
            client.fetch_word_lyrics(&track_id),
            client.get_lyrics(&track_id)
        );
        let mut found = gw
            .ok()
            .map(|p| parse_deezer_gw_lyrics(&p))
            .unwrap_or_default();
        found.word = word;
        found_to_response(found.filled())
    }
}

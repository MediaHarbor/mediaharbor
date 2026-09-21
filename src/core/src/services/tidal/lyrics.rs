use async_trait::async_trait;

use crate::ipc_contract::{GetLyricsRequest, GetLyricsResponse};
use crate::services::common::lyrics::{empty_response, LyricsProvider};

pub struct TidalLyrics;

#[async_trait]
impl LyricsProvider for TidalLyrics {
    async fn fetch(
        &self,
        req: &GetLyricsRequest,
        settings: &crate::defaults::Settings,
        state: &crate::BackendState,
    ) -> GetLyricsResponse {
        let Ok(client) = state.authenticate_tidal(settings).await else {
            return empty_response();
        };
        let Some(track_id) = crate::extract_tidal_track_id(&req.url) else {
            return empty_response();
        };
        let Some(lyr) = client.fetch_lyrics(&track_id).await else {
            return empty_response();
        };
        GetLyricsResponse {
            synced: lyr["subtitles"].as_str().map(|s| s.to_string()),
            plain: lyr["lyrics"].as_str().map(|s| s.to_string()),
            word_synced: None,
        }
    }
}

use std::sync::LazyLock;

use async_trait::async_trait;

use crate::ipc_contract::{GetLyricsRequest, GetLyricsResponse};
use crate::services::common::lyrics::{empty_response, LyricsProvider};

static YT_VIDEO_ID_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"[?&]v=([a-zA-Z0-9_-]{11})").unwrap());

fn video_id(url: &str) -> Option<String> {
    if let Some(caps) = YT_VIDEO_ID_RE.captures(url) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }
    if url.len() == 11
        && url
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Some(url.to_string());
    }
    None
}

pub struct YtMusicLyrics;

#[async_trait]
impl LyricsProvider for YtMusicLyrics {
    async fn fetch(
        &self,
        req: &GetLyricsRequest,
        _settings: &crate::defaults::Settings,
        _state: &crate::BackendState,
    ) -> GetLyricsResponse {
        let Some(video_id) = video_id(&req.url) else {
            return empty_response();
        };
        let Ok(client) = crate::services::ytmusic::api::YtMusicClient::shared().await else {
            return empty_response();
        };
        match client.fetch_lyrics(&video_id).await {
            Some((synced, plain)) => GetLyricsResponse {
                synced,
                plain,
                word_synced: None,
            },
            None => empty_response(),
        }
    }
}

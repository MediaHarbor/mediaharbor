use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_client;
use crate::services::common::http::read_json;
use crate::services::common::library::string_at;

const BASE_URL: &str = "https://www.googleapis.com/youtube/v3";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtVideoResult {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub thumbnail: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtPlaylistResult {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub thumbnail: Option<String>,
    pub url: String,
    pub track_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtChannelResult {
    pub id: String,
    pub title: String,
    pub thumbnail: Option<String>,
    pub url: String,
}

#[derive(Clone)]
pub struct YtSearchClient {
    pub api_key: String,
    http: reqwest::Client,
}

impl YtSearchClient {
    pub fn new(api_key: impl Into<String>) -> MhResult<Self> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(MhError::Auth(
                "YouTube API key not configured. Add it in Settings → API Keys.".to_string(),
            ));
        }
        Ok(Self {
            api_key,
            http: build_client()?,
        })
    }

    async fn request(&self, endpoint: &str, params: &[(&str, &str)]) -> MhResult<Value> {
        let url = format!("{}/{}", BASE_URL, endpoint);
        let resp = self
            .http
            .get(&url)
            .query(params)
            .query(&[("key", self.api_key.as_str())])
            .send()
            .await
            .map_err(MhError::Network)?;

        read_json("YouTube", resp).await
    }

    async fn search_raw(
        &self,
        search_type: &str,
        query: &str,
        max_results: u32,
    ) -> MhResult<Vec<Value>> {
        let max_str = max_results.to_string();
        let data = self
            .request(
                "search",
                &[
                    ("q", query),
                    ("part", "snippet"),
                    ("type", search_type),
                    ("maxResults", max_str.as_str()),
                ],
            )
            .await?;

        Ok(data["items"].as_array().cloned().unwrap_or_default())
    }

    pub async fn search_videos(&self, query: &str, limit: u32) -> MhResult<serde_json::Value> {
        let items = self.search_raw("video", query, limit).await?;
        let results: Vec<YtVideoResult> = items
            .into_iter()
            .filter_map(|item| {
                let video_id = item["id"]["videoId"].as_str()?.to_string();
                let snippet = &item["snippet"];
                Some(YtVideoResult {
                    id: video_id.clone(),
                    title: string_at(snippet, &["/title"]),
                    channel: string_at(snippet, &["/channelTitle"]),
                    thumbnail: pick_thumbnail(snippet),
                    url: format!("https://www.youtube.com/watch?v={}", video_id),
                })
            })
            .collect();
        Ok(serde_json::to_value(results)?)
    }

    pub async fn search_playlists(&self, query: &str, limit: u32) -> MhResult<serde_json::Value> {
        let items = self.search_raw("playlist", query, limit).await?;
        let results: Vec<YtPlaylistResult> = items
            .into_iter()
            .filter_map(|item| {
                let playlist_id = item["id"]["playlistId"].as_str()?.to_string();
                let snippet = &item["snippet"];
                Some(YtPlaylistResult {
                    id: playlist_id.clone(),
                    title: string_at(snippet, &["/title"]),
                    channel: string_at(snippet, &["/channelTitle"]),
                    thumbnail: pick_thumbnail(snippet),
                    url: format!("https://www.youtube.com/playlist?list={}", playlist_id),
                    track_count: None,
                })
            })
            .collect();
        Ok(serde_json::to_value(results)?)
    }

    pub async fn search_channels(&self, query: &str, limit: u32) -> MhResult<serde_json::Value> {
        let items = self.search_raw("channel", query, limit).await?;
        let results: Vec<YtChannelResult> = items
            .into_iter()
            .filter_map(|item| {
                let channel_id = item["id"]["channelId"].as_str()?.to_string();
                let snippet = &item["snippet"];
                Some(YtChannelResult {
                    id: channel_id.clone(),
                    title: string_at(snippet, &["/title"]),
                    thumbnail: pick_thumbnail(snippet),
                    url: format!("https://www.youtube.com/channel/{}", channel_id),
                })
            })
            .collect();
        Ok(serde_json::to_value(results)?)
    }
}

fn pick_thumbnail(snippet: &Value) -> Option<String> {
    snippet["thumbnails"]["medium"]["url"]
        .as_str()
        .or_else(|| snippet["thumbnails"]["default"]["url"].as_str())
        .map(|s| s.to_string())
}

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_client;

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

        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }
        Ok(resp.json().await.map_err(MhError::Network)?)
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

        Ok(data["items"]
            .as_array()
            .cloned()
            .unwrap_or_default())
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
                    title: snippet["title"].as_str().unwrap_or("").to_string(),
                    channel: snippet["channelTitle"].as_str().unwrap_or("").to_string(),
                    thumbnail: pick_thumbnail(snippet),
                    url: format!("https://www.youtube.com/watch?v={}", video_id),
                })
            })
            .collect();
        Ok(serde_json::to_value(results)?)
    }

    pub async fn search_playlists(
        &self,
        query: &str,
        limit: u32,
    ) -> MhResult<serde_json::Value> {
        let items = self.search_raw("playlist", query, limit).await?;
        let results: Vec<YtPlaylistResult> = items
            .into_iter()
            .filter_map(|item| {
                let playlist_id = item["id"]["playlistId"].as_str()?.to_string();
                let snippet = &item["snippet"];
                Some(YtPlaylistResult {
                    id: playlist_id.clone(),
                    title: snippet["title"].as_str().unwrap_or("").to_string(),
                    channel: snippet["channelTitle"].as_str().unwrap_or("").to_string(),
                    thumbnail: pick_thumbnail(snippet),
                    url: format!("https://www.youtube.com/playlist?list={}", playlist_id),
                    track_count: None,
                })
            })
            .collect();
        Ok(serde_json::to_value(results)?)
    }

    pub async fn search_channels(
        &self,
        query: &str,
        limit: u32,
    ) -> MhResult<serde_json::Value> {
        let items = self.search_raw("channel", query, limit).await?;
        let results: Vec<YtChannelResult> = items
            .into_iter()
            .filter_map(|item| {
                let channel_id = item["id"]["channelId"].as_str()?.to_string();
                let snippet = &item["snippet"];
                Some(YtChannelResult {
                    id: channel_id.clone(),
                    title: snippet["title"].as_str().unwrap_or("").to_string(),
                    thumbnail: pick_thumbnail(snippet),
                    url: format!("https://www.youtube.com/channel/{}", channel_id),
                })
            })
            .collect();
        Ok(serde_json::to_value(results)?)
    }

    pub async fn get_playlist_videos(&self, playlist_id: &str) -> MhResult<serde_json::Value> {
        let data = self
            .request(
                "playlistItems",
                &[
                    ("part", "snippet"),
                    ("playlistId", playlist_id),
                    ("maxResults", "50"),
                ],
            )
            .await?;

        let tracks: Vec<serde_json::Value> = data["items"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|item| {
                let snippet = &item["snippet"];
                let video_id = snippet["resourceId"]["videoId"].as_str()?.to_string();
                Some(serde_json::json!({
                    "id": video_id,
                    "title": snippet["title"].as_str().unwrap_or(""),
                    "artist": snippet["videoOwnerChannelTitle"].as_str().unwrap_or(""),
                    "thumbnail": pick_thumbnail(snippet),
                    "url": format!("https://www.youtube.com/watch?v={}", video_id),
                }))
            })
            .collect();

        let playlist_meta = self
            .request("playlists", &[("part", "snippet"), ("id", playlist_id)])
            .await
            .ok();

        let (pl_title, cover_url) = playlist_meta
            .as_ref()
            .and_then(|d| d["items"].as_array()?.first().cloned())
            .map(|item| {
                let snip = item["snippet"].clone();
                (
                    snip["title"].as_str().unwrap_or("").to_string(),
                    pick_thumbnail(&snip),
                )
            })
            .unwrap_or_default();

        Ok(serde_json::json!({
            "tracks": tracks,
            "playlist": {
                "title": pl_title,
                "coverUrl": cover_url,
            }
        }))
    }

    pub async fn get_channel_uploads(&self, channel_id: &str) -> MhResult<serde_json::Value> {
        let channel_data = self
            .request(
                "channels",
                &[
                    ("part", "contentDetails,snippet"),
                    ("id", channel_id),
                ],
            )
            .await?;

        let items = channel_data["items"].as_array().unwrap_or(&vec![]).clone();
        let channel_item = items.first().ok_or_else(|| {
            MhError::NotFound(format!("Channel {} not found", channel_id))
        })?;

        let uploads_id = channel_item["contentDetails"]["relatedPlaylists"]["uploads"]
            .as_str()
            .ok_or_else(|| MhError::NotFound("No uploads playlist for channel".into()))?
            .to_string();

        let snippet = &channel_item["snippet"];
        let channel_title = snippet["title"].as_str().unwrap_or("").to_string();
        let channel_thumb = pick_thumbnail(snippet);

        let playlist_data = self
            .request(
                "playlistItems",
                &[
                    ("part", "snippet"),
                    ("playlistId", &uploads_id),
                    ("maxResults", "50"),
                ],
            )
            .await?;

        let albums: Vec<serde_json::Value> = playlist_data["items"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|item| {
                let snip = &item["snippet"];
                let video_id = snip["resourceId"]["videoId"].as_str()?.to_string();
                Some(serde_json::json!({
                    "id": video_id,
                    "title": snip["title"].as_str().unwrap_or(""),
                    "thumbnail": pick_thumbnail(snip),
                    "url": format!("https://www.youtube.com/watch?v={}", video_id),
                }))
            })
            .collect();

        Ok(serde_json::json!({
            "albums": albums,
            "channel": {
                "title": channel_title,
                "thumbnail": channel_thumb,
            }
        }))
    }

    pub async fn get_channel_playlists(
        &self,
        channel_id: &str,
    ) -> MhResult<Vec<YtPlaylistResult>> {
        let data = self
            .request(
                "playlists",
                &[
                    ("part", "snippet,contentDetails"),
                    ("channelId", channel_id),
                    ("maxResults", "50"),
                ],
            )
            .await?;

        Ok(data["items"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|item| {
                let playlist_id = item["id"].as_str()?.to_string();
                let snippet = &item["snippet"];
                let track_count = item["contentDetails"]["itemCount"].as_u64();
                Some(YtPlaylistResult {
                    id: playlist_id.clone(),
                    title: snippet["title"].as_str().unwrap_or("").to_string(),
                    channel: snippet["channelTitle"].as_str().unwrap_or("").to_string(),
                    thumbnail: pick_thumbnail(snippet),
                    url: format!("https://www.youtube.com/playlist?list={}", playlist_id),
                    track_count,
                })
            })
            .collect())
    }
}

fn pick_thumbnail(snippet: &Value) -> Option<String> {
    snippet["thumbnails"]["medium"]["url"]
        .as_str()
        .or_else(|| snippet["thumbnails"]["default"]["url"].as_str())
        .map(|s| s.to_string())
}

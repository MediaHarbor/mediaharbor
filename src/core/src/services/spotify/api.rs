use std::sync::Arc;

use reqwest::Client;
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_client;
use crate::services::common::http::read_json;
use crate::services::common::oauth::TokenCache;

const API_URL: &str = "https://api.spotify.com/v1";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotifySearchType {
    Track,
    Album,
    Playlist,
    Artist,
    Episode,
    Show,
    Audiobook,
}

impl SpotifySearchType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Album => "album",
            Self::Playlist => "playlist",
            Self::Artist => "artist",
            Self::Episode => "episode",
            Self::Show => "show",
            Self::Audiobook => "audiobook",
        }
    }
}

#[derive(Clone)]
pub struct SpotifyApiClient {
    client_id: String,
    client_secret: String,
    token: Arc<TokenCache>,
    http: Client,
}

impl SpotifyApiClient {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> MhResult<Self> {
        let client_id = client_id.into();
        let client_secret = client_secret.into();
        if client_id.is_empty() || client_secret.is_empty() {
            return Err(MhError::Auth(
                "Spotify API credentials not configured. Add them in Settings → API Keys."
                    .to_string(),
            ));
        }
        Ok(Self {
            client_id,
            client_secret,
            token: Arc::new(TokenCache::default()),
            http: build_client()?,
        })
    }

    pub async fn ensure_token(&self) -> MhResult<String> {
        self.token
            .client_credentials(
                &self.http,
                TOKEN_URL,
                "Spotify",
                &self.client_id,
                &self.client_secret,
            )
            .await
    }

    async fn get(&self, url: &str, params: &[(&str, &str)]) -> MhResult<Value> {
        let token = self.ensure_token().await?;
        let mut req = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json");

        if !params.is_empty() {
            req = req.query(params);
        }

        let resp = req.send().await.map_err(MhError::Network)?;
        read_json("Spotify", resp).await
    }

    /// Fetch a paging object at `url` and follow its `next` cursor to completion,
    /// returning every `items` entry.
    async fn collect_paginated(&self, url: &str, params: &[(&str, &str)]) -> MhResult<Vec<Value>> {
        let first = self.get(url, params).await?;
        let mut items: Vec<Value> = first["items"].as_array().cloned().unwrap_or_default();
        let mut next = first["next"].as_str().map(String::from);
        while let Some(next_url) = next {
            let page = self.get(&next_url, &[]).await?;
            if let Some(arr) = page["items"].as_array() {
                items.extend(arr.iter().cloned());
            }
            next = page["next"].as_str().map(String::from);
        }
        Ok(items)
    }

    pub async fn search(
        &self,
        query: &str,
        search_type: SpotifySearchType,
        limit: u32,
        offset: u32,
    ) -> MhResult<Value> {
        let limit_str = limit.to_string();
        let offset_str = offset.to_string();
        let mut params = vec![
            ("q", query),
            ("type", search_type.as_str()),
            ("limit", limit_str.as_str()),
            ("offset", offset_str.as_str()),
        ];
        let needs_market = matches!(
            search_type,
            SpotifySearchType::Episode | SpotifySearchType::Show | SpotifySearchType::Audiobook
        );
        if needs_market {
            params.push(("market", "US"));
        }
        self.get(&format!("{}/search", API_URL), &params).await
    }

    pub async fn get_show_episodes(&self, show_id: &str) -> MhResult<Value> {
        let id = show_id.strip_prefix("spotify:show:").unwrap_or(show_id);

        let show_url = format!("{}/shows/{}", API_URL, id);
        let episodes_url = format!("{}/shows/{}/episodes", API_URL, id);

        let limit = crate::services::common::limits::SPOTIFY_PAGE_MAX.to_string();
        let ep_params = [("market", "US"), ("limit", limit.as_str())];
        let (show_data, episodes) = tokio::try_join!(
            self.get(&show_url, &[("market", "US")]),
            self.collect_paginated(&episodes_url, &ep_params)
        )?;

        Ok(serde_json::json!({
            "show_name":  show_data["name"],
            "publisher":  show_data["publisher"],
            "cover_url":  show_data["images"][0]["url"],
            "episodes":   Value::Array(episodes),
        }))
    }

    pub async fn get_track(&self, track_id: &str) -> MhResult<Value> {
        self.get(&format!("{}/tracks/{}", API_URL, track_id), &[])
            .await
    }

    pub async fn get_album(&self, album_id: &str) -> MhResult<Value> {
        self.get(&format!("{}/albums/{}", API_URL, album_id), &[])
            .await
    }
}

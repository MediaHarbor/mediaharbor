use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_client;

const API_URL: &str = "https://api.spotify.com/v1";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";

#[derive(Debug, Clone)]
struct TokenInfo {
    access_token: String,
    expires_at: Instant,
}

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
    token: Arc<RwLock<Option<TokenInfo>>>,
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
            token: Arc::new(RwLock::new(None)),
            http: build_client()?,
        })
    }

    pub async fn ensure_token(&self) -> MhResult<String> {
        {
            let guard = self.token.read().await;
            if let Some(ref t) = *guard {
                if Instant::now() < t.expires_at {
                    return Ok(t.access_token.clone());
                }
            }
        }

        let auth = B64.encode(format!("{}:{}", self.client_id, self.client_secret));
        let resp = self
            .http
            .post(TOKEN_URL)
            .header("Authorization", format!("Basic {}", auth))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("grant_type=client_credentials")
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Auth(format!(
                "Spotify authentication failed: {}",
                resp.status()
            )));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        let access_token = data["access_token"]
            .as_str()
            .ok_or_else(|| MhError::Auth("Missing access_token in Spotify response".to_string()))?
            .to_string();
        let expires_in = data["expires_in"].as_u64().unwrap_or(3600);
        let expires_at = Instant::now() + Duration::from_secs(expires_in.saturating_sub(60));

        let mut guard = self.token.write().await;
        *guard = Some(TokenInfo {
            access_token: access_token.clone(),
            expires_at,
        });

        Ok(access_token)
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
        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }
        Ok(resp.json().await.map_err(MhError::Network)?)
    }

    pub async fn search(
        &self,
        query: &str,
        search_type: SpotifySearchType,
        limit: u32,
    ) -> MhResult<Value> {
        let limit_str = limit.to_string();
        let mut params = vec![
            ("q", query),
            ("type", search_type.as_str()),
            ("limit", limit_str.as_str()),
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

    pub async fn get_album_tracks(&self, album_id: &str) -> MhResult<Value> {
        let album_url = format!("{}/albums/{}", API_URL, album_id);
        let tracks_url = format!("{}/albums/{}/tracks", API_URL, album_id);

        let (album_data, tracks_data) =
            tokio::try_join!(self.get(&album_url, &[]), self.get(&tracks_url, &[]))?;

        let album_url = format!("https://open.spotify.com/album/{}", album_id);
        Ok(serde_json::json!({
            "album_name":   album_data["name"],
            "release_date": album_data["release_date"],
            "artist_name":  album_data["artists"][0]["name"],
            "cover_url":    album_data["images"][0]["url"],
            "thumbnail":    album_data["images"][0]["url"],
            "url":          album_url,
            "tracks":       tracks_data["items"],
        }))
    }

    pub async fn get_playlist_tracks(&self, playlist_id: &str) -> MhResult<Value> {
        let id = playlist_id
            .strip_prefix("spotify:playlist:")
            .unwrap_or(playlist_id);

        let playlist_url = format!("{}/playlists/{}", API_URL, id);
        let tracks_url = format!("{}/playlists/{}/tracks", API_URL, id);

        let (playlist_data, tracks_data) = tokio::try_join!(
            self.get(&playlist_url, &[("market", "US")]),
            self.get(&tracks_url, &[("market", "US"), ("limit", "100")])
        )
        .map_err(|e| {
            if e.to_string().contains("Not Found") || e.to_string().contains("404") {
                MhError::NotFound(
                    "Spotify playlist not found. It may be private or unavailable in your region."
                        .to_string(),
                )
            } else {
                e
            }
        })?;

        let playlist_url = format!("https://open.spotify.com/playlist/{}", id);
        Ok(serde_json::json!({
            "playlist_name": playlist_data["name"],
            "owner_name":    playlist_data["owner"]["display_name"],
            "cover_url":     playlist_data["images"][0]["url"],
            "thumbnail":     playlist_data["images"][0]["url"],
            "url":           playlist_url,
            "tracks":        tracks_data["items"],
        }))
    }

    pub async fn get_artist_albums(&self, artist_id: &str) -> MhResult<Value> {
        let url = format!("{}/artists/{}/albums", API_URL, artist_id);
        self.get(
            &url,
            &[
                ("limit", "50"),
                ("include_groups", "album,single"),
                ("market", "US"),
            ],
        )
        .await
    }

    pub async fn get_show_episodes(&self, show_id: &str) -> MhResult<Value> {
        let id = show_id
            .strip_prefix("spotify:show:")
            .unwrap_or(show_id);

        let show_url = format!("{}/shows/{}", API_URL, id);
        let episodes_url = format!("{}/shows/{}/episodes", API_URL, id);

        let (show_data, episodes_data) = tokio::try_join!(
            self.get(&show_url, &[("market", "US")]),
            self.get(&episodes_url, &[("market", "US"), ("limit", "50")])
        )?;

        Ok(serde_json::json!({
            "show_name":  show_data["name"],
            "publisher":  show_data["publisher"],
            "cover_url":  show_data["images"][0]["url"],
            "episodes":   episodes_data["items"],
        }))
    }

    pub async fn get_audiobook_chapters(&self, audiobook_id: &str) -> MhResult<Value> {
        let id = audiobook_id
            .strip_prefix("spotify:audiobook:")
            .unwrap_or(audiobook_id);

        let book_url = format!("{}/audiobooks/{}", API_URL, id);
        let chapters_url = format!("{}/audiobooks/{}/chapters", API_URL, id);

        let (book_data, chapters_data) = tokio::try_join!(
            self.get(&book_url, &[("market", "US")]),
            self.get(&chapters_url, &[("market", "US"), ("limit", "50")])
        )?;

        Ok(serde_json::json!({
            "book_name": book_data["name"],
            "author":    book_data["authors"][0]["name"],
            "cover_url": book_data["images"][0]["url"],
            "chapters":  chapters_data["items"],
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

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_client;

const API_URL_V2: &str = "https://openapi.tidal.com/v2";
const API_URL_V1: &str = "https://api.tidal.com/v1";
const TOKEN_URL: &str = "https://auth.tidal.com/v1/oauth2/token";

#[derive(Debug, Clone)]
struct TokenInfo {
    access_token: String,
    expires_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TidalSearchType {
    Tracks,
    Albums,
    Artists,
    Playlists,
    Videos,
}

impl TidalSearchType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Tracks => "TRACKS",
            Self::Albums => "ALBUMS",
            Self::Artists => "ARTISTS",
            Self::Playlists => "PLAYLISTS",
            Self::Videos => "VIDEOS",
        }
    }

    fn include_param(self) -> &'static str {
        match self {
            Self::Tracks => "tracks,albums,artists",
            Self::Albums => "albums,artists",
            Self::Artists => "artists",
            Self::Playlists => "playlists,artists",
            Self::Videos => "videos,artists",
        }
    }

    fn v1_str(self) -> &'static str {
        self.as_str()
    }
}

#[derive(Clone)]
pub struct TidalApiClient {
    client_id: String,
    client_secret: String,
    token: Arc<RwLock<Option<TokenInfo>>>,
    http: Client,
}

impl TidalApiClient {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> MhResult<Self> {
        Ok(Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            token: Arc::new(RwLock::new(None)),
            http: build_client()?,
        })
    }

    pub async fn ensure_token(&self) -> MhResult<String> {
        if self.client_id.is_empty() || self.client_secret.is_empty() {
            return Err(MhError::Auth(
                "Tidal API credentials not configured. Add them in Settings → API Keys."
                    .to_string(),
            ));
        }
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
                "Tidal authentication failed: {}",
                resp.status()
            )));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        let access_token = data["access_token"]
            .as_str()
            .ok_or_else(|| MhError::Auth("Missing access_token in Tidal response".to_string()))?
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

    pub async fn search_v2(
        &self,
        query: &str,
        search_type: TidalSearchType,
        country_code: &str,
    ) -> MhResult<Value> {
        let token = self.ensure_token().await?;
        let include = search_type.include_param();

        let encoded_query = urlencoding_encode(query);
        let url = format!(
            "{}/searchResults/{}?countryCode={}&include={}",
            API_URL_V2, encoded_query, country_code, include
        );

        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.api+json")
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        Ok(self.transform_search_response(&data))
    }

    fn transform_search_response(&self, api_response: &Value) -> Value {
        let empty_vec = vec![];
        let results = api_response["included"]
            .as_array()
            .unwrap_or(&empty_vec);

        let mut included_map: HashMap<&str, HashMap<String, &Value>> = HashMap::new();
        for item in results {
            if let (Some(item_type), Some(item_id)) =
                (item["type"].as_str(), item["id"].as_str())
            {
                included_map
                    .entry(item_type)
                    .or_default()
                    .insert(item_id.to_string(), item);
            }
        }

        let mut grouped = serde_json::json!({
            "tracks": [],
            "albums": [],
            "artists": [],
            "playlists": [],
            "videos": [],
        });

        for item in results {
            let item_type = match item["type"].as_str() {
                Some(t) => t,
                None => continue,
            };

            let mut enriched = item.clone();

            match item_type {
                "tracks" => {
                    if let Some(album_id) = item["relationships"]["albums"]["data"][0]["id"].as_str() {
                        if let Some(album) = included_map.get("albums").and_then(|m| m.get(album_id)) {
                            enriched["album"] = album["attributes"].clone();
                        }
                    }
                    if let Some(artists_data) = item["relationships"]["artists"]["data"].as_array() {
                        let artists: Vec<Value> = artists_data
                            .iter()
                            .filter_map(|r| r["id"].as_str())
                            .filter_map(|aid| {
                                included_map
                                    .get("artists")
                                    .and_then(|m| m.get(aid))
                                    .map(|a| a["attributes"].clone())
                            })
                            .collect();
                        enriched["artists"] = Value::Array(artists);
                    }
                    grouped["tracks"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!({ "resource": enriched }));
                }
                "albums" => {
                    if let Some(artist_id) =
                        item["relationships"]["artists"]["data"][0]["id"].as_str()
                    {
                        if let Some(artist) =
                            included_map.get("artists").and_then(|m| m.get(artist_id))
                        {
                            enriched["artist"] = artist["attributes"].clone();
                        }
                    }
                    grouped["albums"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!({ "resource": enriched }));
                }
                "artists" => {
                    grouped["artists"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!({ "resource": enriched }));
                }
                "playlists" => {
                    if let Some(artists_data) = item["relationships"]["artists"]["data"].as_array() {
                        let artists: Vec<Value> = artists_data
                            .iter()
                            .filter_map(|r| r["id"].as_str())
                            .filter_map(|aid| {
                                included_map
                                    .get("artists")
                                    .and_then(|m| m.get(aid))
                                    .map(|a| a["attributes"].clone())
                            })
                            .collect();
                        enriched["artists"] = Value::Array(artists);
                    }
                    grouped["playlists"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!({ "resource": enriched }));
                }
                "videos" => {
                    if let Some(artists_data) = item["relationships"]["artists"]["data"].as_array() {
                        let artists: Vec<Value> = artists_data
                            .iter()
                            .filter_map(|r| r["id"].as_str())
                            .filter_map(|aid| {
                                included_map
                                    .get("artists")
                                    .and_then(|m| m.get(aid))
                                    .map(|a| a["attributes"].clone())
                            })
                            .collect();
                        enriched["artists"] = Value::Array(artists);
                    }
                    grouped["videos"]
                        .as_array_mut()
                        .unwrap()
                        .push(serde_json::json!({ "resource": enriched }));
                }
                _ => {}
            }
        }

        grouped
    }

    pub async fn search_v1(
        &self,
        query: &str,
        search_type: TidalSearchType,
        country_code: &str,
        user_token: &str,
    ) -> MhResult<Value> {
        let url = format!(
            "{}/search?query={}&types={}&countryCode={}&limit=30",
            API_URL_V1,
            urlencoding_encode(query),
            search_type.v1_str(),
            country_code
        );

        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", user_token))
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Other(format!(
                "Tidal v1 search failed: {}",
                resp.status()
            )));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;

        fn wrap_items(data: &Value, key: &str) -> Vec<Value> {
            data[key]["items"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|item| serde_json::json!({ "resource": item }))
                .collect()
        }

        Ok(serde_json::json!({
            "tracks":    wrap_items(&data, "tracks"),
            "albums":    wrap_items(&data, "albums"),
            "artists":   wrap_items(&data, "artists"),
            "playlists": wrap_items(&data, "playlists"),
            "videos":    wrap_items(&data, "videos"),
        }))
    }

    async fn get_v2(&self, path: &str, params: &[(&str, &str)]) -> MhResult<Value> {
        let token = self.ensure_token().await?;
        let url = format!("{}/{}", API_URL_V2, path);
        let mut req = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.tidal.v1+json");

        if !params.is_empty() {
            req = req.query(params);
        }

        let resp = req.send().await.map_err(MhError::Network)?;
        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }
        Ok(resp.json().await.map_err(MhError::Network)?)
    }

    pub async fn get_track(&self, track_id: &str, country_code: &str) -> MhResult<Value> {
        self.get_v2(
            &format!("tracks/{}", track_id),
            &[("countryCode", country_code), ("include", "artists,albums")],
        )
        .await
    }

    pub async fn get_album(&self, album_id: &str, country_code: &str) -> MhResult<Value> {
        self.get_v2(
            &format!("albums/{}", album_id),
            &[("countryCode", country_code), ("include", "items")],
        )
        .await
    }

    pub async fn get_artist_albums(
        &self,
        artist_id: &str,
        country_code: &str,
    ) -> MhResult<Value> {
        self.get_v2(
            &format!("artists/{}/relationships/albums", artist_id),
            &[("countryCode", country_code)],
        )
        .await
    }

    pub async fn get_playlist(&self, playlist_id: &str, country_code: &str) -> MhResult<Value> {
        self.get_v2(
            &format!("playlists/{}/relationships/items", playlist_id),
            &[("countryCode", country_code)],
        )
        .await
    }

    pub async fn get_stream_url(
        &self,
        track_id: &str,
        country_code: &str,
        user_token: Option<&str>,
    ) -> MhResult<Value> {
        let token = match user_token {
            Some(t) => t.to_string(),
            None => self.ensure_token().await?,
        };

        let url = format!(
            "{}/tracks/{}/streamUrl?countryCode={}",
            API_URL_V1, track_id, country_code
        );

        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }
        Ok(resp.json().await.map_err(MhError::Network)?)
    }
}

fn urlencoding_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

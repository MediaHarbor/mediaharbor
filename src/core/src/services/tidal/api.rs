use crate::services::common::library::str_at;
use std::collections::HashMap;
use std::sync::Arc;

use reqwest::Client;
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_client;
use crate::services::common::http::read_json;
use crate::services::common::oauth::TokenCache;

const API_URL_V2: &str = "https://openapi.tidal.com/v2";
const API_URL_V1: &str = "https://api.tidal.com/v1";
const TOKEN_URL: &str = "https://auth.tidal.com/v1/oauth2/token";

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
    token: Arc<TokenCache>,
    http: Client,
}

/// `included` keyed by type then id — every relationship in a Tidal search response is
/// resolved through it rather than being inlined on the item.
type IncludedMap<'a> = HashMap<&'a str, HashMap<String, &'a Value>>;

/// Copy the attributes of every related artist onto `enriched` as an `artists` array.
fn attach_artists(enriched: &mut Value, item: &Value, included: &IncludedMap<'_>) {
    let Some(rows) = item["relationships"]["artists"]["data"].as_array() else {
        return;
    };
    let artists: Vec<Value> = rows
        .iter()
        .filter_map(|r| r["id"].as_str())
        .filter_map(|id| {
            included
                .get("artists")
                .and_then(|m| m.get(id))
                .map(|a| a["attributes"].clone())
        })
        .collect();
    enriched["artists"] = Value::Array(artists);
}

/// Copy the attributes of the first related `rel` onto `enriched` under `dest`.
fn attach_first(
    enriched: &mut Value,
    item: &Value,
    included: &IncludedMap<'_>,
    rel: &str,
    dest: &str,
) {
    if let Some(id) = item["relationships"][rel]["data"][0]["id"].as_str() {
        if let Some(v) = included.get(rel).and_then(|m| m.get(id)) {
            enriched[dest] = v["attributes"].clone();
        }
    }
}

impl TidalApiClient {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> MhResult<Self> {
        Ok(Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            token: Arc::new(TokenCache::default()),
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
        self.token
            .client_credentials(
                &self.http,
                TOKEN_URL,
                "Tidal",
                &self.client_id,
                &self.client_secret,
            )
            .await
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

        let data: Value = read_json("Tidal", resp).await?;
        Ok(self.transform_search_response(&data))
    }

    pub async fn suggestions(&self, query: &str, country_code: &str) -> MhResult<Vec<String>> {
        let cc = if country_code.is_empty() {
            "US"
        } else {
            country_code
        };
        let data = self.search_v2(query, TidalSearchType::Tracks, cc).await?;
        Ok(crate::services::common::search::suggestion_lines(
            data["tracks"].as_array().map(|v| &v[..]).unwrap_or(&[]),
            |item| {
                let r = &item["resource"];
                (
                    r.pointer("/attributes/title")
                        .or_else(|| r.get("title"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    str_at(r, &["/artists/0/name"]).unwrap_or(""),
                )
            },
        ))
    }

    fn transform_search_response(&self, api_response: &Value) -> Value {
        let empty_vec = vec![];
        let results = api_response["included"].as_array().unwrap_or(&empty_vec);

        let mut included_map: IncludedMap<'_> = HashMap::new();
        for item in results {
            if let (Some(item_type), Some(item_id)) = (item["type"].as_str(), item["id"].as_str()) {
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
                    attach_first(&mut enriched, item, &included_map, "albums", "album");
                    attach_artists(&mut enriched, item, &included_map);
                }
                "albums" => attach_first(&mut enriched, item, &included_map, "artists", "artist"),
                "artists" => {}
                "playlists" | "videos" => attach_artists(&mut enriched, item, &included_map),
                _ => continue,
            }

            if let Some(bucket) = grouped[item_type].as_array_mut() {
                bucket.push(serde_json::json!({ "resource": enriched }));
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
            "{}/search?query={}&types={}&countryCode={}&limit={}",
            API_URL_V1,
            urlencoding_encode(query),
            search_type.v1_str(),
            country_code,
            crate::services::common::limits::TIDAL_PAGE_MAX
        );

        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", user_token))
            .send()
            .await
            .map_err(MhError::Network)?;

        let data: Value = crate::services::common::http::read_json("Tidal", resp).await?;

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
        read_json("Tidal", resp).await
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

        read_json("Tidal", resp).await
    }
}

fn urlencoding_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

use reqwest::Client;
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::{build_mozilla_client, UA_MOZILLA};

const ITUNES_SEARCH: &str = "https://itunes.apple.com/search";
const ITUNES_LOOKUP: &str = "https://itunes.apple.com/lookup";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppleMusicMediaType {
    Song,
    Album,
    Playlist,
    Artist,
    MusicVideo,
}

impl AppleMusicMediaType {
    fn entity(self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Album => "album",
            Self::Artist => "musicArtist",
            Self::Playlist => "musicPlaylist",
            Self::MusicVideo => "musicVideo",
        }
    }
}

#[derive(Clone)]
pub struct AppleMusicApiClient {
    pub developer_token: Option<String>,
    http: Client,
}

impl AppleMusicApiClient {
    pub fn new(developer_token: Option<String>) -> MhResult<Self> {
        Ok(Self {
            developer_token,
            http: build_mozilla_client()?,
        })
    }

    pub fn unauthenticated() -> MhResult<Self> {
        Self::new(None)
    }

    pub async fn search(
        &self,
        query: &str,
        media_type: AppleMusicMediaType,
        limit: u32,
    ) -> MhResult<Value> {
        let limit_str = limit.to_string();
        let resp = self
            .http
            .get(ITUNES_SEARCH)
            .query(&[
                ("term", query),
                ("media", "music"),
                ("entity", media_type.entity()),
                ("limit", limit_str.as_str()),
            ])
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        let mut results = data["results"].clone();

        if media_type == AppleMusicMediaType::Artist {
            if let Some(arr) = results.as_array_mut() {
                let enriched: Vec<Value> = futures::future::join_all(arr.iter().map(|artist| {
                    let client = self.http.clone();
                    let artist = artist.clone();
                    async move {
                        let link = match artist["artistLinkUrl"].as_str() {
                            Some(l) => l.to_string(),
                            None => return artist,
                        };
                        match fetch_og_image(&client, &link).await {
                            Some(url) => {
                                let mut enriched = artist;
                                enriched["artworkUrl100"] = Value::String(url);
                                enriched
                            }
                            None => artist,
                        }
                    }
                }))
                .await;
                return Ok(Value::Array(enriched));
            }
        }

        Ok(results)
    }

    pub async fn get_artist_albums(&self, artist_id: &str, _storefront: &str) -> MhResult<Value> {
        let resp = self
            .http
            .get(ITUNES_LOOKUP)
            .query(&[
                ("id", artist_id),
                ("entity", "album"),
                ("limit", "50"),
            ])
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        let albums: Vec<Value> = data["results"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter(|item| item["wrapperType"].as_str() == Some("collection"))
            .cloned()
            .collect();
        Ok(Value::Array(albums))
    }

    pub async fn lookup_by_id(&self, id: &str) -> MhResult<Value> {
        let resp = self
            .http
            .get(ITUNES_LOOKUP)
            .query(&[("id", id)])
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        Ok(data["results"]
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null))
    }

    pub async fn get_album_tracks(&self, album_id: &str, _storefront: &str) -> MhResult<Value> {
        let resp = self
            .http
            .get(ITUNES_LOOKUP)
            .query(&[("id", album_id), ("entity", "song")])
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        let results = data["results"].as_array().unwrap_or(&vec![]).clone();

        let album = results
            .iter()
            .find(|item| item["wrapperType"].as_str() == Some("collection"))
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        let tracks: Vec<Value> = results
            .iter()
            .filter(|item| {
                item["wrapperType"].as_str() == Some("track")
                    && item["kind"].as_str() == Some("song")
            })
            .cloned()
            .collect();

        let collection_url = album["collectionViewUrl"].as_str().unwrap_or("").to_string();
        let thumbnail = album["artworkUrl100"]
            .as_str()
            .unwrap_or("")
            .replace("100x100", "640x640");
        Ok(serde_json::json!({
            "album":     album,
            "tracks":    tracks,
            "url":       collection_url,
            "thumbnail": thumbnail,
        }))
    }
}

async fn fetch_og_image(client: &Client, url: &str) -> Option<String> {
    let resp = client
        .get(url)
        .header("User-Agent", UA_MOZILLA)
        .send()
        .await
        .ok()?;
    let html = resp.text().await.ok()?;
    let re = regex::Regex::new(r#"<meta\s+property="og:image"\s+content="([^"]+)""#).ok()?;
    re.captures(&html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

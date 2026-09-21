use reqwest::Client;
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::{build_mozilla_client, UA_MOZILLA};
use crate::services::common::http::read_json;

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

        let data: Value = read_json("Apple Music", resp).await?;
        let mut results = data["results"].clone();

        if media_type == AppleMusicMediaType::Artist {
            if let Some(arr) = results.as_array_mut() {
                let enriched: Vec<Value> =
                    futures_util::future::join_all(arr.iter().map(|artist| {
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

    pub async fn lookup_by_id(&self, id: &str) -> MhResult<Value> {
        self.lookup_by_id_in(id, "").await
    }

    /// `storefront` picks the store to answer from. Without it iTunes answers from
    /// the US store, so a release that is not carried there comes back empty even
    /// when the link named a storefront that does carry it.
    pub async fn lookup_by_id_in(&self, id: &str, storefront: &str) -> MhResult<Value> {
        let mut query: Vec<(&str, &str)> = vec![("id", id)];
        if !storefront.is_empty() {
            query.push(("country", storefront));
        }
        let resp = self
            .http
            .get(ITUNES_LOOKUP)
            .query(&query)
            .send()
            .await
            .map_err(MhError::Network)?;

        let data: Value = read_json("Apple Music", resp).await?;
        Ok(data["results"]
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null))
    }

    pub async fn get_album_tracks(&self, album_id: &str, storefront: &str) -> MhResult<Value> {
        let mut query: Vec<(&str, &str)> = vec![("id", album_id), ("entity", "song")];
        if !storefront.is_empty() {
            query.push(("country", storefront));
        }
        let resp = self
            .http
            .get(ITUNES_LOOKUP)
            .query(&query)
            .send()
            .await
            .map_err(MhError::Network)?;

        let data: Value = read_json("Apple Music", resp).await?;
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

        let collection_url = album["collectionViewUrl"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let thumbnail = album["artworkUrl100"]
            .as_str()
            .unwrap_or("")
            .replace("100x100", "640x640");
        let total_duration_ms: u64 = tracks
            .iter()
            .filter_map(|t| t["trackTimeMillis"].as_u64())
            .sum();
        let total_duration_secs = total_duration_ms / 1000;
        Ok(serde_json::json!({
            "album": {
                "id":          album["collectionId"],
                "title":       album["collectionName"],
                "artist":      album["artistName"],
                "artistId":    album["artistId"],
                "coverUrl":    thumbnail,
                "releaseDate": album["releaseDate"],
                "trackCount":  album["trackCount"],
                "duration":    total_duration_secs,
                "copyright":   album["copyright"],
                "genre":       album["primaryGenreName"],
                "explicit":    album["collectionExplicitness"] == "explicit",
                "discCount":   album["discCount"],
                "label":       album["copyright"],
            },
            "album_raw":  album,
            "tracks":     tracks,
            "url":        collection_url,
            "thumbnail":  thumbnail,

            "copyright":      album["copyright"],
            "total_duration": total_duration_secs,
            "release_date":   album["releaseDate"],
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

use reqwest::Client;
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_client;

const API_BASE: &str = "https://api.deezer.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeezerSearchType {
    Track,
    Album,
    Artist,
    Playlist,
    Radio,
    Podcast,
    Episode,
}

impl DeezerSearchType {
    fn path(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Album => "album",
            Self::Artist => "artist",
            Self::Playlist => "playlist",
            Self::Radio => "radio",
            Self::Podcast => "podcast",
            Self::Episode => "episode",
        }
    }
}

#[derive(Clone)]
pub struct DeezerApiClient {
    pub client: Client,
}

impl DeezerApiClient {
    pub fn new() -> MhResult<Self> {
        Ok(Self {
            client: build_client()?,
        })
    }

    pub async fn search(
        &self,
        query: &str,
        search_type: DeezerSearchType,
        limit: u32,
    ) -> MhResult<Value> {
        let url = format!("{}/search/{}", API_BASE, search_type.path());
        let limit_str = limit.to_string();
        let resp = self
            .client
            .get(&url)
            .query(&[("q", query), ("limit", limit_str.as_str())])
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        Ok(if data.is_array() {
            data
        } else {
            data["data"].clone()
        })
    }

    pub async fn get_track(&self, track_id: &str) -> MhResult<Value> {
        let url = format!("{}/track/{}", API_BASE, track_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(MhError::Network)?;
        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }
        Ok(resp.json().await.map_err(MhError::Network)?)
    }

    pub async fn get_track_list(&self, id: &str, list_type: &str) -> MhResult<Value> {
        if !matches!(list_type, "album" | "playlist") {
            return Err(MhError::Unsupported(format!(
                "Invalid Deezer list type: {}. Must be 'album' or 'playlist'.",
                list_type
            )));
        }

        let details_url = format!("{}/{}/{}", API_BASE, list_type, id);
        let tracks_url = format!("{}/{}/{}/tracks", API_BASE, list_type, id);

        let details_resp = self
            .client
            .get(&details_url)
            .send()
            .await
            .map_err(MhError::Network)?;
        if !details_resp.status().is_success() {
            return Err(MhError::Other(format!(
                "Failed to fetch {} details for id {}",
                list_type, id
            )));
        }
        let item_details: Value = details_resp.json().await.map_err(MhError::Network)?;

        let tracks_resp = self
            .client
            .get(&tracks_url)
            .send()
            .await
            .map_err(MhError::Network)?;
        if !tracks_resp.status().is_success() {
            return Err(MhError::Other(format!(
                "Failed to fetch tracks for {} id {}",
                list_type, id
            )));
        }
        let track_data: Value = tracks_resp.json().await.map_err(MhError::Network)?;
        let track_list = track_data["data"].clone();

        let artist_name = if list_type == "album" {
            item_details["artist"]["name"]
                .as_str()
                .unwrap_or("Unknown Artist")
                .to_string()
        } else {
            "N/A".to_string()
        };

        Ok(serde_json::json!({
            "type":         list_type,
            "id":           id,
            "name":         item_details["title"].as_str().unwrap_or("Unknown Title"),
            "artist":       artist_name,
            "release_date": item_details["release_date"].as_str().unwrap_or("Unknown Date"),
            "total_tracks": track_list.as_array().map(|a| a.len()).unwrap_or(0),
            "cover_xl":     item_details["cover_xl"].as_str().unwrap_or(""),
            "thumbnail":    item_details["cover_xl"].as_str().or_else(|| item_details["cover_big"].as_str()).unwrap_or(""),
            "md5_image":    item_details["md5_image"].as_str().unwrap_or(""),
            "link":         format!("https://www.deezer.com/{}/{}", list_type, id),
            "url":          format!("https://www.deezer.com/{}/{}", list_type, id),
            "tracks":       track_list,
        }))
    }

    pub async fn get_artist_albums(&self, artist_id: &str) -> MhResult<Value> {
        let url = format!("{}/artist/{}/albums?limit=50", API_BASE, artist_id);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(MhError::Network)?;
        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }
        let data: Value = resp.json().await.map_err(MhError::Network)?;
        Ok(if data.is_array() {
            data
        } else {
            data["data"].clone()
        })
    }
}

impl Default for DeezerApiClient {
    fn default() -> Self {
        Self::new().expect("failed to build reqwest client")
    }
}

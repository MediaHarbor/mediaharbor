use crate::services::common::library::str_at;
use reqwest::Client;
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_client;
use crate::services::common::http::read_json;

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
        offset: u32,
    ) -> MhResult<Value> {
        let url = format!("{}/search/{}", API_BASE, search_type.path());
        let limit_str = limit.to_string();
        let index_str = offset.to_string();
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("q", query),
                ("limit", limit_str.as_str()),
                ("index", index_str.as_str()),
            ])
            .send()
            .await
            .map_err(MhError::Network)?;

        let data: Value = read_json("Deezer", resp).await?;
        Ok(if data.is_array() {
            data
        } else {
            data["data"].clone()
        })
    }

    pub async fn suggestions(&self, query: &str) -> MhResult<Vec<String>> {
        let data = self.search(query, DeezerSearchType::Track, 10, 0).await?;
        Ok(crate::services::common::search::suggestion_lines(
            data.as_array().map(|v| &v[..]).unwrap_or(&[]),
            |item| {
                (
                    str_at(item, &["title"]).unwrap_or(""),
                    str_at(item, &["/artist/name"]).unwrap_or(""),
                )
            },
        ))
    }

    pub async fn get_track(&self, track_id: &str) -> MhResult<Value> {
        self.get_public(&format!("track/{}", track_id)).await
    }

    pub async fn get_album(&self, album_id: &str) -> MhResult<Value> {
        self.get_public(&format!("album/{}", album_id)).await
    }

    pub async fn get_playlist(&self, playlist_id: &str) -> MhResult<Value> {
        self.get_public(&format!("playlist/{}", playlist_id)).await
    }

    async fn get_public(&self, path: &str) -> MhResult<Value> {
        let url = format!("{}/{}", API_BASE, path);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(MhError::Network)?;
        read_json("Deezer", resp).await
    }
}

impl Default for DeezerApiClient {
    fn default() -> Self {
        Self::new().expect("failed to build reqwest client")
    }
}

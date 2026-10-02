use crate::services::common::library::str_at;
use reqwest::Client;
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_client;
use crate::services::common::http::read_json;
use crate::services::common::ids::now_secs;

const API_BASE: &str = "https://www.qobuz.com/api.json/0.2";

const BUNDLED_APP_ID: &str = "312369995";
const BUNDLED_APP_SECRET: &str = "";
const BUNDLED_AUTH_TOKEN: &str = "";

#[derive(Clone)]
pub struct QobuzApiClient {
    pub app_id: String,
    pub token: String,
    pub app_secret: String,
    http: Client,
}

impl QobuzApiClient {
    pub fn new(
        app_id: impl Into<String>,
        token: impl Into<String>,
        app_secret: impl Into<String>,
    ) -> MhResult<Self> {
        Ok(Self {
            app_id: app_id.into(),
            token: token.into(),
            app_secret: app_secret.into(),
            http: build_client()?,
        })
    }

    pub fn with_bundled_credentials() -> MhResult<Self> {
        Self::new(BUNDLED_APP_ID, BUNDLED_AUTH_TOKEN, BUNDLED_APP_SECRET)
    }

    async fn get(&self, path: &str, params: &[(&str, &str)]) -> MhResult<Value> {
        let url = format!("{}/{}", API_BASE, path);
        let resp = self
            .http
            .get(&url)
            .header("X-User-Auth-Token", &self.token)
            .query(params)
            .send()
            .await
            .map_err(MhError::Network)?;

        read_json("Qobuz", resp).await
    }

    pub async fn search(
        &self,
        query: &str,
        search_type: &str,
        limit: u32,
        offset: u32,
    ) -> MhResult<Value> {
        let limit_str = limit.to_string();
        let offset_str = offset.to_string();
        self.get(
            &format!("{}/search", search_type),
            &[
                ("app_id", self.app_id.as_str()),
                ("query", query),
                ("limit", limit_str.as_str()),
                ("offset", offset_str.as_str()),
            ],
        )
        .await
    }

    pub async fn suggestions(&self, query: &str) -> MhResult<Vec<String>> {
        let data = self.search(query, "track", 10, 0).await?;
        Ok(crate::services::common::search::suggestion_lines(
            data["tracks"]["items"]
                .as_array()
                .map(|v| &v[..])
                .unwrap_or(&[]),
            |item| {
                (
                    str_at(item, &["title"]).unwrap_or(""),
                    item.pointer("/performer/name")
                        .or_else(|| item.pointer("/album/artist/name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                )
            },
        ))
    }

    pub async fn get_track(&self, track_id: &str) -> MhResult<Value> {
        self.get(
            "track/get",
            &[("app_id", self.app_id.as_str()), ("track_id", track_id)],
        )
        .await
    }

    pub async fn get_stream_url(&self, track_id: &str, format_id: u8) -> MhResult<String> {
        let format_str = format_id.to_string();
        let unix_ts = now_secs().to_string();
        let request_sig = crate::services::qobuz::app_credentials::file_url_signature(
            track_id,
            &format_str,
            &unix_ts,
            &self.app_secret,
        );

        let data = self
            .get(
                "track/getFileUrl",
                &[
                    ("app_id", self.app_id.as_str()),
                    ("track_id", track_id),
                    ("format_id", format_str.as_str()),
                    ("intent", "stream"),
                    ("request_ts", unix_ts.as_str()),
                    ("request_sig", request_sig.as_str()),
                ],
            )
            .await?;

        data["url"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| MhError::Parse("Missing url in Qobuz stream response".to_string()))
    }

    pub async fn get_album(&self, album_id: &str) -> MhResult<Value> {
        self.get(
            "album/get",
            &[("app_id", self.app_id.as_str()), ("album_id", album_id)],
        )
        .await
    }

    pub async fn get_playlist(&self, playlist_id: &str) -> MhResult<Value> {
        self.get(
            "playlist/get",
            &[
                ("app_id", self.app_id.as_str()),
                ("playlist_id", playlist_id),
                ("extra", "tracks"),
            ],
        )
        .await
    }
}

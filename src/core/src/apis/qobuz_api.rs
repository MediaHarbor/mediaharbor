use reqwest::Client;
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_client;

const API_BASE: &str = "https://www.qobuz.com/api.json/0.2";

const BUNDLED_APP_ID: &str = "950096963";
const BUNDLED_APP_SECRET: &str = "10b251c286cfbf64d6b7105f253d9a2e";
const BUNDLED_AUTH_TOKEN: &str =
    "u6lHtzb1Vv_TbNYYL_PrIzVZfkMpxUJ4Y4AkpdrfFRaj5o1sbLP7ENCKVD-wQEmkMbQIN-G6vcgzPvwaZdEvPA";

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

        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }
        Ok(resp.json().await.map_err(MhError::Network)?)
    }

    pub async fn search(
        &self,
        query: &str,
        search_type: &str,
        limit: u32,
    ) -> MhResult<Value> {
        let limit_str = limit.to_string();
        self.get(
            &format!("{}/search", search_type),
            &[
                ("app_id", self.app_id.as_str()),
                ("query", query),
                ("limit", limit_str.as_str()),
            ],
        )
        .await
    }

    pub async fn get_track(&self, track_id: &str) -> MhResult<Value> {
        self.get(
            "track/get",
            &[("app_id", self.app_id.as_str()), ("track_id", track_id)],
        )
        .await
    }

    pub async fn get_stream_url(&self, track_id: &str, format_id: u8) -> MhResult<String> {
        use md5::{Digest, Md5};
        use std::time::{SystemTime, UNIX_EPOCH};

        let format_str = format_id.to_string();
        let unix_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();

        let sig_str = format!(
            "trackgetFileUrlformat_id{}intentstreamtrack_id{}{}{}",
            format_str, track_id, unix_ts, self.app_secret
        );
        let request_sig = format!("{:x}", Md5::digest(sig_str.as_bytes()));

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

    pub async fn get_artist_albums(&self, artist_id: &str) -> MhResult<Value> {
        let data = self
            .get(
                "artist/get",
                &[
                    ("app_id", self.app_id.as_str()),
                    ("artist_id", artist_id),
                    ("extra", "albums"),
                ],
            )
            .await?;

        Ok(data["albums"].clone())
    }

    pub async fn get_track_list(&self, entity_id: &str, entity_type: &str) -> MhResult<Value> {
        match entity_type {
            "album" => self.get_album(entity_id).await,
            "playlist" => self.get_playlist(entity_id).await,
            "artist" => {
                self.get(
                    "artist/get",
                    &[
                        ("app_id", self.app_id.as_str()),
                        ("artist_id", entity_id),
                        ("extra", "albums"),
                    ],
                )
                .await
            }
            other => Err(MhError::Unsupported(format!(
                "Unknown Qobuz entity type: {}",
                other
            ))),
        }
    }
}

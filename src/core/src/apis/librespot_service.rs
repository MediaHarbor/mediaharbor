
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use bytes::Bytes;
use hmac::Mac;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client, ClientBuilder,
};
use serde_json::{json, Value};
use sha1::Sha1;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::{debug, warn};

use crate::errors::{MhError, MhResult};
use crate::http_client::UA_CHROME_LATEST;

const TOTP_PERIOD: u64 = 30;
const TOTP_DIGITS: u32 = 6;

const TOTP_SECRETS_URL: &str =
    "https://git.gay/thereallo/totp-secrets/raw/branch/main/secrets/secretDict.json";
const SERVER_TIME_URL: &str = "https://open.spotify.com/api/server-time";
const SESSION_TOKEN_URL: &str = "https://open.spotify.com/api/token";
const CLIENT_TOKEN_URL: &str = "https://clienttoken.spotify.com/v1/clienttoken";
const PLAYBACK_INFO_URL: &str =
    "https://gue1-spclient.spotify.com/track-playback/v1/media/spotify:{mediaType}:{mediaId}";
const STORAGE_RESOLVE_URL: &str =
    "https://gue1-spclient.spotify.com/storage-resolve/v2/files/audio/interactive/11/{fileId}?version=10000000&product=9&platform=39&alt=json";
const PLAYPLAY_LICENSE_URL: &str =
    "https://gew4-spclient.spotify.com/playplay/v1/key/{fileId}";

const CLIENT_VERSION: &str = "1.2.70.61.g856ccd63";
const UA_ANDROID: &str = "Spotify/8.9.86.551 Android/34 (Google Pixel 8)";

const AES_IV: [u8; 16] = [
    0x72, 0xe0, 0x67, 0xfb, 0xdd, 0xcb, 0xcf, 0x77,
    0xeb, 0xe8, 0xbc, 0x64, 0x3f, 0x63, 0x0d, 0x93,
];

pub struct LibrespotService {
    client: Client,
    access_token: Option<String>,
    token_expiry: u64,
    user_profile: Option<Value>,
    logged_in: bool,
    sp_dc: Option<String>,
    totp_secret: Option<Vec<u8>>,
    totp_version: Option<u32>,
    client_token: Option<String>,
    client_id: Option<String>,
    pub wvd_path: Option<String>,
}

impl LibrespotService {
    pub fn new() -> Self {
        let client = ClientBuilder::new()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::limited(10))
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(15))
            .gzip(true)
            .build()
            .expect("failed to build reqwest client for LibrespotService");

        Self {
            client,
            access_token: None,
            token_expiry: 0,
            user_profile: None,
            logged_in: false,
            sp_dc: None,
            totp_secret: None,
            totp_version: None,
            client_token: None,
            client_id: None,
            wvd_path: None,
        }
    }

    pub async fn login_from_cookies(&mut self, cookies_path: &Path) -> MhResult<Value> {
        if !cookies_path.exists() {
            return Err(MhError::Auth(format!(
                "Cookies file not found: {}",
                cookies_path.display()
            )));
        }

        let content = tokio::fs::read_to_string(cookies_path).await?;
        let sp_dc = Self::_extract_sp_dc(&content).ok_or_else(|| {
            MhError::Auth(
                "sp_dc cookie not found in cookies file. Make sure you exported cookies from open.spotify.com".into(),
            )
        })?;

        self.sp_dc = Some(sp_dc);
        self._init_totp().await?;
        self._refresh_token().await?;

        self.user_profile = match self._fetch_profile().await {
            Ok(profile) => Some(profile),
            Err(e) => {
                warn!("Failed to fetch Spotify profile: {e}");
                Some(json!({ "name": "Spotify User" }))
            }
        };

        self.logged_in = true;
        Ok(self.user_profile.clone().unwrap_or(json!({})))
    }

    pub fn is_logged_in(&self) -> bool {
        self.logged_in && self.access_token.is_some()
    }

    pub fn cached_profile(&self) -> Option<Value> {
        self.user_profile.clone()
    }

    pub fn cached_access_token(&self) -> Option<String> {
        self.access_token.clone()
    }

    pub fn logout(&mut self) {
        self.access_token = None;
        self.token_expiry = 0;
        self.user_profile = None;
        self.logged_in = false;
        self.sp_dc = None;
    }

    pub async fn search(
        &mut self,
        query: &str,
        search_type: &str,
        limit: u32,
    ) -> MhResult<Value> {
        if !self.is_logged_in() {
            return Err(MhError::Auth("Spotify account not connected".into()));
        }

        let sp_type = match search_type {
            "track" => "track",
            "album" => "album",
            "artist" => "artist",
            "playlist" => "playlist",
            "episode" => "episode",
            "podcast" | "show" => "podcast",
            "audiobook" => "audiobook",
            other => other,
        };

        let token = self._get_valid_token().await?;
        let encoded_query = pct_encode(query);
        let url = format!(
            "https://spclient.wg.spotify.com/searchview/km/v4/search/{encoded_query}?limit={limit}&entityVersion=2&catalogue=&platform=web&locale=en&types={sp_type}"
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| MhError::Other(e.to_string()))?,
        );
        headers.insert(
            HeaderName::from_static("accept"),
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            HeaderName::from_static("app-platform"),
            HeaderValue::from_static("WebPlayer"),
        );

        let resp = self
            .client
            .get(&url)
            .headers(headers)
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Other(format!(
                "Spotify search error: {}",
                resp.status()
            )));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        let results = data["results"].clone();
        Ok(self._transform_search_results(&results, sp_type))
    }

    pub async fn get_track_info(&mut self, track_id: &str) -> MhResult<Value> {
        if !self.is_logged_in() {
            return Err(MhError::Auth("Spotify account not connected".into()));
        }

        let token = self._get_valid_token().await?;
        let vars = serde_json::to_string(&json!({ "uri": format!("spotify:track:{track_id}") }))
            .map_err(|e| MhError::Parse(e.to_string()))?;
        let ext = serde_json::to_string(&json!({
            "persistedQuery": {
                "version": 1,
                "sha256Hash": "ae85b52abb74d20a4c331d4143d4772c95f34757bfa8c625474b912b9055b5c0"
            }
        }))
        .map_err(|e| MhError::Parse(e.to_string()))?;

        let url = format!(
            "https://api-partner.spotify.com/pathfinder/v1/query?operationName=getTrack&variables={}&extensions={}",
            pct_encode(&vars),
            pct_encode(&ext)
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .header("app-platform", "WebPlayer")
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Other(format!(
                "Spotify track info error: {}",
                resp.status()
            )));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        let t = data["data"]["trackUnion"].clone();
        if t.is_null() {
            return Err(MhError::NotFound(format!("Track {track_id} not found")));
        }

        let fallback_url = format!("https://open.spotify.com/track/{track_id}");
        let share_url = t["sharingInfo"]["shareUrl"]
            .as_str()
            .unwrap_or(&fallback_url)
            .to_string();
        let explicit = t["contentRating"]["label"].as_str() == Some("EXPLICIT");

        let artists: Vec<Value> = t["firstArtist"]["items"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|a| json!({ "name": a["profile"]["name"] }))
                    .collect()
            })
            .unwrap_or_default();

        Ok(json!({
            "id": t["id"],
            "name": t["name"],
            "uri": t["uri"],
            "duration_ms": t["duration"]["totalMilliseconds"],
            "explicit": explicit,
            "external_urls": { "spotify": share_url },
            "album": { "name": t["albumOfTrack"]["name"] },
            "artists": artists,
            "preview_url": null,
        }))
    }

    pub async fn get_track_stream(
        &mut self,
        track_id: &str,
        venv_python: Option<&Path>,
    ) -> MhResult<(Bytes, String)> {
        if !self.is_logged_in() {
            return Err(MhError::Auth("Spotify account not connected".into()));
        }
        self._stream_media(track_id, "track", venv_python).await
    }

    pub async fn get_podcast_episode_stream(
        &mut self,
        episode_id: &str,
    ) -> MhResult<(Bytes, String)> {
        if !self.is_logged_in() {
            return Err(MhError::Auth("Spotify account not connected".into()));
        }
        self._stream_media(episode_id, "episode", None).await
    }

    fn _extract_sp_dc(content: &str) -> Option<String> {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            let parts: Vec<&str> = trimmed.split('\t').collect();
            if parts.len() >= 7 && parts[5] == "sp_dc" {
                return Some(parts[6].trim().to_string());
            }
        }
        None
    }

    async fn _init_totp(&mut self) -> MhResult<()> {
        if self.totp_secret.is_some() {
            return Ok(());
        }

        let resp = self
            .client
            .get(TOTP_SECRETS_URL)
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Auth("Failed to fetch TOTP secrets".into()));
        }

        let secrets: Value = resp.json().await.map_err(MhError::Network)?;
        let obj = secrets
            .as_object()
            .ok_or_else(|| MhError::Parse("TOTP secrets is not an object".into()))?;

        let (version_str, ciphertext_val) = obj
            .iter()
            .max_by_key(|(k, _)| k.parse::<i64>().unwrap_or(0))
            .ok_or_else(|| MhError::Parse("TOTP secrets object is empty".into()))?;

        let version: u32 = version_str
            .parse()
            .map_err(|e| MhError::Parse(format!("TOTP version parse error: {e}")))?;

        let cipher_bytes: Vec<u8> = ciphertext_val
            .as_array()
            .ok_or_else(|| MhError::Parse("TOTP ciphertext is not an array".into()))?
            .iter()
            .map(|v| v.as_u64().unwrap_or(0) as u8)
            .collect();

        let derived: String = cipher_bytes
            .iter()
            .enumerate()
            .map(|(i, &byte)| {
                let xored = byte ^ (((i % 33) + 9) as u8);
                xored.to_string()
            })
            .collect::<String>();

        self.totp_secret = Some(derived.into_bytes());
        self.totp_version = Some(version);
        debug!("TOTP initialised, version={version}");
        Ok(())
    }

    fn _generate_totp(&self, timestamp_sec: u64) -> String {
        let secret = self
            .totp_secret
            .as_deref()
            .expect("TOTP secret not initialised");

        let counter = timestamp_sec / TOTP_PERIOD;
        let counter_bytes = counter.to_be_bytes();

        let mut mac = <hmac::Hmac<Sha1> as hmac::Mac>::new_from_slice(secret)
            .expect("HMAC-SHA1 accepts any key length");
        mac.update(&counter_bytes);
        let result = mac.finalize().into_bytes();

        let offset = (result[19] & 0x0f) as usize;
        let binary = (((result[offset] & 0x7f) as u32) << 24)
            | (((result[offset + 1] & 0xff) as u32) << 16)
            | (((result[offset + 2] & 0xff) as u32) << 8)
            | ((result[offset + 3] & 0xff) as u32);

        let modulus = 10_u32.pow(TOTP_DIGITS);
        format!("{:0>width$}", binary % modulus, width = TOTP_DIGITS as usize)
    }

    async fn _get_server_time(&self) -> MhResult<u64> {
        let resp = self
            .client
            .get(SERVER_TIME_URL)
            .header("user-agent", UA_CHROME_LATEST)
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Auth("Failed to get Spotify server time".into()));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        data["serverTime"]
            .as_u64()
            .ok_or_else(|| MhError::Parse("serverTime field missing or not a number".into()))
    }

    async fn _refresh_token(&mut self) -> MhResult<()> {
        self._init_totp().await?;

        let server_time = self._get_server_time().await?;
        let totp = self._generate_totp(server_time);

        let sp_dc = self
            .sp_dc
            .as_deref()
            .ok_or_else(|| MhError::Auth("sp_dc not set".into()))?
            .to_string();
        let totp_version = self
            .totp_version
            .ok_or_else(|| MhError::Auth("TOTP version not initialised".into()))?
            .to_string();

        let params = [
            ("reason", "init"),
            ("productType", "web-player"),
            ("totp", totp.as_str()),
            ("totpServer", totp.as_str()),
            ("totpVer", totp_version.as_str()),
        ];

        let url = format!(
            "{}?{}",
            SESSION_TOKEN_URL,
            params
                .iter()
                .map(|(k, v)| format!("{}={}", k, url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()))
                .collect::<Vec<_>>()
                .join("&")
        );

        let resp = self
            .client
            .get(&url)
            .header("cookie", format!("sp_dc={sp_dc}"))
            .header("user-agent", UA_CHROME_LATEST)
            .header("app-platform", "WebPlayer")
            .header("spotify-app-version", CLIENT_VERSION)
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(MhError::Auth(format!(
                "Failed to get access token: {} — {}",
                status,
                &text[..text.len().min(200)]
            )));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;

        if data["accessToken"].is_null() {
            return Err(MhError::Auth(
                "No access token in response. Cookie may be expired.".into(),
            ));
        }
        if data["isAnonymous"].as_bool() == Some(true) {
            return Err(MhError::Auth(
                "Cookie is expired or invalid — got anonymous token. Re-export your cookies."
                    .into(),
            ));
        }

        self.access_token = data["accessToken"].as_str().map(String::from);
        self.client_id = data["clientId"].as_str().map(String::from);

        self.token_expiry = data["accessTokenExpirationTimestampMs"]
            .as_u64()
            .unwrap_or_else(|| now_ms() + 3_600_000);

        self._acquire_client_token().await?;
        Ok(())
    }

    async fn _get_valid_token(&mut self) -> MhResult<String> {
        if now_ms() >= self.token_expiry.saturating_sub(60_000) {
            self._refresh_token().await?;
        }
        self.access_token
            .clone()
            .ok_or_else(|| MhError::Auth("No access token available".into()))
    }

    async fn _acquire_client_token(&mut self) -> MhResult<()> {
        let client_id = match self.client_id.as_deref() {
            Some(id) => id.to_string(),
            None => return Ok(()),
        };

        let body = json!({
            "client_data": {
                "client_version": CLIENT_VERSION,
                "client_id": client_id,
                "js_sdk_data": {}
            }
        });

        let resp = self
            .client
            .post(CLIENT_TOKEN_URL)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            warn!("Client token acquisition failed: {}", resp.status());
            return Ok(());
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        self.client_token = data["granted_token"]["token"].as_str().map(String::from);
        Ok(())
    }

    async fn _fetch_profile(&mut self) -> MhResult<Value> {
        let token = self._get_valid_token().await?;

        let resp = self
            .client
            .get("https://api.spotify.com/v1/me")
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Auth(format!(
                "Profile fetch failed: {}",
                resp.status()
            )));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        let name = data["display_name"]
            .as_str()
            .or_else(|| data["id"].as_str())
            .unwrap_or("Spotify User")
            .to_string();

        Ok(json!({
            "name": name,
            "plan": data["product"],
            "email": data["email"],
            "id": data["id"],
        }))
    }

    fn _auth_headers(&self) -> Vec<(String, String)> {
        let token = self.access_token.as_deref().unwrap_or("");
        let mut h = vec![
            ("Authorization".to_string(), format!("Bearer {token}")),
            ("Accept".to_string(), "application/json".to_string()),
            ("user-agent".to_string(), UA_CHROME_LATEST.to_string()),
            ("app-platform".to_string(), "WebPlayer".to_string()),
            ("spotify-app-version".to_string(), CLIENT_VERSION.to_string()),
            (
                "origin".to_string(),
                "https://open.spotify.com/".to_string(),
            ),
            (
                "referer".to_string(),
                "https://open.spotify.com/".to_string(),
            ),
        ];
        if let Some(ct) = &self.client_token {
            h.push(("client-token".to_string(), ct.clone()));
        }
        h
    }

    fn _id_to_gid(id: &str) -> String {
        const CHARSET: &[u8] =
            b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

        let mut n: u128 = 0;
        for c in id.bytes() {
            let pos = CHARSET
                .iter()
                .position(|&b| b == c)
                .unwrap_or(0) as u128;
            n = n * 62 + pos;
        }
        format!("{:0>32x}", n)
    }

    async fn _get_playback_info(
        &mut self,
        media_type: &str,
        media_id: &str,
    ) -> MhResult<Value> {
        let base_url = PLAYBACK_INFO_URL
            .replace("{mediaType}", media_type)
            .replace("{mediaId}", media_id);

        let formats_to_try: &[&str] = if media_type == "episode" || media_type == "chapter" {
            &["file_ids_mp4", "file_ids_mp4_dual", "file_ids_ogg"]
        } else {
            &["file_ids_mp4"]
        };

        for fmt in formats_to_try {
            for attempt in 0..3u32 {
                let url = format!("{base_url}?manifestFileFormat={fmt}");

                self._get_valid_token().await?;
                let auth = self._auth_headers();
                let mut req = self.client.get(&url);
                for (k, v) in &auth {
                    req = req.header(k.as_str(), v.as_str());
                }

                let resp = req.send().await.map_err(MhError::Network)?;
                let status = resp.status();

                if status.is_success() {
                    let data: Value = resp.json().await.map_err(MhError::Network)?;
                    if self._manifest_has_files(&data) {
                        return Ok(data);
                    }
                    break;
                }

                match status.as_u16() {
                    429 if attempt < 2 => {
                        let retry_after_secs: u64 = resp
                            .headers()
                            .get("Retry-After")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        let wait_ms = std::cmp::max(retry_after_secs * 1000, 1000u64 << attempt);
                        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                        continue;
                    }
                    502 | 503 | 504 if attempt < 2 => {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            800 * (attempt as u64 + 1),
                        ))
                        .await;
                        continue;
                    }
                    400 => break, // bad format
                    _ => {
                        let text = resp.text().await.unwrap_or_default();
                        return Err(MhError::Other(format!(
                            "Playback info failed: {} — {}",
                            status,
                            &text[..text.len().min(300)]
                        )));
                    }
                }
            }
        }

        let url = format!("{base_url}?manifestFileFormat=file_ids_mp4");
        self._get_valid_token().await?;
        let auth = self._auth_headers();
        let mut req = self.client.get(&url);
        for (k, v) in &auth {
            req = req.header(k.as_str(), v.as_str());
        }

        let resp = req.send().await.map_err(MhError::Network)?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(MhError::Other(format!(
                "Playback info failed: {} — {}",
                status,
                &text[..text.len().min(300)]
            )));
        }
        Ok(resp.json().await.map_err(MhError::Network)?)
    }

    fn _manifest_has_files(&self, info: &Value) -> bool {
        let check = |m: &Value| -> bool {
            if m.is_null() {
                return false;
            }
            if let Some(arr) = m["file_ids_mp4"].as_array() {
                if !arr.is_empty() {
                    return true;
                }
            }
            if let Some(arr) = m["file_ids_ogg"].as_array() {
                if !arr.is_empty() {
                    return true;
                }
            }
            !m["url"].is_null()
        };

        if check(&info["manifest"]) {
            return true;
        }
        if let Some(media) = info["media"].as_object() {
            for entry in media.values() {
                if check(&entry["item"]["manifest"]) {
                    return true;
                }
                if check(&entry["manifest"]) {
                    return true;
                }
                if let Some(items) = entry["items"].as_array() {
                    if let Some(first) = items.first() {
                        if check(&first["manifest"]) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    async fn _resolve_storage(&mut self, file_id: &str) -> MhResult<String> {
        let token = self._get_valid_token().await?;
        let url = STORAGE_RESOLVE_URL.replace("{fileId}", file_id);

        let mut req = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .header("app-platform", "Android")
            .header("user-agent", UA_ANDROID);

        if let Some(ct) = &self.client_token {
            req = req.header("client-token", ct.clone());
        }

        let resp = req.send().await.map_err(MhError::Network)?;
        if !resp.status().is_success() {
            return Err(MhError::Other(format!(
                "Storage resolve failed: {}",
                resp.status()
            )));
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        data["cdnurl"][0]
            .as_str()
            .map(String::from)
            .ok_or_else(|| MhError::Other("No CDN URL from storage-resolve".into()))
    }

    fn _encode_varint(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            if value <= 0x7f {
                bytes.push(value as u8);
                break;
            }
            bytes.push(((value & 0x7f) | 0x80) as u8);
            value >>= 7;
        }
        bytes
    }

    fn _encode_protobuf_field(field_number: u32, data: &[u8]) -> Vec<u8> {
        let tag = ((field_number as u64) << 3) | 2;
        let mut out = Self::_encode_varint(tag);
        out.extend(Self::_encode_varint(data.len() as u64));
        out.extend_from_slice(data);
        out
    }

    fn _decode_varint(buf: &[u8], mut offset: usize) -> (u64, usize) {
        let mut value: u64 = 0;
        let mut shift = 0usize;
        let mut bytes_read = 0usize;
        while offset < buf.len() {
            let byte = buf[offset];
            offset += 1;
            bytes_read += 1;
            value |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        (value, bytes_read)
    }

    fn _parse_protobuf_field<'a>(buf: &'a [u8], target_field: u32) -> Option<&'a [u8]> {
        let mut offset = 0usize;
        while offset < buf.len() {
            let (tag, tag_bytes) = Self::_decode_varint(buf, offset);
            offset += tag_bytes;
            let field_num = (tag >> 3) as u32;
            let wire_type = tag & 0x7;

            match wire_type {
                2 => {
                    let (len, len_bytes) = Self::_decode_varint(buf, offset);
                    offset += len_bytes;
                    let len = len as usize;
                    if field_num == target_field {
                        return Some(&buf[offset..offset + len]);
                    }
                    offset += len;
                }
                0 => {
                    let (_, vb) = Self::_decode_varint(buf, offset);
                    offset += vb;
                }
                _ => break,
            }
        }
        None
    }

    async fn _acquire_playplay_key(
        &mut self,
        file_id: &str,
        challenge: &[u8],
    ) -> MhResult<Vec<u8>> {
        let file_id_bytes =
            hex::decode(file_id).map_err(|e| MhError::Crypto(format!("hex decode: {e}")))?;

        let field1 = Self::_encode_protobuf_field(1, challenge);
        let field2 = Self::_encode_protobuf_field(2, &file_id_bytes);
        let mut body = field1;
        body.extend(field2);

        let token = self._get_valid_token().await?;
        let url = PLAYPLAY_LICENSE_URL.replace("{fileId}", file_id);

        let mut req = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/x-protobuf")
            .header("Accept", "application/x-protobuf")
            .header("app-platform", "Android")
            .header("user-agent", UA_ANDROID)
            .body(body);

        if let Some(ct) = &self.client_token.clone() {
            req = req.header("client-token", ct.clone());
        }

        let resp = req.send().await.map_err(MhError::Network)?;
        if !resp.status().is_success() {
            return Err(MhError::Crypto(format!(
                "PlayPlay license failed: {}",
                resp.status()
            )));
        }

        let resp_bytes = resp.bytes().await.map_err(MhError::Network)?;
        let obfuscated_key =
            Self::_parse_protobuf_field(&resp_bytes, 1).ok_or_else(|| {
                MhError::Crypto("No obfuscated key in PlayPlay response".into())
            })?;

        Ok(obfuscated_key.to_vec())
    }

    fn _derive_aes_key(obfuscated_key: &[u8; 16], file_id: &[u8; 20]) -> MhResult<[u8; 16]> {
        Ok(crate::crypto::playplay::decrypt_and_bind_key(obfuscated_key, file_id))
    }

    async fn _get_gid_metadata(&mut self, track_id: &str, media_type: &str) -> MhResult<Value> {
        let gid = Self::_id_to_gid(track_id);
        let token = self._get_valid_token().await?;

        let mut headers_vec = vec![
            ("Accept".to_string(), "application/json".to_string()),
            ("app-platform".to_string(), "Android".to_string()),
            ("user-agent".to_string(), UA_ANDROID.to_string()),
            ("Authorization".to_string(), format!("Bearer {token}")),
        ];
        if let Some(ct) = &self.client_token.clone() {
            headers_vec.push(("client-token".to_string(), ct.clone()));
        }

        let base = format!(
            "https://spclient.wg.spotify.com/metadata/4/{media_type}/{gid}"
        );

        for url in [format!("{base}?market=from_token"), base.clone()] {
            let mut req = self.client.get(&url);
            for (k, v) in &headers_vec {
                req = req.header(k.as_str(), v.as_str());
            }
            let resp = req.send().await.map_err(MhError::Network)?;
            if resp.status().is_success() {
                return Ok(resp.json().await.map_err(MhError::Network)?);
            }
            if resp.status().as_u16() != 404 {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                return Err(MhError::Other(format!(
                    "GID metadata failed: {} — {}",
                    status,
                    &text[..text.len().min(300)]
                )));
            }
        }

        Err(MhError::NotFound(format!(
            "GID metadata not found for {media_type} {track_id}"
        )))
    }

    fn _select_ogg_file<'a>(&self, files: &'a [Value]) -> Option<&'a Value> {
        let priority = ["OGG_VORBIS_320", "OGG_VORBIS_160", "OGG_VORBIS_96"];
        for fmt in &priority {
            if let Some(f) = files
                .iter()
                .find(|f| f["format"].as_str() == Some(fmt))
            {
                return Some(f);
            }
        }
        files
            .iter()
            .find(|f| f["format"].as_str().map_or(false, |s| s.starts_with("OGG_VORBIS")))
    }

    fn _select_audio_file<'a>(&self, files: &'a [Value]) -> Option<(&'a Value, bool)> {
        if let Some(f) = self._select_ogg_file(files) {
            return Some((f, true));
        }
        for fmt in &["MP3_320", "MP3_256", "MP3_160", "MP3_96", "MP3_128"] {
            if let Some(f) = files.iter().find(|f| f["format"].as_str() == Some(fmt)) {
                return Some((f, false));
            }
        }
        for fmt in &["AAC_24", "AAC_48", "MP4_128", "MP4_256"] {
            if let Some(f) = files.iter().find(|f| f["format"].as_str() == Some(fmt)) {
                return Some((f, false));
            }
        }
        files
            .iter()
            .find(|f| !f["file_id"].is_null())
            .map(|f| (f, false))
    }

    fn _decrypt_spotify_buffer(key: &[u8; 16], encrypted: &[u8]) -> Vec<u8> {
        use aes::cipher::generic_array::GenericArray;

        let cipher = Aes128::new_from_slice(key).expect("valid 16-byte key");
        let block_size = 16usize;
        let mut output = Vec::with_capacity(encrypted.len());

        for (block_idx, chunk) in encrypted.chunks(block_size).enumerate() {
            let mut iv = AES_IV;
            let mut carry = block_idx as u64;
            let mut j = 15i32;
            while j >= 0 && carry > 0 {
                carry += iv[j as usize] as u64;
                iv[j as usize] = (carry & 0xff) as u8;
                carry >>= 8;
                j -= 1;
            }

            let mut keystream_block = GenericArray::from(iv);
            cipher.encrypt_block(&mut keystream_block);

            for (i, &byte) in chunk.iter().enumerate() {
                output.push(byte ^ keystream_block[i]);
            }
        }

        output
    }

    fn _strip_spotify_header(decrypted: &[u8]) -> &[u8] {
        if let Some(pos) = decrypted
            .windows(4)
            .position(|w| w == b"OggS")
        {
            return &decrypted[pos..];
        }
        if decrypted.len() > 0xa7 {
            return &decrypted[0xa7..];
        }
        decrypted
    }

    fn _extract_feed_url(data: &Value) -> Option<String> {
        let d = if !data["show"].is_null() {
            &data["show"]
        } else {
            data
        };
        for field in &[
            "rssFeedUrl", "rss_url", "feed_url", "feedUrl", "rssUrl",
            "external_url",
        ] {
            if let Some(s) = d[field].as_str() {
                return Some(s.to_string());
            }
        }
        None
    }

    fn _extract_episode_external_url(data: &Value) -> Option<String> {
        let ep = if !data["episode"].is_null() {
            &data["episode"]
        } else {
            data
        };
        for field in &["externalUrl", "external_url", "audioUrl"] {
            if let Some(s) = ep[field].as_str() {
                return Some(s.to_string());
            }
        }
        if let Some(s) = ep["audio"]["url"].as_str() {
            return Some(s.to_string());
        }
        if let Some(s) = ep["media"]["url"].as_str() {
            return Some(s.to_string());
        }
        None
    }

    async fn _get_external_episode_url(
        &mut self,
        episode_id: &str,
        show_id: Option<&str>,
        _show_name: Option<&str>,
        episode_name: Option<&str>,
        duration_ms: u64,
    ) -> Option<String> {
        let token = match self._get_valid_token().await {
            Ok(t) => t,
            Err(_) => return None,
        };

        let mut hdr_pairs: Vec<(String, String)> = vec![
            ("Authorization".to_string(), format!("Bearer {token}")),
            ("Accept".to_string(), "application/json".to_string()),
            ("app-platform".to_string(), "Android".to_string()),
            ("user-agent".to_string(), UA_ANDROID.to_string()),
        ];
        if let Some(ct) = &self.client_token.clone() {
            hdr_pairs.push(("client-token".to_string(), ct.clone()));
        }

        let try_fetch = |client: &Client, url: String, pairs: Vec<(String, String)>| {
            let client = client.clone();
            async move {
                let mut req = client.get(&url);
                for (k, v) in &pairs {
                    req = req.header(k.as_str(), v.as_str());
                }
                match req.send().await {
                    Ok(r) if r.status().is_success() => r.json::<Value>().await.ok(),
                    _ => None,
                }
            }
        };

        let pe_ep = try_fetch(
            &self.client,
            format!(
                "https://spclient.wg.spotify.com/podcast-experience/v2/episodes/{episode_id}"
            ),
            hdr_pairs.clone(),
        )
        .await;

        if let Some(ref data) = pe_ep {
            if let Some(url) = Self::_extract_episode_external_url(data) {
                return Some(url);
            }
            if let Some(feed_url) = Self::_extract_feed_url(data) {
                if let Some(url) = self
                    ._find_episode_in_rss(&feed_url, episode_id, episode_name, duration_ms)
                    .await
                {
                    return Some(url);
                }
            }
        }

        if let Some(sid) = show_id {
            let pe_show = try_fetch(
                &self.client,
                format!(
                    "https://spclient.wg.spotify.com/podcast-experience/v2/shows/{sid}"
                ),
                hdr_pairs.clone(),
            )
            .await;

            if let Some(ref data) = pe_show {
                if let Some(feed_url) = Self::_extract_feed_url(data) {
                    if let Some(url) = self
                        ._find_episode_in_rss(&feed_url, episode_id, episode_name, duration_ms)
                        .await
                    {
                        return Some(url);
                    }
                }
            }

            let show_gid = Self::_id_to_gid(sid);
            let base = format!(
                "https://spclient.wg.spotify.com/metadata/4/podcast/{show_gid}"
            );
            for url in [format!("{base}?market=from_token"), base] {
                let data = try_fetch(&self.client, url, hdr_pairs.clone()).await;
                if let Some(ref d) = data {
                    if let Some(feed_url) = Self::_extract_feed_url(d) {
                        if let Some(audio_url) = self
                            ._find_episode_in_rss(
                                &feed_url,
                                episode_id,
                                episode_name,
                                duration_ms,
                            )
                            .await
                        {
                            return Some(audio_url);
                        }
                    }
                    break;
                }
            }
        }

        None
    }

    async fn _find_episode_in_rss(
        &self,
        feed_url: &str,
        episode_id: &str,
        episode_name: Option<&str>,
        duration_ms: u64,
    ) -> Option<String> {
        let resp = self
            .client
            .get(feed_url)
            .header("User-Agent", "Mozilla/5.0 (compatible; podcast-player/1.0)")
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            return None;
        }

        let text = resp.text().await.ok()?;
        let spotify_uri = format!("spotify:episode:{episode_id}");

        let mut best_match: Option<String> = None;
        let mut current_audio_url: Option<String> = None;
        let mut current_title: Option<String> = None;
        let mut current_guid: Option<String> = None;
        let mut current_duration: Option<String> = None;
        let mut in_item = false;

        let mut reader = quick_xml::Reader::from_str(&text);
        reader.config_mut().trim_text(true);

        loop {
            use quick_xml::events::Event;
            match reader.read_event() {
                Ok(Event::Start(ref e)) => {
                    let name = std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_lowercase();
                    if name == "item" {
                        in_item = true;
                        current_audio_url = None;
                        current_title = None;
                        current_guid = None;
                        current_duration = None;
                    }
                }
                Ok(Event::Empty(ref e)) if in_item => {
                    let name = std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_lowercase();
                    if name == "enclosure" {
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("").to_lowercase();
                            if key == "url" {
                                current_audio_url =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    }
                }
                Ok(Event::Text(ref t)) if in_item => {
                    let _ = t;
                }
                Ok(Event::End(ref e)) => {
                    let name = std::str::from_utf8(e.name().as_ref()).unwrap_or("").to_lowercase();
                    if name == "item" {
                        in_item = false;
                        if let Some(ref audio_url) = current_audio_url.clone() {
                            if current_guid.as_deref() == Some(&spotify_uri) {
                                return Some(audio_url.clone());
                            }
                            if let (Some(ref ep_name), Some(ref title)) =
                                (episode_name, &current_title)
                            {
                                if title.to_lowercase().trim()
                                    == ep_name.to_lowercase().trim()
                                {
                                    return Some(audio_url.clone());
                                }
                                if ep_name.len() >= 20
                                    && title
                                        .to_lowercase()
                                        .contains(&ep_name.to_lowercase()[..20])
                                {
                                    best_match = best_match.or_else(|| Some(audio_url.clone()));
                                }
                            }
                            if duration_ms > 0 {
                                if let Some(ref dur_str) = current_duration {
                                    let parsed = Self::_parse_itunes_duration(dur_str);
                                    if (parsed as i64 - duration_ms as i64).unsigned_abs() < 5000 {
                                        best_match = Some(audio_url.clone());
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }

        let _ = Self::_rss_extract_item_texts(&text, episode_id, episode_name, duration_ms);

        best_match
    }

    fn _rss_extract_item_texts(
        xml: &str,
        episode_id: &str,
        episode_name: Option<&str>,
        duration_ms: u64,
    ) -> Option<String> {
        let spotify_uri = format!("spotify:episode:{episode_id}");
        let mut best: Option<String> = None;
        let mut in_item = false;
        let mut cur_url: Option<String> = None;
        let mut cur_title: Option<String> = None;
        let mut cur_guid: Option<String> = None;
        let mut cur_dur: Option<String> = None;
        let mut cur_tag = String::new();

        let mut reader = quick_xml::Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        loop {
            use quick_xml::events::Event;
            match reader.read_event() {
                Ok(Event::Start(ref e)) => {
                    let name_bytes = e.name();
                    let raw = std::str::from_utf8(name_bytes.as_ref()).unwrap_or("");
                    let name = raw.to_lowercase();
                    if name == "item" {
                        in_item = true;
                        cur_url = None;
                        cur_title = None;
                        cur_guid = None;
                        cur_dur = None;
                    } else if in_item {
                        cur_tag = name.clone();
                        if name == "enclosure" {
                            for attr in e.attributes().flatten() {
                                let key = std::str::from_utf8(attr.key.as_ref())
                                    .unwrap_or("")
                                    .to_lowercase();
                                if key == "url" {
                                    cur_url =
                                        Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                            }
                        }
                    }
                }
                Ok(Event::Empty(ref e)) if in_item => {
                    let name = std::str::from_utf8(e.name().as_ref())
                        .unwrap_or("")
                        .to_lowercase();
                    if name == "enclosure" {
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref())
                                .unwrap_or("")
                                .to_lowercase();
                            if key == "url" {
                                cur_url =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    }
                }
                Ok(Event::Text(ref t)) if in_item => {
                    let txt = t.unescape().map(|s| s.to_string()).unwrap_or_default();
                    match cur_tag.as_str() {
                        "title" => cur_title = Some(txt),
                        "guid" => cur_guid = Some(txt),
                        "itunes:duration" => cur_dur = Some(txt),
                        _ => {}
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = std::str::from_utf8(e.name().as_ref())
                        .unwrap_or("")
                        .to_lowercase();
                    if name == "item" {
                        in_item = false;
                        if let Some(ref audio_url) = cur_url {
                            if cur_guid.as_deref() == Some(&spotify_uri) {
                                return Some(audio_url.clone());
                            }
                            if let (Some(ref ep_name), Some(ref title)) =
                                (episode_name, &cur_title)
                            {
                                if title.to_lowercase().trim()
                                    == ep_name.to_lowercase().trim()
                                {
                                    return Some(audio_url.clone());
                                }
                                if ep_name.len() >= 20
                                    && title
                                        .to_lowercase()
                                        .contains(&ep_name.to_lowercase()[..20])
                                {
                                    best = best.or_else(|| Some(audio_url.clone()));
                                }
                            }
                            if duration_ms > 0 {
                                if let Some(ref d) = cur_dur {
                                    let parsed = Self::_parse_itunes_duration(d);
                                    if (parsed as i64 - duration_ms as i64).unsigned_abs() < 5000 {
                                        best = Some(audio_url.clone());
                                    }
                                }
                            }
                        }
                        cur_tag.clear();
                    } else if name == cur_tag {
                        cur_tag.clear();
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }
        best
    }

    fn _parse_itunes_duration(duration: &str) -> u64 {
        let parts: Vec<&str> = duration.split(':').collect();
        match parts.len() {
            3 => {
                let h: u64 = parts[0].parse().unwrap_or(0);
                let m: u64 = parts[1].parse().unwrap_or(0);
                let s: u64 = parts[2].parse().unwrap_or(0);
                (h * 3600 + m * 60 + s) * 1000
            }
            2 => {
                let m: u64 = parts[0].parse().unwrap_or(0);
                let s: u64 = parts[1].parse().unwrap_or(0);
                (m * 60 + s) * 1000
            }
            _ => {
                let secs: f64 = duration.parse().unwrap_or(0.0);
                (secs * 1000.0) as u64
            }
        }
    }

    async fn _stream_mp4_with_widevine(
        &mut self,
        cdn_url: &str,
        file_id: &str,
        venv_python: Option<&Path>,
    ) -> MhResult<Bytes> {
        let venv_python = venv_python
            .ok_or_else(|| MhError::Other("venv_python required for Widevine decryption".into()))?;

        let seek_resp = self
            .client
            .get(&format!(
                "https://seektables.scdn.co/seektable/{file_id}.json"
            ))
            .header("Accept", "*/*")
            .header("Origin", "https://open.spotify.com")
            .header("Referer", "https://open.spotify.com/")
            .header("User-Agent", UA_CHROME_LATEST)
            .send()
            .await
            .map_err(MhError::Network)?;

        if !seek_resp.status().is_success() {
            return Err(MhError::Other(format!(
                "Seek table failed: {}",
                seek_resp.status()
            )));
        }

        let seek_data: Value = seek_resp.json().await.map_err(MhError::Network)?;
        let pssh = seek_data["pssh"]
            .as_str()
            .or_else(|| seek_data["widevine_pssh"].as_str())
            .ok_or_else(|| MhError::Other("No PSSH found in seek table".into()))?
            .to_string();

        let access_token = self._get_valid_token().await?;
        let client_token = self.client_token.clone().unwrap_or_default();

        let decrypt_key_hex = self
            ._get_widevine_key_via_python(
                &pssh,
                venv_python,
                &access_token,
                &client_token,
            )
            .await?;

        let ffmpeg_bin = crate::venv_manager::resolve_ffmpeg();
        let mut child = Command::new(&ffmpeg_bin)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-decryption_key",
                &decrypt_key_hex,
                "-i",
                cdn_url,
                "-c",
                "copy",
                "-f",
                "adts",
                "pipe:1",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| MhError::Subprocess(format!("ffmpeg spawn failed: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MhError::Subprocess("No stdout from ffmpeg".into()))?;

        let mut buf = Vec::new();
        let mut reader = BufReader::new(stdout);
        tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut buf).await?;

        child
            .wait()
            .await
            .map_err(|e| MhError::Subprocess(format!("ffmpeg wait: {e}")))?;

        Ok(Bytes::from(buf))
    }

    async fn _get_widevine_key_via_python(
        &self,
        pssh: &str,
        venv_python: &Path,
        access_token: &str,
        client_token: &str,
    ) -> MhResult<String> {
        let py_script = r#"
import sys, json, base64
from pywidevine import PSSH, Cdm, Device

args = json.loads(sys.stdin.readline())
device = Device.load(args["wvd_path"])
cdm = Cdm.from_device(device)
session_id = cdm.open()
pssh_obj = PSSH(args["pssh"])
challenge = cdm.get_license_challenge(session_id, pssh_obj)

sys.stdout.write(json.dumps({"challenge": base64.b64encode(challenge).decode()}) + "\n")
sys.stdout.flush()

license_line = sys.stdin.readline().strip()
license_b64 = json.loads(license_line)["license"]
license_bytes = base64.b64decode(license_b64)

cdm.parse_license(session_id, license_bytes)
keys = [k for k in cdm.get_keys(session_id) if k.type == "CONTENT"]
cdm.close(session_id)

if not keys:
    sys.stdout.write(json.dumps({"error": "No content keys in license response"}) + "\n")
    sys.exit(1)

kid_hex = keys[0].kid.hex if isinstance(keys[0].kid.hex, str) else keys[0].kid.hex()
key_hex = keys[0].key.hex() if callable(keys[0].key.hex) else keys[0].key.hex
sys.stdout.write(json.dumps({"key": key_hex, "kid": kid_hex}) + "\n")
"#;

        let wvd = self.wvd_path.as_deref().unwrap_or("");
        if wvd.is_empty() {
            return Err(MhError::Other(
                "Widevine device file (.wvd) not configured. Set the path in Settings → Spotify → Widevine Device Path.".into()
            ));
        }
        let input_json = serde_json::to_string(&json!({
            "pssh": pssh,
            "wvd_path": wvd
        }))
        .map_err(|e| MhError::Parse(e.to_string()))?
            + "\n";

        let mut child = Command::new(venv_python)
            .arg("-c")
            .arg(py_script)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| MhError::Subprocess(format!("pywidevine spawn failed: {e}")))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| MhError::Subprocess("No stdin for pywidevine".into()))?;
        stdin.write_all(input_json.as_bytes()).await?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MhError::Subprocess("No stdout from pywidevine".into()))?;

        let mut lines = BufReader::new(stdout).lines();
        let challenge_line = lines
            .next_line()
            .await?
            .ok_or_else(|| MhError::Subprocess("No challenge from pywidevine".into()))?;

        let challenge_data: Value =
            serde_json::from_str(&challenge_line).map_err(|e| MhError::Parse(e.to_string()))?;
        let challenge_b64 = challenge_data["challenge"]
            .as_str()
            .ok_or_else(|| MhError::Subprocess("No challenge field".into()))?;

        let challenge_bytes = B64
            .decode(challenge_b64)
            .map_err(|e| MhError::Crypto(format!("base64 decode challenge: {e}")))?;

        let license_resp = self
            .client
            .post("https://gue1-spclient.spotify.com/widevine-license/v1/audio/license")
            .header("Authorization", format!("Bearer {access_token}"))
            .header("client-token", client_token)
            .header("Content-Type", "application/octet-stream")
            .header("Accept", "*/*")
            .header("app-platform", "WebPlayer")
            .header("spotify-app-version", CLIENT_VERSION)
            .header("Origin", "https://open.spotify.com")
            .header("Referer", "https://open.spotify.com/")
            .header("User-Agent", UA_CHROME_LATEST)
            .body(challenge_bytes)
            .send()
            .await
            .map_err(MhError::Network)?;

        if !license_resp.status().is_success() {
            let status = license_resp.status();
            let text = license_resp.text().await.unwrap_or_default();
            return Err(MhError::Other(format!(
                "Widevine license server returned {status}: {}",
                &text[..text.len().min(200)]
            )));
        }

        let license_bytes = license_resp.bytes().await.map_err(MhError::Network)?;
        let license_b64 = B64.encode(&license_bytes);
        let license_input = serde_json::to_string(&json!({ "license": license_b64 }))
            .map_err(|e| MhError::Parse(e.to_string()))?
            + "\n";

        stdin.write_all(license_input.as_bytes()).await?;
        drop(stdin);

        let result_line = lines
            .next_line()
            .await?
            .ok_or_else(|| MhError::Subprocess("No result from pywidevine".into()))?;

        let result: Value = serde_json::from_str(&result_line).map_err(|e| MhError::Parse(e.to_string()))?;
        if let Some(err) = result["error"].as_str() {
            return Err(MhError::Crypto(err.to_string()));
        }

        result["key"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| MhError::Crypto("No key in pywidevine result".into()))
    }

    async fn _stream_media(
        &mut self,
        media_id: &str,
        media_type: &str,
        venv_python: Option<&Path>,
    ) -> MhResult<(Bytes, String)> {
        let mut file_id: Option<String> = None;
        let mut is_ogg = false;
        let mut episode_external_url: Option<String> = None;
        let mut duration_ms: u64 = 0;
        let mut gid_error: Option<String> = None;

        match self._get_gid_metadata(media_id, media_type).await {
            Ok(metadata) => {
                duration_ms = metadata["duration"].as_u64().unwrap_or(0);

                let mut files: Option<Vec<Value>> = None;
                for key in &["file", "audio"] {
                    if let Some(arr) = metadata[key].as_array() {
                        if !arr.is_empty() {
                            files = Some(arr.clone());
                            break;
                        }
                    }
                }

                if files.as_ref().map_or(true, |f| f.is_empty()) {
                    if let Some(alts) = metadata["alternative"].as_array() {
                        for alt in alts {
                            for key in &["file", "audio"] {
                                if let Some(arr) = alt[key].as_array() {
                                    if !arr.is_empty() {
                                        files = Some(arr.clone());
                                        break;
                                    }
                                }
                            }
                            if files.is_some() {
                                break;
                            }
                        }
                    }
                }

                if let Some(ref flist) = files {
                    if let Some((selected, ogg)) = self._select_audio_file(flist) {
                        file_id = selected["file_id"].as_str().map(String::from);
                        is_ogg = ogg;
                    }
                }

                if file_id.is_none() {
                    if let Some(ext) = metadata["external_url"].as_str() {
                        episode_external_url = Some(ext.to_string());
                    }
                }
            }
            Err(e) => {
                gid_error = Some(e.to_string());
            }
        }

        if file_id.is_none() {
            if let Some(ref ext_url) = episode_external_url {
                let resp = self
                    .client
                    .get(ext_url)
                    .send()
                    .await
                    .map_err(MhError::Network)?;
                if !resp.status().is_success() {
                    return Err(MhError::Other(format!(
                        "External episode fetch failed: {}",
                        resp.status()
                    )));
                }
                let bytes = resp.bytes().await.map_err(MhError::Network)?;
                return Ok((bytes, "audio/mpeg".to_string()));
            }
        }

        if file_id.is_none() {
            let playback_info = match self._get_playback_info(media_type, media_id).await {
                Ok(info) => info,
                Err(e) => {
                    let msg = if let Some(ref ge) = gid_error {
                        format!("GID: {ge} | Playback: {e}")
                    } else {
                        e.to_string()
                    };
                    return Err(MhError::Other(format!("No playable file found: {msg}")));
                }
            };

            let mut manifest = playback_info["manifest"].clone();
            let mut item_meta = playback_info["metadata"].clone();

            if manifest.is_null() {
                if let Some(media) = playback_info["media"].as_object() {
                    'outer: for entry in media.values() {
                        for path in &[
                            ("item", "manifest"),
                        ] {
                            let m = &entry[path.0][path.1];
                            if !m.is_null() {
                                manifest = m.clone();
                                item_meta = entry[path.0]["metadata"].clone();
                                break 'outer;
                            }
                        }
                        if !entry["manifest"].is_null() {
                            manifest = entry["manifest"].clone();
                            item_meta = entry["metadata"].clone();
                            break;
                        }
                        if let Some(items) = entry["items"].as_array() {
                            if let Some(first) = items.first() {
                                if !first["manifest"].is_null() {
                                    manifest = first["manifest"].clone();
                                    item_meta = first["metadata"].clone();
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            if let Some(d) = item_meta["duration"].as_u64() {
                duration_ms = d;
            }

            let no_mp4 = manifest["file_ids_mp4"]
                .as_array()
                .map_or(true, |a| a.is_empty());
            let no_ogg = manifest["file_ids_ogg"]
                .as_array()
                .map_or(true, |a| a.is_empty());
            if !manifest["url"].is_null() && no_mp4 && no_ogg {
                let url = manifest["url"].as_str().unwrap_or("");
                let resp = self
                    .client
                    .get(url)
                    .send()
                    .await
                    .map_err(MhError::Network)?;
                if !resp.status().is_success() {
                    return Err(MhError::Other(format!(
                        "External episode fetch failed: {}",
                        resp.status()
                    )));
                }
                let ct = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("audio/mpeg")
                    .to_string();
                let bytes = resp.bytes().await.map_err(MhError::Network)?;
                return Ok((bytes, ct));
            }

            if let Some(ogg_files) = manifest["file_ids_ogg"].as_array() {
                if !ogg_files.is_empty() {
                    let preferred = ogg_files
                        .iter()
                        .find(|f| f["format"].as_str() == Some("OGG_VORBIS_320"))
                        .or_else(|| {
                            ogg_files
                                .iter()
                                .find(|f| f["format"].as_str() == Some("OGG_VORBIS_160"))
                        })
                        .or_else(|| ogg_files.first());
                    if let Some(f) = preferred {
                        file_id = f["file_id"].as_str().map(String::from);
                        is_ogg = true;
                    }
                }
            }

            if file_id.is_none() {
                if let Some(mp4_files) = manifest["file_ids_mp4"].as_array() {
                    if !mp4_files.is_empty() {
                        let preferred = mp4_files
                            .iter()
                            .find(|f| f["format"].as_str() == Some("11"))
                            .or_else(|| {
                                mp4_files.iter().find(|f| f["format"].as_str() == Some("10"))
                            })
                            .or_else(|| mp4_files.first());
                        if let Some(f) = preferred {
                            file_id = f["file_id"].as_str().map(String::from);
                        }
                    }
                }
            }

            if file_id.is_none() {
                if let Some(dual) = manifest["file_ids_mp4_dual"].as_array() {
                    let entries = dual
                        .iter()
                        .find(|e| {
                            e["type"].as_str() == Some("audio")
                                || e["qualities"].as_array().map_or(false, |q| !q.is_empty())
                        })
                        .map(|e| e["qualities"].as_array())
                        .flatten()
                        .map(|q| q.to_vec())
                        .unwrap_or_else(|| dual.to_vec());

                    let preferred = entries
                        .iter()
                        .find(|f| f["format"].as_str() == Some("11"))
                        .or_else(|| {
                            entries.iter().find(|f| f["format"].as_str() == Some("10"))
                        })
                        .or_else(|| entries.first());
                    if let Some(f) = preferred {
                        file_id = f["file_id"].as_str().map(String::from);
                    }
                }
            }

            if file_id.is_none() && media_type == "episode" {
                let show_id = item_meta["group_uri"]
                    .as_str()
                    .and_then(|s| s.split(':').last())
                    .map(String::from);
                let show_name = item_meta["group_name"]
                    .as_str()
                    .or_else(|| item_meta["context_description"].as_str())
                    .map(String::from);
                let ep_name = item_meta["name"].as_str().map(String::from);
                let ep_dur = item_meta["duration"].as_u64().unwrap_or(duration_ms);

                if let Some(audio_url) = self
                    ._get_external_episode_url(
                        media_id,
                        show_id.as_deref(),
                        show_name.as_deref(),
                        ep_name.as_deref(),
                        ep_dur,
                    )
                    .await
                {
                    let resp = self
                        .client
                        .get(&audio_url)
                        .send()
                        .await
                        .map_err(MhError::Network)?;
                    if resp.status().is_success() {
                        let ct = resp
                            .headers()
                            .get("content-type")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("audio/mpeg")
                            .to_string();
                        let bytes = resp.bytes().await.map_err(MhError::Network)?;
                        return Ok((bytes, ct));
                    }
                }
            }

            if file_id.is_none() {
                let gid_msg = gid_error
                    .as_deref()
                    .map(|e| format!(" (GID error: {e})"))
                    .unwrap_or_default();
                return Err(MhError::Other(format!(
                    "No playable file found. Neither OGG nor MP4 files available.{gid_msg}"
                )));
            }
        }

        let fid = file_id.unwrap();

        let cdn_url = self._resolve_storage(&fid).await?;

        let pp_token = crate::crypto::playplay::get_token();
        let fid_bytes = hex::decode(&fid).map_err(|e| MhError::Crypto(format!("{e}")))?;
        let fid_arr: [u8; 20] = fid_bytes.try_into()
            .map_err(|_| MhError::Crypto("file_id must be 20 bytes".into()))?;
        let decrypted = match self._acquire_playplay_key(&fid, &pp_token).await {
            Ok(playplay_key) => {
                let obf_arr: [u8; 16] = playplay_key[..16].try_into()
                    .map_err(|_| MhError::Crypto("playplay key must be 16 bytes".into()))?;
                let aes_key = Self::_derive_aes_key(&obf_arr, &fid_arr)?;

                let cdn_resp = self
                    .client
                    .get(&cdn_url)
                    .send()
                    .await
                    .map_err(MhError::Network)?;
                if !cdn_resp.status().is_success() {
                    return Err(MhError::Other(format!(
                        "CDN fetch failed: {}",
                        cdn_resp.status()
                    )));
                }
                let encrypted = cdn_resp.bytes().await.map_err(MhError::Network)?;

                let decrypted_buf = Self::_decrypt_spotify_buffer(&aes_key, &encrypted);

                if is_ogg {
                    let stripped = Self::_strip_spotify_header(&decrypted_buf);
                    Bytes::copy_from_slice(stripped)
                } else {
                    Bytes::from(decrypted_buf)
                }
            }
            Err(e) => {
                if !is_ogg {
                    warn!("PlayPlay key failed ({e}), trying Widevine fallback");
                    return match self
                        ._stream_mp4_with_widevine(&cdn_url, &fid, venv_python)
                        .await
                    {
                        Ok(bytes) => Ok((bytes, "audio/aac".to_string())),
                        Err(e2) => Err(e2),
                    };
                }
                return Err(e);
            }
        };

        let content_type = if is_ogg {
            "audio/ogg"
        } else {
            "audio/mp4"
        };

        Ok((decrypted, content_type.to_string()))
    }

    fn _transform_search_results(&self, results: &Value, search_type: &str) -> Value {
        let uri2url = |uri: &str| -> Option<String> {
            let re = regex::Regex::new(
                r"spotify:(track|album|artist|playlist|show|episode|audiobook):(.+)",
            )
            .ok()?;
            let caps = re.captures(uri)?;
            Some(format!(
                "https://open.spotify.com/{}/{}",
                &caps[1], &caps[2]
            ))
        };

        let uri2id = |uri: &str| -> String {
            uri.split(':').last().unwrap_or("").to_string()
        };

        let mut out = serde_json::Map::new();

        let empty_arr = Value::Array(vec![]);

        if !results["tracks"].is_null()
            && (search_type == "track" || search_type.is_empty())
        {
            let items: Vec<Value> = results["tracks"]["hits"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|t| {
                    let uri = t["uri"].as_str().unwrap_or("");
                    let artists: Vec<Value> = t["artists"]
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .map(|a| {
                            let a_uri = a["uri"].as_str().unwrap_or("");
                            json!({
                                "name": a["name"],
                                "id": uri2id(a_uri),
                                "uri": a["uri"],
                            })
                        })
                        .collect();
                    let images: Vec<Value> = if !t["image"].is_null() {
                        vec![json!({ "url": t["image"] })]
                    } else {
                        vec![]
                    };
                    json!({
                        "name": t["name"],
                        "id": uri2id(uri),
                        "uri": t["uri"],
                        "external_urls": { "spotify": uri2url(uri) },
                        "artists": artists,
                        "album": {
                            "name": t["album"]["name"],
                            "uri": t["album"]["uri"],
                            "images": images,
                        },
                        "duration_ms": t["duration"],
                        "explicit": t["explicit"].as_bool().unwrap_or(false),
                    })
                })
                .collect();
            out.insert("tracks".to_string(), json!({ "items": items }));
        }

        if !results["albums"].is_null()
            && (search_type == "album" || search_type.is_empty())
        {
            let items: Vec<Value> = results["albums"]["hits"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|a| {
                    let uri = a["uri"].as_str().unwrap_or("");
                    let artists: Vec<Value> = a["artists"]
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .map(|ar| {
                            let ar_uri = ar["uri"].as_str().unwrap_or("");
                            json!({
                                "name": ar["name"],
                                "id": uri2id(ar_uri),
                                "uri": ar["uri"],
                            })
                        })
                        .collect();
                    let images: Vec<Value> = if !a["image"].is_null() {
                        vec![json!({ "url": a["image"] })]
                    } else {
                        vec![]
                    };
                    json!({
                        "name": a["name"],
                        "id": uri2id(uri),
                        "uri": a["uri"],
                        "external_urls": { "spotify": uri2url(uri) },
                        "artists": artists,
                        "images": images,
                    })
                })
                .collect();
            out.insert("albums".to_string(), json!({ "items": items }));
        }

        if !results["artists"].is_null()
            && (search_type == "artist" || search_type.is_empty())
        {
            let items: Vec<Value> = results["artists"]["hits"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|a| {
                    let uri = a["uri"].as_str().unwrap_or("");
                    let images: Vec<Value> = if !a["image"].is_null() {
                        vec![json!({ "url": a["image"] })]
                    } else {
                        vec![]
                    };
                    json!({
                        "name": a["name"],
                        "id": uri2id(uri),
                        "uri": a["uri"],
                        "external_urls": { "spotify": uri2url(uri) },
                        "images": images,
                    })
                })
                .collect();
            out.insert("artists".to_string(), json!({ "items": items }));
        }

        if !results["playlists"].is_null()
            && (search_type == "playlist" || search_type.is_empty())
        {
            let items: Vec<Value> = results["playlists"]["hits"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|p| {
                    let uri = p["uri"].as_str().unwrap_or("");
                    let images: Vec<Value> = if !p["image"].is_null() {
                        vec![json!({ "url": p["image"] })]
                    } else {
                        vec![]
                    };
                    json!({
                        "name": p["name"],
                        "id": uri2id(uri),
                        "uri": p["uri"],
                        "external_urls": { "spotify": uri2url(uri) },
                        "images": images,
                        "owner": { "display_name": p["author"] },
                        "tracks": { "total": p["followersCount"] },
                    })
                })
                .collect();
            out.insert("playlists".to_string(), json!({ "items": items }));
        }

        let shows_hits = if !results["podcasts"].is_null() {
            Some(&results["podcasts"])
        } else if !results["shows"].is_null() {
            Some(&results["shows"])
        } else {
            None
        };

        if let Some(sh) = shows_hits {
            if search_type == "podcast"
                || search_type == "show"
                || search_type.is_empty()
            {
                let items: Vec<Value> = sh["hits"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|s| {
                        let uri = s["uri"].as_str().unwrap_or("");
                        let images: Vec<Value> = if !s["image"].is_null() {
                            vec![json!({ "url": s["image"] })]
                        } else {
                            vec![]
                        };
                        json!({
                            "name": s["name"],
                            "id": uri2id(uri),
                            "uri": s["uri"],
                            "external_urls": { "spotify": uri2url(uri) },
                            "images": images,
                            "publisher": s["author"],
                            "total_episodes": s["episodeCount"],
                            "media_type": s["mediaType"],
                        })
                    })
                    .collect();
                out.insert("shows".to_string(), json!({ "items": items }));
            }
        }

        let episodes_hits = if !results["podcastEpisodes"].is_null() {
            Some(&results["podcastEpisodes"])
        } else if !results["episodes"].is_null() {
            Some(&results["episodes"])
        } else {
            None
        };

        if let Some(eh) = episodes_hits {
            if search_type == "episode" || search_type.is_empty() {
                let items: Vec<Value> = eh["hits"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|e| {
                        let uri = e["uri"].as_str().unwrap_or("");
                        let images: Vec<Value> = if !e["image"].is_null() {
                            vec![json!({ "url": e["image"] })]
                        } else {
                            vec![]
                        };
                        json!({
                            "name": e["name"],
                            "id": uri2id(uri),
                            "uri": e["uri"],
                            "external_urls": { "spotify": uri2url(uri) },
                            "images": images,
                            "duration_ms": e["duration"],
                            "release_date": e["releaseDate"],
                            "explicit": e["explicit"].as_bool().unwrap_or(false),
                        })
                    })
                    .collect();
                out.insert("episodes".to_string(), json!({ "items": items }));
            }
        }

        if !results["audiobooks"].is_null()
            && (search_type == "audiobook" || search_type.is_empty())
        {
            let items: Vec<Value> = results["audiobooks"]["hits"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|b| {
                    let uri = b["uri"].as_str().unwrap_or("");
                    let images: Vec<Value> = if !b["image"].is_null() {
                        vec![json!({ "url": b["image"] })]
                    } else {
                        vec![]
                    };
                    let authors: Vec<Value> = b["authors"]
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .map(|a| json!({ "name": a }))
                        .collect();
                    let narrators: Vec<Value> = b["narrators"]
                        .as_array()
                        .unwrap_or(&vec![])
                        .iter()
                        .map(|n| json!({ "name": n }))
                        .collect();
                    json!({
                        "name": b["name"],
                        "id": uri2id(uri),
                        "uri": b["uri"],
                        "external_urls": { "spotify": uri2url(uri) },
                        "images": images,
                        "authors": authors,
                        "narrators": narrators,
                        "total_chapters": b["chapterCount"],
                    })
                })
                .collect();
            out.insert("audiobooks".to_string(), json!({ "items": items }));
        }

        let _ = empty_arr;

        Value::Object(out)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~' => out.push(byte as char),
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

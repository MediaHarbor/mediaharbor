use std::path::Path;

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use bytes::Bytes;
use dashmap::DashMap;
use hmac::Mac;
use reqwest::{header::HeaderMap, Client, ClientBuilder};
use serde_json::{json, Value};
use sha1::Sha1;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::errors::{MhError, MhResult};
use crate::http_client::UA_CHROME_LATEST;
use crate::services::common::ids::now_millis;

const TOTP_PERIOD: u64 = 30;
const TOTP_DIGITS: u32 = 6;

const SERVER_TIME_URL: &str = "https://open.spotify.com/api/server-time";
const SESSION_TOKEN_URL: &str = "https://open.spotify.com/api/token";
const CLIENT_TOKEN_URL: &str = "https://clienttoken.spotify.com/v1/clienttoken";
const PLAYBACK_INFO_PATH: &str = "/track-playback/v1/media/spotify:{mediaType}:{mediaId}";
const STORAGE_RESOLVE_PATH: &str =
    "/storage-resolve/v2/files/audio/interactive/{formatId}/{fileId}?version=10000000&product=9&platform=39&alt=json";
const PRODUCT_STATE_PATH: &str = "/melody/v1/product_state?market=from_token";
const WIDEVINE_LICENSE_PATH: &str = "/widevine-license/v1/audio/license";

const CLIENT_VERSION: &str = "1.2.70.61.g856ccd63";
const UA_ANDROID: &str = "Spotify/8.9.86.551 Android/34 (Google Pixel 8)";

const TRANSIENT_MAX_ATTEMPTS: u32 = 4;
const TRANSIENT_BACKOFF_CAP_MS: u64 = 30_000;

fn transient_backoff_ms(code: u16, attempt: u32, retry_after_secs: u64) -> u64 {
    let raw = if code == 429 {
        std::cmp::max(retry_after_secs.saturating_mul(1000), 1000u64 << attempt)
    } else {
        800 * (attempt as u64 + 1)
    };
    std::cmp::min(raw, TRANSIENT_BACKOFF_CAP_MS)
}

fn retry_after_secs(headers: &HeaderMap) -> u64 {
    headers
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

pub struct LibrespotService {
    client: Client,
    access_token: Option<String>,
    download_mode: String,
    token_expiry: u64,
    user_profile: Option<Value>,
    logged_in: bool,
    sp_dc: Option<String>,
    totp_secret: Option<Vec<u8>>,
    totp_version: Option<u32>,
    client_token: Option<String>,
    client_token_refresh_at: u64,
    client_id: Option<String>,
    pub wvd_path: Option<String>,
    key_cache: DashMap<String, String>,
    key_cache_path: Option<std::path::PathBuf>,
    license_limiter:
        Option<std::sync::Arc<crate::services::spotify::rate_limit::LicenseRateLimiter>>,
    license_clamp_medium: bool,
    license_retry_enabled: bool,
    emitter: Option<std::sync::Arc<dyn crate::EventEmitter>>,
}

#[derive(Debug, Clone, Default)]
pub struct SpotifyTrackDownloadMeta {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artist: String,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub total_tracks: Option<u32>,
    pub year: Option<String>,
    pub date: Option<String>,
    pub isrc: Option<String>,
    pub cover_url: Option<String>,
    /// Every artwork tier Spotify published, so the download path can honour the
    /// user's cover-size preference instead of always taking the largest.
    pub cover_group: Option<Value>,
    pub label: Option<String>,
    pub genre: Option<String>,
    pub disc_total: Option<u32>,
    pub upc: Option<String>,
    pub copyright: Option<String>,
    pub explicit: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SpotifyAlbumDownloadMeta {
    pub title: String,
    pub artist: String,
    pub year: Option<String>,
    pub date: Option<String>,
    pub cover_url: Option<String>,
    pub label: Option<String>,
    pub track_ids: Vec<String>,
}

fn cover_url_from_image_value(v: &Value) -> Option<String> {
    let fid = v.get("file_id").and_then(|f| f.as_str())?;
    let lower = fid.to_ascii_lowercase();
    let trimmed: String = lower.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if trimmed.len() >= 40 {
        return Some(format!("https://i.scdn.co/image/{}", &trimmed[..40]));
    }
    if let Ok(bytes) = B64.decode(fid) {
        if bytes.len() >= 20 {
            let hex: String = bytes.iter().take(20).map(|b| format!("{b:02x}")).collect();
            return Some(format!("https://i.scdn.co/image/{hex}"));
        }
    }
    None
}

fn pick_cover_url(group: &Value) -> Option<String> {
    pick_cover_url_sized(group, "extra-large")
}

/// Spotify publishes a track's artwork at a few fixed widths (64 / 300 / 640). The
/// requested tier picks the nearest one rather than always taking the largest.
pub(crate) fn pick_cover_url_sized(group: &Value, preference: &str) -> Option<String> {
    let images = group.get("image").and_then(|v| v.as_array())?;
    let rank = |img: &Value| -> u64 {
        img.get("width")
            .and_then(|s| s.as_u64())
            .unwrap_or_else(|| img.get("size").and_then(|s| s.as_u64()).unwrap_or(0))
    };
    let chosen = match preference.trim().to_ascii_lowercase().as_str() {
        "small" => images.iter().min_by_key(|i| rank(i)),
        "medium" => {
            let target = 300u64;
            images.iter().min_by_key(|i| rank(i).abs_diff(target))
        }
        "large" => {
            let target = 640u64;
            images.iter().min_by_key(|i| rank(i).abs_diff(target))
        }
        _ => images.iter().max_by_key(|i| rank(i)),
    };
    chosen
        .and_then(cover_url_from_image_value)
        .or_else(|| images.first().and_then(cover_url_from_image_value))
}

/// Spotify's `Date` carries year, month and day separately; taking only the year
/// meant a track could never be tagged with a full release date.
fn album_release_date(album: &Value) -> Option<String> {
    let date = album.get("date")?;
    let part = |key: &str| date.get(key).and_then(|v| v.as_u64()).filter(|n| *n > 0);
    let year = part("year")?;
    match (part("month"), part("day")) {
        (Some(m), Some(d)) => Some(format!("{year:04}-{m:02}-{d:02}")),
        (Some(m), None) => Some(format!("{year:04}-{m:02}")),
        _ => Some(year.to_string()),
    }
}

/// The `(P)`/`(C)` lines Spotify publishes on the album, joined the way the tag
/// itself reads them.
/// `{year}` in a folder template must stay four digits even when the tag carries a
/// full ISO date.
fn year_of(date: Option<&String>) -> Option<String> {
    date.and_then(|d| d.split('-').next())
        .filter(|y| y.len() == 4)
        .map(str::to_string)
}

fn album_copyright(album: &Value) -> Option<String> {
    let lines: Vec<String> = album
        .get("copyright")?
        .as_array()?
        .iter()
        .filter_map(|c| c.get("text").and_then(|v| v.as_str()))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines.join(" / "))
}

fn parse_common_meta(album: &Value) -> (String, Option<String>, Option<String>, Option<String>) {
    let title = album
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let date = album_release_date(album);
    let cover_url = album.get("cover_group").and_then(pick_cover_url);
    let label = album
        .get("label")
        .and_then(|v| v.as_str())
        .map(String::from);
    (title, date, cover_url, label)
}

/// `external_id` is a typed list on both tracks and albums — ISRC on one, UPC on
/// the other — so the lookup is the same either way.
fn external_id(meta: &Value, kind: &str) -> Option<String> {
    meta.get("external_id")?.as_array()?.iter().find_map(|e| {
        let t = e.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if t.eq_ignore_ascii_case(kind) {
            e.get("id").and_then(|v| v.as_str()).map(String::from)
        } else {
            None
        }
    })
}

fn isrc_from_external_id(meta: &Value) -> Option<String> {
    external_id(meta, "isrc")
}

pub(crate) fn parse_track_download_meta(track_id: &str, meta: &Value) -> SpotifyTrackDownloadMeta {
    let title = meta
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let artists: Vec<String> = meta
        .get("artist")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let album_obj = meta.get("album").cloned().unwrap_or(Value::Null);
    let (album_title, date, cover_url, label) = parse_common_meta(&album_obj);
    let year = year_of(date.as_ref());
    let album_artist = album_obj
        .get("artist")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|a| a.get("name").and_then(|v| v.as_str()))
        .map(String::from)
        .or_else(|| artists.first().cloned())
        .unwrap_or_default();
    let track_number = meta
        .get("number")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let disc_number = meta
        .get("disc_number")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let total_tracks = album_obj
        .get("disc")
        .and_then(|v| v.as_array())
        .map(|discs| {
            discs
                .iter()
                .map(|d| {
                    d.get("track")
                        .and_then(|t| t.as_array())
                        .map(|a| a.len() as u32)
                        .unwrap_or(0)
                })
                .sum::<u32>()
        })
        .filter(|n| *n > 0);
    let disc_total = album_obj
        .get("disc")
        .and_then(|v| v.as_array())
        .map(|discs| discs.len() as u32)
        .filter(|n| *n > 0);
    let genre = album_obj
        .get("genre")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|g| g.as_str())
        .map(String::from);
    SpotifyTrackDownloadMeta {
        id: track_id.to_string(),
        title,
        artists,
        album: album_title,
        album_artist,
        track_number,
        disc_number,
        total_tracks,
        year,
        date,
        isrc: isrc_from_external_id(meta),
        cover_url,
        cover_group: album_obj.get("cover_group").cloned(),
        label,
        genre,
        disc_total,
        upc: external_id(&album_obj, "upc"),
        copyright: album_copyright(&album_obj),
        explicit: meta
            .get("explicit")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

pub(crate) fn parse_album_download_meta(meta: &Value) -> SpotifyAlbumDownloadMeta {
    let (title, date, cover_url, label) = parse_common_meta(meta);
    let year = year_of(date.as_ref());
    let artist = meta
        .get("artist")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|a| a.get("name").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let mut track_ids: Vec<String> = Vec::new();
    if let Some(discs) = meta.get("disc").and_then(|v| v.as_array()) {
        for disc in discs {
            if let Some(tracks) = disc.get("track").and_then(|v| v.as_array()) {
                for t in tracks {
                    if let Some(uri) = t.get("uri").and_then(|v| v.as_str()) {
                        if let Some(id) = uri.rsplit(':').next() {
                            track_ids.push(id.to_string());
                            continue;
                        }
                    }
                    if let Some(gid) = t.get("gid").and_then(|v| v.as_str()) {
                        if let Some(id) = LibrespotService::_gid_to_id(gid) {
                            track_ids.push(id);
                        }
                    }
                }
            }
        }
    }
    SpotifyAlbumDownloadMeta {
        title,
        artist,
        year,
        date,
        cover_url,
        label,
        track_ids,
    }
}

impl Default for LibrespotService {
    fn default() -> Self {
        Self::new()
    }
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
            download_mode: String::new(),
            token_expiry: 0,
            user_profile: None,
            logged_in: false,
            sp_dc: None,
            totp_secret: None,
            totp_version: None,
            client_token: None,
            client_token_refresh_at: 0,
            client_id: None,
            wvd_path: None,
            key_cache: DashMap::new(),
            key_cache_path: None,
            license_limiter: None,
            license_clamp_medium: false,
            license_retry_enabled: true,
            emitter: None,
        }
    }

    pub fn set_emitter(&mut self, emitter: std::sync::Arc<dyn crate::EventEmitter>) {
        self.emitter = Some(emitter);
    }

    pub fn set_key_cache_path(&mut self, path: std::path::PathBuf) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(map) = serde_json::from_str::<serde_json::Map<String, Value>>(&text) {
                let now = now_millis() as u64;
                const TTL_MS: u64 = 24 * 3600 * 1000;
                for (file_id, entry) in map {
                    let key = entry.get("key").and_then(|v| v.as_str());
                    let at = entry.get("at").and_then(|v| v.as_u64()).unwrap_or(0);
                    if let Some(k) = key {
                        if now.saturating_sub(at) < TTL_MS {
                            self.key_cache.insert(file_id, k.to_string());
                        }
                    }
                }
            }
        }
        self.key_cache_path = Some(path);
    }

    fn persist_key_cache(&self) {
        let Some(path) = &self.key_cache_path else {
            return;
        };
        let now = now_millis() as u64;
        let mut map = serde_json::Map::new();
        for entry in self.key_cache.iter() {
            map.insert(
                entry.key().clone(),
                json!({ "key": entry.value(), "at": now }),
            );
        }
        if let Ok(text) = serde_json::to_string(&Value::Object(map)) {
            let _ = std::fs::write(path, text);
        }
    }

    fn log_line(&self, level: &str, message: String) {
        if let Some(em) = &self.emitter {
            em.emit_log(&crate::ipc_contract::BackendLogEvent::new(
                level, "spotify", "Spotify", message,
            ));
        }
    }

    pub fn set_license_limiter(
        &mut self,
        limiter: std::sync::Arc<crate::services::spotify::rate_limit::LicenseRateLimiter>,
    ) {
        self.license_limiter = Some(limiter);
    }

    pub fn debug_set_license_retry(&mut self, on: bool) {
        self.license_retry_enabled = on;
    }

    pub fn debug_clear_key_cache(&self) {
        self.key_cache.clear();
    }

    pub async fn debug_raw_storage_resolve(
        &mut self,
        file_id: &str,
        format_id: u32,
    ) -> MhResult<(u16, Vec<(String, String)>, u64)> {
        let token = self._get_valid_token().await?;
        let url = format!(
            "{}{}",
            crate::services::spotify::endpoints::spclient_base(&self.client).await,
            STORAGE_RESOLVE_PATH
        )
        .replace("{formatId}", &format_id.to_string())
        .replace("{fileId}", file_id);
        let client_token = self.client_token.clone();

        let mut req = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .header("app-platform", "Android")
            .header("user-agent", UA_ANDROID);
        if let Some(ct) = &client_token {
            req = req.header("client-token", ct.clone());
        }

        let resp = req.send().await.map_err(MhError::Network)?;
        let status = resp.status();
        let retry_after = retry_after_secs(resp.headers());
        let headers = resp
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        Ok((status.as_u16(), headers, retry_after))
    }

    /// The track's Widevine PSSH plus the two credentials a license POST needs.
    async fn _pssh_and_creds(&mut self, file_id: &str) -> MhResult<(String, String, String)> {
        let seek_resp = self
            .client
            .get(format!(
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

        Ok((pssh, access_token, client_token))
    }

    pub async fn debug_raw_license_post(
        &mut self,
        file_id: &str,
        venv_python: &std::path::Path,
    ) -> MhResult<(u16, Vec<(String, String)>, u64)> {
        let (pssh, access_token, client_token) = self._pssh_and_creds(file_id).await?;

        let challenge_bytes = self
            ._generate_widevine_challenge(&pssh, venv_python)
            .await?;

        let license_resp = self
            .client
            .post(format!(
                "{}{}",
                crate::services::spotify::endpoints::spclient_base(&self.client).await,
                WIDEVINE_LICENSE_PATH
            ))
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

        let status = license_resp.status();
        let retry_after = retry_after_secs(license_resp.headers());
        let headers = license_resp
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        Ok((status.as_u16(), headers, retry_after))
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
                self.log_line("warn", format!("Failed to fetch Spotify profile: {e}"));
                Some(json!({ "name": "Spotify User" }))
            }
        };

        self.logged_in = true;
        Ok(self.user_profile.clone().unwrap_or(json!({})))
    }

    pub fn is_logged_in(&self) -> bool {
        self.logged_in && self.access_token.is_some()
    }

    pub async fn probe_session(&mut self) -> MhResult<bool> {
        if !self.logged_in {
            return Ok(false);
        }
        let token = match self._get_valid_token().await {
            Ok(t) => t,
            Err(MhError::Auth(_)) => return Ok(false),
            Err(e) => return Err(e),
        };
        let resp = self
            .client
            .get("https://api.spotify.com/v1/me")
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(MhError::Network)?;
        match resp.status().as_u16() {
            401 | 403 => Ok(false),
            s if (200..300).contains(&s) => Ok(true),
            s => Err(MhError::Other(format!("Spotify /v1/me probe HTTP {s}"))),
        }
    }

    pub fn cached_profile(&self) -> Option<Value> {
        self.user_profile.clone()
    }

    fn is_known_free(&self) -> bool {
        self.user_profile
            .as_ref()
            .and_then(|p| p.get("plan"))
            .and_then(|v| v.as_str())
            .map(|plan| plan.eq_ignore_ascii_case("free") || plan.eq_ignore_ascii_case("open"))
            .unwrap_or(false)
    }

    fn account_max_quality(&self) -> &'static str {
        if self.is_known_free() || self.license_clamp_medium {
            "aac-medium"
        } else {
            "aac-high"
        }
    }

    fn _effective_quality<'a>(requested: Option<&'a str>, max: &'a str) -> &'a str {
        match requested {
            Some("aac-high") if max != "aac-high" => max,
            Some(q) => q,
            None => max,
        }
    }

    pub fn cached_access_token(&self) -> Option<String> {
        self.access_token.clone()
    }

    pub fn cached_client_token(&self) -> Option<String> {
        self.client_token.clone()
    }

    pub async fn force_refresh_client_token(&mut self) -> MhResult<Option<String>> {
        self.client_token = None;
        self._acquire_client_token().await?;
        Ok(self.client_token.clone())
    }

    pub async fn ensure_tokens(&mut self) -> MhResult<()> {
        if !self.logged_in {
            return Ok(());
        }
        self._get_valid_token().await.map(|_| ())
    }

    pub fn logout(&mut self) {
        self.access_token = None;
        self.token_expiry = 0;
        self.user_profile = None;
        self.logged_in = false;
        self.sp_dc = None;
        self.license_clamp_medium = false;
    }

    pub async fn get_track_info(&mut self, track_id: &str) -> MhResult<Value> {
        if !self.is_logged_in() {
            return Err(MhError::Auth("Spotify account not connected".into()));
        }

        let token = self._get_valid_token().await?;
        let vars = serde_json::to_string(&json!({ "uri": format!("spotify:track:{track_id}") }))?;
        let ext = serde_json::to_string(&json!({
            "persistedQuery": {
                "version": 1,
                "sha256Hash": "ae85b52abb74d20a4c331d4143d4772c95f34757bfa8c625474b912b9055b5c0"
            }
        }))?;

        let url = format!(
            "https://api-partner.spotify.com/pathfinder/v1/query?operationName=getTrack&variables={}&extensions={}",
            crate::services::common::http::pct_encode(&vars),
            crate::services::common::http::pct_encode(&ext)
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

        let data: Value = crate::services::common::http::read_json("Spotify", resp).await?;
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
        let already_clamped = self.is_known_free() || self.license_clamp_medium;
        match self
            ._stream_media(track_id, "track", venv_python, None, None)
            .await
        {
            Err(MhError::Forbidden(_)) if !already_clamped => {
                self.license_clamp_medium = true;
                self._stream_media(track_id, "track", venv_python, Some("aac-medium"), None)
                    .await
            }
            other => other,
        }
    }

    pub async fn get_track_stream_for_download(
        &mut self,
        track_id: &str,
        preferred_quality: &str,
        venv_python: Option<&Path>,
        progress: Option<&crate::downloads::ByteProgress>,
        download_mode: &str,
    ) -> MhResult<(Bytes, String)> {
        self.download_mode = download_mode.to_string();
        if !self.is_logged_in() {
            return Err(MhError::Auth("Spotify account not connected".into()));
        }
        let already_clamped = self.is_known_free() || self.license_clamp_medium;
        match self
            ._stream_media(
                track_id,
                "track",
                venv_python,
                Some(preferred_quality),
                progress,
            )
            .await
        {
            Err(MhError::Forbidden(_)) if !already_clamped && preferred_quality != "aac-medium" => {
                self.license_clamp_medium = true;
                self._stream_media(track_id, "track", venv_python, Some("aac-medium"), progress)
                    .await
            }
            other => other,
        }
    }

    pub async fn get_track_download_metadata(
        &mut self,
        track_id: &str,
    ) -> MhResult<SpotifyTrackDownloadMeta> {
        let meta = self._get_gid_metadata(track_id, "track").await?;
        Ok(parse_track_download_meta(track_id, &meta))
    }

    pub async fn get_album_download_metadata(
        &mut self,
        album_id: &str,
    ) -> MhResult<SpotifyAlbumDownloadMeta> {
        let meta = self._get_gid_metadata(album_id, "album").await?;
        Ok(parse_album_download_meta(&meta))
    }

    pub async fn get_podcast_episode_stream(
        &mut self,
        episode_id: &str,
    ) -> MhResult<(Bytes, String)> {
        if !self.is_logged_in() {
            return Err(MhError::Auth("Spotify account not connected".into()));
        }
        self._stream_media(episode_id, "episode", None, None, None)
            .await
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

        use crate::services::spotify::{totp_secrets, web_player};
        let secret = match web_player::cached_or_scrape(&self.client).await {
            Ok(assets) => match assets.totp {
                Some(secret) => secret,
                None => totp_secrets::from_mirror_only(&self.client).await?,
            },
            Err(e) => {
                self.log_line(
                    "warn",
                    format!("Spotify web player scrape failed ({e}); trying the secret mirror"),
                );
                totp_secrets::from_mirror_only(&self.client).await?
            }
        };
        self.log_line(
            "debug",
            format!("Spotify TOTP secret v{} loaded", secret.version),
        );
        self.totp_secret = Some(secret.hmac_key());
        self.totp_version = Some(secret.version);
        Ok(())
    }

    fn _generate_totp(&self, timestamp_sec: u64) -> String {
        let secret = self
            .totp_secret
            .as_deref()
            .expect("TOTP secret not initialised");

        let counter = timestamp_sec / TOTP_PERIOD;
        let counter_bytes = counter.to_be_bytes();

        let mut mac = <hmac::Hmac<Sha1> as hmac::KeyInit>::new_from_slice(secret)
            .expect("HMAC-SHA1 accepts any key length");
        mac.update(&counter_bytes);
        let result = mac.finalize().into_bytes();

        let offset = (result[19] & 0x0f) as usize;
        let binary = (((result[offset] & 0x7f) as u32) << 24)
            | ((result[offset + 1] as u32) << 16)
            | ((result[offset + 2] as u32) << 8)
            | (result[offset + 3] as u32);

        let modulus = 10_u32.pow(TOTP_DIGITS);
        format!(
            "{:0>width$}",
            binary % modulus,
            width = TOTP_DIGITS as usize
        )
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
            return Err(crate::services::common::http::auth_error("Spotify", resp).await);
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
                .map(|(k, v)| format!(
                    "{}={}",
                    k,
                    url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
                ))
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
                status, text
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
            .unwrap_or_else(|| (now_millis() as u64) + 3_600_000);

        self._acquire_client_token().await?;
        Ok(())
    }

    async fn _get_valid_token(&mut self) -> MhResult<String> {
        if (now_millis() as u64) >= self.token_expiry.saturating_sub(60_000) {
            self._refresh_token().await?;
        }
        self.ensure_client_token().await?;
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
            self.log_line(
                "warn",
                format!("Client token acquisition failed: {}", resp.status()),
            );
            return Ok(());
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        self.client_token = data["granted_token"]["token"].as_str().map(String::from);
        let refresh_after = data["granted_token"]["refresh_after_seconds"]
            .as_u64()
            .filter(|s| *s > 0)
            .unwrap_or(2 * 3600);
        self.client_token_refresh_at = (now_millis() as u64) + refresh_after.saturating_mul(1000);
        Ok(())
    }

    async fn ensure_client_token(&mut self) -> MhResult<()> {
        if self.client_token.is_none() || (now_millis() as u64) >= self.client_token_refresh_at {
            self._acquire_client_token().await?;
        }
        Ok(())
    }

    async fn _fetch_profile(&mut self) -> MhResult<Value> {
        let plan = match self._fetch_product_state().await {
            Ok(p) => Value::String(p),
            Err(e) => {
                self.log_line(
                    "warn",
                    format!("Spotify product-state fetch failed: {e}; account tier unknown"),
                );
                Value::Null
            }
        };

        let token = self._get_valid_token().await?;
        let me: Value = match self
            .client
            .get("https://api.spotify.com/v1/me")
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => resp.json().await.unwrap_or_default(),
            Ok(resp) => {
                self.log_line(
                    "warn",
                    format!(
                        "Spotify /v1/me returned {} (name/email best-effort)",
                        resp.status()
                    ),
                );
                Value::Null
            }
            Err(e) => {
                self.log_line(
                    "warn",
                    format!("Spotify /v1/me request failed: {e} (name/email best-effort)"),
                );
                Value::Null
            }
        };

        let name = me["display_name"]
            .as_str()
            .or_else(|| me["id"].as_str())
            .unwrap_or("Spotify User")
            .to_string();

        Ok(json!({
            "name": name,
            "plan": plan,
            "email": me["email"],
            "id": me["id"],
        }))
    }

    async fn _fetch_product_state(&mut self) -> MhResult<String> {
        let token = self._get_valid_token().await?;
        let mut req = self
            .client
            .get(format!(
                "{}{}",
                crate::services::spotify::endpoints::spclient_base(&self.client).await,
                PRODUCT_STATE_PATH
            ))
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .header("app-platform", "WebPlayer")
            .header("spotify-app-version", CLIENT_VERSION)
            .header("User-Agent", UA_CHROME_LATEST);
        if let Some(ct) = &self.client_token {
            req = req.header("client-token", ct.clone());
        }

        let resp = req.send().await.map_err(MhError::Network)?;
        if !resp.status().is_success() {
            return Err(crate::services::common::http::auth_error("Spotify", resp).await);
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        data["product"]
            .as_str()
            .or_else(|| data["catalogue"].as_str())
            .map(String::from)
            .ok_or_else(|| MhError::Auth(format!("Product-state missing tier: {data}")))
    }

    fn _auth_headers(&self) -> Vec<(String, String)> {
        let token = self.access_token.as_deref().unwrap_or("");
        let mut h = vec![
            ("Authorization".to_string(), format!("Bearer {token}")),
            ("Accept".to_string(), "application/json".to_string()),
            ("user-agent".to_string(), UA_CHROME_LATEST.to_string()),
            ("app-platform".to_string(), "WebPlayer".to_string()),
            (
                "spotify-app-version".to_string(),
                CLIENT_VERSION.to_string(),
            ),
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
        const CHARSET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

        let mut n: u128 = 0;
        for c in id.bytes() {
            let pos = CHARSET.iter().position(|&b| b == c).unwrap_or(0) as u128;
            n = n.wrapping_mul(62).wrapping_add(pos);
        }
        format!("{:0>32x}", n)
    }

    pub(crate) fn _gid_to_id(gid: &str) -> Option<String> {
        const CHARSET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let lower = gid.to_ascii_lowercase();
        let hex: String = lower.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        let mut n = u128::from_str_radix(&hex, 16).ok()?;
        let mut out = vec![b'0'; 22];
        for slot in out.iter_mut().rev() {
            *slot = CHARSET[(n % 62) as usize];
            n /= 62;
        }
        String::from_utf8(out).ok()
    }

    fn _format_id_for_quality(quality: &str) -> Option<&'static str> {
        match quality {
            "vorbis-high" => Some("OGG_VORBIS_320"),
            "vorbis-medium" => Some("OGG_VORBIS_160"),
            "vorbis-low" => Some("OGG_VORBIS_96"),
            "mp3-320" => Some("MP3_320"),
            "mp3-256" => Some("MP3_256"),
            "mp3-160" => Some("MP3_160"),
            "mp3-128" => Some("MP3_128"),
            "mp3-96" => Some("MP3_96"),
            _ => None,
        }
    }

    fn _pb_varint(n: u64, out: &mut Vec<u8>) {
        let mut v = n;
        while v >= 0x80 {
            out.push(((v as u8) & 0x7F) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    fn _pb_read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
        let mut result: u64 = 0;
        let mut shift = 0u32;
        while *pos < buf.len() {
            let b = buf[*pos];
            *pos += 1;
            result |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
        None
    }

    fn _pb_emit_lengthed(out: &mut Vec<u8>, field_no: u32, payload: &[u8]) {
        let tag = (field_no << 3) | 2;
        Self::_pb_varint(tag as u64, out);
        Self::_pb_varint(payload.len() as u64, out);
        out.extend_from_slice(payload);
    }

    fn _pb_emit_string(out: &mut Vec<u8>, field_no: u32, s: &str) {
        Self::_pb_emit_lengthed(out, field_no, s.as_bytes());
    }

    fn _encode_device(client_id: &str, device_id: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        Self::_pb_emit_string(&mut out, 1, client_id);
        Self::_pb_emit_string(&mut out, 2, device_id);
        out
    }

    fn _encode_locale(language_tag: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(16);
        Self::_pb_emit_string(&mut out, 1, language_tag);
        out
    }

    fn _encode_client(device: &[u8], locale: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(device.len() + locale.len() + 8);
        Self::_pb_emit_lengthed(&mut out, 1, device);
        Self::_pb_emit_lengthed(&mut out, 2, locale);
        out
    }

    fn _encode_audio_manifest_request(
        format_id: &str,
        media_id: &[u8],
        client_id: &str,
        device_id: &str,
        language_tag: &str,
    ) -> Vec<u8> {
        let mut out = Vec::with_capacity(128);
        Self::_pb_emit_string(&mut out, 1, format_id);
        Self::_pb_emit_lengthed(&mut out, 2, media_id);
        let device = Self::_encode_device(client_id, device_id);
        let locale = Self::_encode_locale(language_tag);
        let client = Self::_encode_client(&device, &locale);
        Self::_pb_emit_lengthed(&mut out, 3, &client);
        out
    }

    fn _pb_find_url(buf: &[u8]) -> Option<String> {
        let mut pos = 0;
        while pos < buf.len() {
            let tag = Self::_pb_read_varint(buf, &mut pos)?;
            let field_no = (tag >> 3) as u32;
            let wire = (tag & 0x7) as u8;
            if wire != 2 {
                Self::_pb_skip(buf, &mut pos, wire)?;
                continue;
            }
            let len = Self::_pb_read_varint(buf, &mut pos)?;
            let end = pos + len as usize;
            if end > buf.len() {
                return None;
            }
            let inner = &buf[pos..end];
            pos = end;
            if field_no == 1 {
                if let Some(s) = Self::_pb_first_string_at_field(inner, 1) {
                    return Some(s);
                }
            }
        }
        None
    }

    fn _pb_first_string_at_field(buf: &[u8], target: u32) -> Option<String> {
        let mut pos = 0;
        while pos < buf.len() {
            let tag = Self::_pb_read_varint(buf, &mut pos)?;
            let field_no = (tag >> 3) as u32;
            let wire = (tag & 0x7) as u8;
            if wire == 2 {
                let len = Self::_pb_read_varint(buf, &mut pos)?;
                let end = pos + len as usize;
                if end > buf.len() {
                    return None;
                }
                if field_no == target {
                    return std::str::from_utf8(&buf[pos..end]).ok().map(String::from);
                }
                pos = end;
            } else {
                Self::_pb_skip(buf, &mut pos, wire)?;
            }
        }
        None
    }

    fn _pb_find_first_string(buf: &[u8]) -> Option<String> {
        Self::_pb_first_string_at_field(buf, 1)
    }

    fn _decode_request_error(buf: &[u8]) -> String {
        let mut pos = 0;
        while pos < buf.len() {
            let Some(tag) = Self::_pb_read_varint(buf, &mut pos) else {
                break;
            };
            let field_no = (tag >> 3) as u32;
            let wire = (tag & 0x7) as u8;
            if wire != 2 {
                let _ = Self::_pb_skip(buf, &mut pos, wire);
                continue;
            }
            let Some(len) = Self::_pb_read_varint(buf, &mut pos) else {
                break;
            };
            let end = pos + len as usize;
            if end > buf.len() {
                break;
            }
            let inner = &buf[pos..end];
            pos = end;
            let (label, has_reason) = match field_no {
                1 => ("not_found", false),
                2 => ("disallowed", false),
                3 => ("unplayable", false),
                4 => ("bad_request", true),
                5 => ("request_timeout", false),
                6 => ("permanent_error", true),
                7 => ("temporary_error", true),
                _ => continue,
            };
            return if has_reason {
                match Self::_pb_first_string_at_field(inner, 1) {
                    Some(r) if !r.is_empty() => format!("{label}: {r}"),
                    _ => label.to_string(),
                }
            } else {
                label.to_string()
            };
        }
        "unknown".to_string()
    }

    fn _pb_skip(buf: &[u8], pos: &mut usize, wire: u8) -> Option<()> {
        match wire {
            0 => {
                Self::_pb_read_varint(buf, pos)?;
                Some(())
            }
            1 => {
                *pos += 8;
                if *pos <= buf.len() {
                    Some(())
                } else {
                    None
                }
            }
            2 => {
                let len = Self::_pb_read_varint(buf, pos)? as usize;
                *pos += len;
                if *pos <= buf.len() {
                    Some(())
                } else {
                    None
                }
            }
            5 => {
                *pos += 4;
                if *pos <= buf.len() {
                    Some(())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn _hex_gid_to_bytes(hex: &str) -> Option<[u8; 16]> {
        if hex.len() != 32 {
            return None;
        }
        let mut out = [0u8; 16];
        for i in 0..16 {
            out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(out)
    }

    fn _unframe_grpc_web(buf: &[u8]) -> MhResult<Vec<u8>> {
        if buf.len() < 5 {
            return Err(MhError::Other("gRPC-Web response too short".into()));
        }
        let mut pos = 0;
        let mut message: Vec<u8> = Vec::new();
        let mut trailers: String = String::new();
        while pos + 5 <= buf.len() {
            let flag = buf[pos];
            let len = u32::from_be_bytes([buf[pos + 1], buf[pos + 2], buf[pos + 3], buf[pos + 4]])
                as usize;
            pos += 5;
            if pos + len > buf.len() {
                return Err(MhError::Other(format!(
                    "gRPC-Web frame overflow: flag={flag:#x} len={len} remaining={}",
                    buf.len() - pos
                )));
            }
            let frame = &buf[pos..pos + len];
            pos += len;
            if flag & 0x80 != 0 {
                trailers = String::from_utf8_lossy(frame).to_string();
            } else {
                message = frame.to_vec();
            }
        }
        let mut status_code: Option<u32> = None;
        let mut status_msg: Option<String> = None;
        for line in trailers.lines() {
            if let Some(rest) = line.strip_prefix("grpc-status:") {
                status_code = rest.trim().parse().ok();
            } else if let Some(rest) = line.strip_prefix("grpc-message:") {
                status_msg = Some(rest.trim().to_string());
            }
        }
        match status_code {
            Some(0) | None => Ok(message),
            Some(code) => Err(MhError::Other(format!(
                "gRPC-Web status={code} ({})",
                status_msg.unwrap_or_default()
            ))),
        }
    }

    async fn _get_playback_info(&mut self, media_type: &str, media_id: &str) -> MhResult<Value> {
        let base_url = format!(
            "{}{}",
            crate::services::spotify::endpoints::spclient_base(&self.client).await,
            PLAYBACK_INFO_PATH
        )
        .replace("{mediaType}", media_type)
        .replace("{mediaId}", media_id);

        let formats_to_try: &[&str] = if media_type == "episode" || media_type == "chapter" {
            &["file_ids_mp4", "file_ids_mp4_dual", "file_ids_ogg"]
        } else {
            &["file_ids_ogg", "file_ids_mp4"]
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
                    502..=504 if attempt < 2 => {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            800 * (attempt as u64 + 1),
                        ))
                        .await;
                        continue;
                    }
                    400 => break,
                    _ => {
                        let text = resp.text().await.unwrap_or_default();
                        return Err(MhError::Other(format!(
                            "Playback info failed: {} — {}",
                            status, text
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
                status, text
            )));
        }
        resp.json().await.map_err(MhError::Network)
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

    async fn _resolve_storage_for_format(
        &mut self,
        file_id: &str,
        format_id: u32,
    ) -> MhResult<String> {
        let token = self._get_valid_token().await?;
        let url = format!(
            "{}{}",
            crate::services::spotify::endpoints::spclient_base(&self.client).await,
            STORAGE_RESOLVE_PATH
        )
        .replace("{formatId}", &format_id.to_string())
        .replace("{fileId}", file_id);
        let client_token = self.client_token.clone();

        let mut attempt = 0u32;
        let data: Value = loop {
            let mut req = self
                .client
                .get(&url)
                .header("Authorization", format!("Bearer {token}"))
                .header("Accept", "application/json")
                .header("app-platform", "Android")
                .header("user-agent", UA_ANDROID);
            if let Some(ct) = &client_token {
                req = req.header("client-token", ct.clone());
            }

            let resp = req.send().await.map_err(MhError::Network)?;
            let status = resp.status();
            if status.is_success() {
                break resp.json().await.map_err(MhError::Network)?;
            }

            let code = status.as_u16();
            let ra = retry_after_secs(resp.headers());
            if code == 429 {
                if let Some(l) = &self.license_limiter {
                    l.on_rate_limited(ra);
                }
            }
            let retryable = (502..=504).contains(&code) || (code == 429 && ra > 0);
            if attempt + 1 < TRANSIENT_MAX_ATTEMPTS && retryable {
                let wait_ms = transient_backoff_ms(code, attempt, ra);
                self.log_line(
                    "warn",
                    format!(
                        "Storage-resolve {code}; retrying in {wait_ms}ms (attempt {}/{TRANSIENT_MAX_ATTEMPTS})",
                        attempt + 1
                    ),
                );
                tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                attempt += 1;
                continue;
            }

            if code == 404 {
                return Err(MhError::Unsupported(format!(
                    "Storage resolve 404 for format {format_id} — account not entitled"
                )));
            }
            if code == 429 {
                return Err(MhError::RateLimited(format!(
                    "Storage resolve rate-limited: {status}"
                )));
            }
            return Err(MhError::Other(format!("Storage resolve failed: {status}")));
        };

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

    fn _parse_protobuf_field(buf: &[u8], target_field: u32) -> Option<&[u8]> {
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

    async fn _get_gid_metadata(&mut self, track_id: &str, media_type: &str) -> MhResult<Value> {
        let gid = Self::_id_to_gid(track_id);

        for attempt in 0..2 {
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

            let base = format!("https://spclient.wg.spotify.com/metadata/4/{media_type}/{gid}");
            let mut last_status = None;
            let mut last_body = String::new();

            for url in [format!("{base}?market=from_token"), base.clone()] {
                let mut req = self.client.get(&url);
                for (k, v) in &headers_vec {
                    req = req.header(k.as_str(), v.as_str());
                }
                let resp = req.send().await.map_err(MhError::Network)?;
                let status = resp.status();
                if status.is_success() {
                    return resp.json().await.map_err(MhError::Network);
                }
                if status.as_u16() == 404 {
                    continue;
                }
                last_status = Some(status);
                last_body = resp.text().await.unwrap_or_default();
                break;
            }

            match last_status {
                None => {
                    return Err(MhError::NotFound(format!(
                        "GID metadata not found for {media_type} {track_id}"
                    )));
                }
                Some(status) => {
                    let refreshable = status.as_u16() == 401
                        || status.as_u16() == 403
                        || status.is_server_error();
                    if attempt == 0 && refreshable {
                        self._acquire_client_token().await?;
                        continue;
                    }
                    return Err(MhError::Other(format!(
                        "GID metadata failed: {} — {}",
                        status, last_body
                    )));
                }
            }
        }

        Err(MhError::NotFound(format!(
            "GID metadata not found for {media_type} {track_id}"
        )))
    }

    fn is_audio_format(fmt: &str) -> bool {
        fmt.starts_with("OGG_VORBIS")
            || fmt.starts_with("MP3_")
            || fmt.starts_with("AAC_")
            || matches!(fmt, "MP4_128" | "MP4_256" | "MP4_128_DUAL" | "MP4_256_DUAL")
    }

    fn _select_ogg_file<'a>(&self, files: &'a [Value]) -> Option<&'a Value> {
        let priority = ["OGG_VORBIS_320", "OGG_VORBIS_160", "OGG_VORBIS_96"];
        for fmt in &priority {
            if let Some(f) = files.iter().find(|f| f["format"].as_str() == Some(fmt)) {
                return Some(f);
            }
        }
        files.iter().find(|f| {
            f["format"]
                .as_str()
                .is_some_and(|s| s.starts_with("OGG_VORBIS"))
        })
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
        None
    }

    fn _aac_ceiling_kbps_for_quality(quality: &str) -> u32 {
        match quality {
            "aac-high" => 256,
            "aac-medium" => 128,
            "vorbis-high" | "mp3-320" | "mp3-256" => 256,
            "vorbis-medium" | "vorbis-low" | "mp3-160" | "mp3-128" | "mp3-96" => 128,
            _ => u32::MAX,
        }
    }

    fn _select_mp4_for_ceiling(entries: &[Value], kbps_ceiling: u32) -> Option<&Value> {
        let bitrate_of = |f: &Value| -> u32 {
            f.get("bitrate")
                .and_then(|v| v.as_u64())
                .map(|b| (b / 1000) as u32)
                .or_else(|| match f.get("format").and_then(|v| v.as_str()) {
                    Some("11") | Some("13") => Some(256),
                    Some("10") | Some("12") => Some(128),
                    _ => None,
                })
                .unwrap_or(0)
        };
        let in_ceiling: Vec<&Value> = entries
            .iter()
            .filter(|f| bitrate_of(f) <= kbps_ceiling)
            .collect();
        if let Some(best) = in_ceiling.iter().max_by_key(|f| bitrate_of(f)) {
            return Some(*best);
        }
        entries.iter().min_by_key(|f| bitrate_of(f))
    }

    fn _format_ladder_for_quality(quality: &str) -> Option<&'static [&'static str]> {
        match quality {
            "vorbis-high" => Some(&["OGG_VORBIS_320", "OGG_VORBIS_160", "OGG_VORBIS_96"]),
            "vorbis-medium" => Some(&["OGG_VORBIS_160", "OGG_VORBIS_96"]),
            "vorbis-low" => Some(&["OGG_VORBIS_96"]),
            "mp3-320" => Some(&["MP3_320", "MP3_256", "MP3_160", "MP3_128", "MP3_96"]),
            "mp3-256" => Some(&["MP3_256", "MP3_160", "MP3_128", "MP3_96"]),
            "mp3-160" => Some(&["MP3_160", "MP3_128", "MP3_96"]),
            "mp3-128" => Some(&["MP3_128", "MP3_96"]),
            "mp3-96" => Some(&["MP3_96"]),
            "aac-high" => Some(&["MP4_256", "MP4_128", "AAC_48", "AAC_24"]),
            "aac-medium" => Some(&["MP4_128", "AAC_48", "AAC_24"]),
            _ => None,
        }
    }

    fn _select_audio_file_preferred<'a>(
        &self,
        files: &'a [Value],
        preferred: &str,
    ) -> Option<(&'a Value, bool)> {
        let Some(ladder) = Self::_format_ladder_for_quality(preferred) else {
            return self._select_audio_file(files);
        };
        for fmt in ladder {
            if let Some(f) = files.iter().find(|f| f["format"].as_str() == Some(*fmt)) {
                let is_ogg = fmt.starts_with("OGG_VORBIS");
                return Some((f, is_ogg));
            }
        }
        None
    }

    fn _extract_feed_url(data: &Value) -> Option<String> {
        let d = if !data["show"].is_null() {
            &data["show"]
        } else {
            data
        };
        for field in &[
            "rssFeedUrl",
            "rss_url",
            "feed_url",
            "feedUrl",
            "rssUrl",
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
            format!("https://spclient.wg.spotify.com/podcast-experience/v2/episodes/{episode_id}"),
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
                format!("https://spclient.wg.spotify.com/podcast-experience/v2/shows/{sid}"),
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
            let base = format!("https://spclient.wg.spotify.com/metadata/4/podcast/{show_gid}");
            for url in [format!("{base}?market=from_token"), base] {
                let data = try_fetch(&self.client, url, hdr_pairs.clone()).await;
                if let Some(ref d) = data {
                    if let Some(feed_url) = Self::_extract_feed_url(d) {
                        if let Some(audio_url) = self
                            ._find_episode_in_rss(&feed_url, episode_id, episode_name, duration_ms)
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
                    let name = e.name().into_inner().to_lowercase();
                    if name == "item" {
                        in_item = true;
                        current_audio_url = None;
                        current_title = None;
                        current_guid = None;
                        current_duration = None;
                    }
                }
                Ok(Event::Empty(ref e)) if in_item => {
                    let name = e.name().into_inner().to_lowercase();
                    if name == "enclosure" {
                        for attr in e.attributes().flatten() {
                            let key = attr.key.into_inner().to_lowercase();
                            if key == "url" {
                                current_audio_url = Some(attr.value.into_owned());
                            }
                        }
                    }
                }
                Ok(Event::Text(ref t)) if in_item => {
                    let _ = t;
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name().into_inner().to_lowercase();
                    if name == "item" {
                        in_item = false;
                        if let Some(ref audio_url) = current_audio_url.clone() {
                            if current_guid.as_deref() == Some(&spotify_uri) {
                                return Some(audio_url.clone());
                            }
                            if let (Some(ep_name), Some(ref title)) = (episode_name, &current_title)
                            {
                                if title.to_lowercase().trim() == ep_name.to_lowercase().trim() {
                                    return Some(audio_url.clone());
                                }
                                if ep_name.len() >= 20
                                    && title.to_lowercase().contains(&ep_name.to_lowercase()[..20])
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
        let mut cur_text = String::new();

        let mut reader = quick_xml::Reader::from_str(xml);

        loop {
            use quick_xml::events::Event;
            match reader.read_event() {
                Ok(Event::Start(ref e)) => {
                    let raw = e.name().into_inner();
                    let name = raw.to_lowercase();
                    if name == "item" {
                        in_item = true;
                        cur_url = None;
                        cur_title = None;
                        cur_guid = None;
                        cur_dur = None;
                        cur_tag.clear();
                        cur_text.clear();
                    } else if in_item {
                        if matches!(name.as_str(), "title" | "guid" | "itunes:duration") {
                            cur_tag = name.clone();
                            cur_text.clear();
                        }
                        if name == "enclosure" {
                            for attr in e.attributes().flatten() {
                                let key = attr.key.into_inner().to_lowercase();
                                if key == "url" {
                                    cur_url = Some(attr.value.into_owned());
                                }
                            }
                        }
                    }
                }
                Ok(Event::Empty(ref e)) if in_item => {
                    let name = e.name().into_inner().to_lowercase();
                    if name == "enclosure" {
                        for attr in e.attributes().flatten() {
                            let key = attr.key.into_inner().to_lowercase();
                            if key == "url" {
                                cur_url = Some(attr.value.into_owned());
                            }
                        }
                    }
                }
                Ok(Event::Text(ref t)) if in_item && !cur_tag.is_empty() => {
                    cur_text.push_str(&t.xml10_content());
                }
                Ok(Event::GeneralRef(ref r)) if in_item && !cur_tag.is_empty() => {
                    if let Ok(Some(ch)) = r.resolve_char_ref() {
                        cur_text.push(ch);
                    } else if let Some(s) = quick_xml::escape::resolve_predefined_entity(r) {
                        cur_text.push_str(s);
                    } else {
                        cur_text.push('&');
                        cur_text.push_str(r);
                        cur_text.push(';');
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name().into_inner().to_lowercase();
                    if name == "item" {
                        in_item = false;
                        if let Some(ref audio_url) = cur_url {
                            if cur_guid.as_deref() == Some(&spotify_uri) {
                                return Some(audio_url.clone());
                            }
                            if let (Some(ep_name), Some(ref title)) = (episode_name, &cur_title) {
                                if title.to_lowercase().trim() == ep_name.to_lowercase().trim() {
                                    return Some(audio_url.clone());
                                }
                                if ep_name.len() >= 20
                                    && title.to_lowercase().contains(&ep_name.to_lowercase()[..20])
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
                        cur_text.clear();
                    } else if name == cur_tag {
                        let txt = cur_text.trim().to_string();
                        match cur_tag.as_str() {
                            "title" => cur_title = Some(txt),
                            "guid" => cur_guid = Some(txt),
                            "itunes:duration" => cur_dur = Some(txt),
                            _ => {}
                        }
                        cur_tag.clear();
                        cur_text.clear();
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

    /// `ffmpeg` streams straight from the CDN URL; anything else fetches the file
    /// first so byte progress is real.
    fn stream_direct_from_cdn(&self) -> bool {
        self.download_mode.eq_ignore_ascii_case("ffmpeg")
    }

    async fn _stream_mp4_with_widevine(
        &mut self,
        cdn_url: &str,
        file_id: &str,
        venv_python: Option<&Path>,
        progress: Option<&crate::downloads::ByteProgress>,
    ) -> MhResult<Bytes> {
        let venv_python = venv_python
            .ok_or_else(|| MhError::Other("venv_python required for Widevine decryption".into()))?;

        let cached_key = self.key_cache.get(file_id).map(|r| r.clone());
        let (decrypt_key_hex, was_cached) = match cached_key {
            Some(k) => (k, true),
            None => {
                let key = self._fetch_widevine_key(file_id, venv_python).await?;
                self.key_cache.insert(file_id.to_string(), key.clone());
                self.persist_key_cache();
                (key, false)
            }
        };

        let direct = self.stream_direct_from_cdn();
        let http = self.client.clone();
        let buf = if direct {
            Self::_ffmpeg_decrypt(cdn_url, &decrypt_key_hex, progress).await?
        } else {
            Self::_fetch_then_decrypt(&http, cdn_url, &decrypt_key_hex, progress).await?
        };

        if buf.is_empty() && was_cached {
            self.log_line(
                "warn",
                format!("Cached Widevine key for {file_id} produced no output; re-licensing"),
            );
            self.key_cache.remove(file_id);
            let key = self._fetch_widevine_key(file_id, venv_python).await?;
            self.key_cache.insert(file_id.to_string(), key.clone());
            self.persist_key_cache();
            return if direct {
                Self::_ffmpeg_decrypt(cdn_url, &key, progress).await
            } else {
                Self::_fetch_then_decrypt(&http, cdn_url, &key, progress).await
            };
        }

        Ok(buf)
    }

    async fn _fetch_widevine_key(&mut self, file_id: &str, venv_python: &Path) -> MhResult<String> {
        let (pssh, access_token, client_token) = self._pssh_and_creds(file_id).await?;

        self._get_widevine_key_via_python(&pssh, venv_python, &access_token, &client_token)
            .await
    }

    /// Fetches the CDN file first so the UI gets a real byte total, then decrypts
    /// the local copy. Letting ffmpeg pull the URL itself yields no Content-Length,
    /// which is why the native Spotify progress bar could only animate.
    async fn _fetch_then_decrypt(
        client: &reqwest::Client,
        cdn_url: &str,
        decrypt_key_hex: &str,
        progress: Option<&crate::downloads::ByteProgress>,
    ) -> MhResult<Bytes> {
        use std::sync::atomic::Ordering;
        let resp = client.get(cdn_url).send().await.map_err(MhError::Network)?;
        if !resp.status().is_success() {
            return Err(MhError::Other(format!(
                "Spotify CDN returned HTTP {} for the audio file",
                resp.status().as_u16()
            )));
        }
        if let Some(prog) = progress {
            prog.total
                .store(resp.content_length().unwrap_or(0), Ordering::Relaxed);
            prog.done.store(0, Ordering::Relaxed);
        }

        let tmp = tempfile::Builder::new()
            .prefix("mh-spotify-")
            .suffix(".mp4")
            .tempfile()?;
        let tmp_path = tmp.path().to_path_buf();
        {
            let mut file = tokio::fs::File::create(&tmp_path).await?;
            let mut stream = resp.bytes_stream();
            let mut written: u64 = 0;
            while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
                let chunk = chunk.map_err(MhError::Network)?;
                tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
                written += chunk.len() as u64;
                if let Some(prog) = progress {
                    prog.done.store(written, Ordering::Relaxed);
                }
            }
            tokio::io::AsyncWriteExt::flush(&mut file).await?;
        }

        let out =
            Self::_ffmpeg_decrypt_path(&tmp_path.to_string_lossy(), decrypt_key_hex, None).await?;
        Ok(out)
    }

    async fn _ffmpeg_decrypt(
        cdn_url: &str,
        decrypt_key_hex: &str,
        progress: Option<&crate::downloads::ByteProgress>,
    ) -> MhResult<Bytes> {
        Self::_ffmpeg_decrypt_path(cdn_url, decrypt_key_hex, progress).await
    }

    async fn _ffmpeg_decrypt_path(
        cdn_url: &str,
        decrypt_key_hex: &str,
        progress: Option<&crate::downloads::ByteProgress>,
    ) -> MhResult<Bytes> {
        let ffmpeg_bin = crate::venv_manager::resolve_ffmpeg();
        let mut ffmpeg_cmd = Command::new(&ffmpeg_bin);
        ffmpeg_cmd
            .args([
                "-y",
                "-loglevel",
                "error",
                "-decryption_key",
                decrypt_key_hex,
                "-i",
                cdn_url,
                "-c",
                "copy",
                "-f",
                "adts",
                "pipe:1",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        crate::subprocess::apply_no_window(&mut ffmpeg_cmd);
        let mut child = ffmpeg_cmd
            .spawn()
            .map_err(|e| MhError::Subprocess(format!("ffmpeg spawn failed: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MhError::Subprocess("No stdout from ffmpeg".into()))?;

        let mut buf = Vec::new();
        let mut reader = BufReader::new(stdout);
        if let Some(prog) = progress {
            use std::sync::atomic::Ordering;
            prog.total.store(0, Ordering::Relaxed);
            prog.done.store(0, Ordering::Relaxed);
            let mut chunk = [0u8; 64 * 1024];
            loop {
                let n = tokio::io::AsyncReadExt::read(&mut reader, &mut chunk).await?;
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                prog.done.store(buf.len() as u64, Ordering::Relaxed);
            }
        } else {
            tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut buf).await?;
        }

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

        let (mut child, mut stdin, mut stderr_pipe) =
            self._spawn_pywidevine(pssh, venv_python, py_script).await?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MhError::Subprocess("No stdout from pywidevine".into()))?;

        let mut lines = BufReader::new(stdout).lines();
        let challenge_line = match lines.next_line().await? {
            Some(line) => line,
            None => {
                let stderr = Self::_drain_pywidevine_stderr(&mut stderr_pipe).await;
                return Err(Self::_pywidevine_failure("challenge", &stderr));
            }
        };

        let challenge_data: Value = serde_json::from_str(&challenge_line)?;
        let challenge_b64 = challenge_data["challenge"]
            .as_str()
            .ok_or_else(|| MhError::Subprocess("No challenge field".into()))?;

        let challenge_bytes = B64
            .decode(challenge_b64)
            .map_err(|e| MhError::Crypto(format!("base64 decode challenge: {e}")))?;

        let mut attempt = 0u32;
        let license_bytes = loop {
            let license_resp = self
                .client
                .post(format!(
                    "{}{}",
                    crate::services::spotify::endpoints::spclient_base(&self.client).await,
                    WIDEVINE_LICENSE_PATH
                ))
                .header("Authorization", format!("Bearer {access_token}"))
                .header("client-token", client_token)
                .header("Content-Type", "application/octet-stream")
                .header("Accept", "*/*")
                .header("app-platform", "WebPlayer")
                .header("spotify-app-version", CLIENT_VERSION)
                .header("Origin", "https://open.spotify.com")
                .header("Referer", "https://open.spotify.com/")
                .header("User-Agent", UA_CHROME_LATEST)
                .body(challenge_bytes.clone())
                .send()
                .await
                .map_err(MhError::Network)?;

            let status = license_resp.status();
            if status.is_success() {
                break license_resp.bytes().await.map_err(MhError::Network)?;
            }

            let code = status.as_u16();
            let ra = retry_after_secs(license_resp.headers());
            if code == 429 {
                if let Some(l) = &self.license_limiter {
                    l.on_rate_limited(ra);
                }
            }
            let retryable = (502..=504).contains(&code) || (code == 429 && ra > 0);
            if self.license_retry_enabled && attempt + 1 < TRANSIENT_MAX_ATTEMPTS && retryable {
                let wait_ms = transient_backoff_ms(code, attempt, ra);
                self.log_line(
                    "warn",
                    format!(
                        "Widevine license {code}; retrying in {wait_ms}ms (attempt {}/{TRANSIENT_MAX_ATTEMPTS})",
                        attempt + 1
                    ),
                );
                tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                attempt += 1;
                continue;
            }

            let resp_headers: Vec<String> = license_resp
                .headers()
                .iter()
                .map(|(k, v)| format!("{}={}", k, v.to_str().unwrap_or("<bin>")))
                .collect();
            let text = license_resp.text().await.unwrap_or_default();
            let body = format!(
                "Widevine license server returned {status}: {text} [client-token: {}; resp-headers: {}]",
                if client_token.is_empty() { "EMPTY" } else { "present" },
                resp_headers.join(", ")
            );
            if code == 403 {
                return Err(MhError::Forbidden(body));
            }
            if code == 429 {
                return Err(MhError::RateLimited(body));
            }
            return Err(MhError::Other(body));
        };
        let license_b64 = B64.encode(&license_bytes);
        let license_input = serde_json::to_string(&json!({ "license": license_b64 }))? + "\n";

        stdin.write_all(license_input.as_bytes()).await?;
        drop(stdin);

        let result_line = match lines.next_line().await? {
            Some(line) => line,
            None => {
                let stderr = Self::_drain_pywidevine_stderr(&mut stderr_pipe).await;
                return Err(Self::_pywidevine_failure("result", &stderr));
            }
        };

        let result: Value = serde_json::from_str(&result_line)?;
        if let Some(err) = result["error"].as_str() {
            return Err(MhError::Crypto(err.to_string()));
        }

        result["key"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| MhError::Crypto("No key in pywidevine result".into()))
    }

    async fn _generate_widevine_challenge(
        &self,
        pssh: &str,
        venv_python: &Path,
    ) -> MhResult<Vec<u8>> {
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
"#;

        let (mut child, stdin, mut stderr_pipe) =
            self._spawn_pywidevine(pssh, venv_python, py_script).await?;
        drop(stdin);

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| MhError::Subprocess("No stdout from pywidevine".into()))?;

        let mut lines = BufReader::new(stdout).lines();
        let challenge_line = match lines.next_line().await? {
            Some(line) => line,
            None => {
                let stderr = Self::_drain_pywidevine_stderr(&mut stderr_pipe).await;
                return Err(Self::_pywidevine_failure("challenge", &stderr));
            }
        };

        let challenge_data: Value = serde_json::from_str(&challenge_line)?;
        let challenge_b64 = challenge_data["challenge"]
            .as_str()
            .ok_or_else(|| MhError::Subprocess("No challenge field".into()))?;

        let challenge_bytes = B64
            .decode(challenge_b64)
            .map_err(|e| MhError::Crypto(format!("base64 decode challenge: {e}")))?;

        drop(child);
        Ok(challenge_bytes)
    }

    /// Spawn the bundled Python with `script` and return the live child with its pipes.
    async fn _spawn_pywidevine(
        &self,
        pssh: &str,
        venv_python: &Path,
        py_script: &str,
    ) -> MhResult<(
        tokio::process::Child,
        tokio::process::ChildStdin,
        Option<tokio::process::ChildStderr>,
    )> {
        let wvd = self.wvd_path.as_deref().unwrap_or("");
        if wvd.is_empty() {
            return Err(MhError::Other(
                "Widevine device file (.wvd) not configured. Set the path in Settings → Spotify → Widevine Device Path.".into()
            ));
        }
        let input_json = serde_json::to_string(&json!({
            "pssh": pssh,
            "wvd_path": wvd
        }))? + "\n";

        let mut py_cmd = Command::new(venv_python);
        py_cmd
            .arg("-c")
            .arg(py_script)
            .envs(crate::venv_manager::python_env())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        crate::subprocess::apply_no_window(&mut py_cmd);
        let mut child = py_cmd
            .spawn()
            .map_err(|e| MhError::Subprocess(format!("pywidevine spawn failed: {e}")))?;

        let stderr_pipe = child.stderr.take();

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| MhError::Subprocess("No stdin for pywidevine".into()))?;
        stdin.write_all(input_json.as_bytes()).await?;

        Ok((child, stdin, stderr_pipe))
    }

    async fn _drain_pywidevine_stderr(stderr: &mut Option<tokio::process::ChildStderr>) -> String {
        let Some(mut pipe) = stderr.take() else {
            return String::new();
        };
        let mut buf = String::new();
        let _ = tokio::io::AsyncReadExt::read_to_string(&mut pipe, &mut buf).await;
        buf.trim().to_string()
    }

    fn _pywidevine_failure(stage: &str, stderr: &str) -> MhError {
        let hint = if stderr.contains("No module named 'pywidevine'")
            || stderr.contains("ModuleNotFoundError")
        {
            " The bundled Python environment is missing pywidevine — \
             reinstall the Spotify dependency from the Dependencies page."
        } else {
            ""
        };
        if stderr.is_empty() {
            MhError::Subprocess(format!(
                "pywidevine exited without emitting a {stage} and produced no stderr.{hint}"
            ))
        } else {
            MhError::Subprocess(format!(
                "pywidevine failed before emitting a {stage}.{hint} stderr: {stderr}"
            ))
        }
    }

    fn _select_relinked_manifest(
        media: &serde_json::Map<String, Value>,
        want_uri: &str,
        relink_uris: &std::collections::HashSet<String>,
    ) -> Option<(Value, Value)> {
        for (key, entry) in media.iter() {
            let entry_link = entry["item"]["metadata"]["linked_from_uri"]
                .as_str()
                .or_else(|| entry["item"]["metadata"]["linked_from"]["uri"].as_str())
                .or_else(|| entry["metadata"]["linked_from_uri"].as_str())
                .or_else(|| entry["metadata"]["linked_from"]["uri"].as_str());
            let is_relink =
                key == want_uri || relink_uris.contains(key) || entry_link == Some(want_uri);
            if !is_relink {
                continue;
            }
            let item_manifest = &entry["item"]["manifest"];
            if !item_manifest.is_null() {
                return Some((item_manifest.clone(), entry["item"]["metadata"].clone()));
            }
            if !entry["manifest"].is_null() {
                return Some((entry["manifest"].clone(), entry["metadata"].clone()));
            }
            if let Some(first) = entry["items"].as_array().and_then(|i| i.first()) {
                if !first["manifest"].is_null() {
                    return Some((first["manifest"].clone(), first["metadata"].clone()));
                }
            }
        }
        None
    }

    async fn _stream_media(
        &mut self,
        media_id: &str,
        media_type: &str,
        venv_python: Option<&Path>,
        preferred_quality: Option<&str>,
        progress: Option<&crate::downloads::ByteProgress>,
    ) -> MhResult<(Bytes, String)> {
        let max_quality = self.account_max_quality();
        let preferred_quality = Self::_effective_quality(preferred_quality, max_quality);
        let mut file_id: Option<String> = None;
        let mut is_ogg = false;
        let mut is_mp3 = false;
        let mut chosen_bitrate_kbps: Option<u32> = None;
        let mut chosen_format_id: Option<u32> = None;
        let mut episode_external_url: Option<String> = None;
        let mut duration_ms: u64 = 0;
        let mut gid_error: Option<String> = None;
        let mut relink_uris: std::collections::HashSet<String> = std::collections::HashSet::new();

        match self._get_gid_metadata(media_id, media_type).await {
            Ok(metadata) => {
                duration_ms = metadata["duration"].as_u64().unwrap_or(0);

                let collect_audio = |arr: &[Value]| -> Vec<Value> {
                    arr.iter()
                        .filter(|f| {
                            f.get("format")
                                .and_then(|v| v.as_str())
                                .is_some_and(Self::is_audio_format)
                        })
                        .cloned()
                        .collect()
                };

                let mut files: Option<Vec<Value>> = None;
                for key in &["file", "audio"] {
                    if let Some(arr) = metadata[key].as_array() {
                        let filtered = collect_audio(arr);
                        if !filtered.is_empty() {
                            files = Some(filtered);
                            break;
                        }
                    }
                }

                if files.as_ref().is_none_or(|f| f.is_empty()) {
                    if let Some(alts) = metadata["alternative"].as_array() {
                        for alt in alts {
                            for key in &["file", "audio"] {
                                if let Some(arr) = alt[key].as_array() {
                                    let filtered = collect_audio(arr);
                                    if !filtered.is_empty() {
                                        files = Some(filtered);
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

                if let Some(alts) = metadata["alternative"].as_array() {
                    for alt in alts {
                        if let Some(gid) = alt["gid"].as_str() {
                            if let Some(alt_id) = Self::_gid_to_id(gid) {
                                relink_uris.insert(format!("spotify:{media_type}:{alt_id}"));
                            }
                        }
                    }
                }

                if let Some(ref flist) = files {
                    let picked = self._select_audio_file_preferred(flist, preferred_quality);
                    if let Some((selected, ogg)) = picked {
                        let fmt = selected["format"].as_str().unwrap_or("");
                        let is_bare_aac = fmt == "AAC_24" || fmt == "AAC_48";
                        if !is_bare_aac {
                            file_id = selected["file_id"].as_str().map(String::from);
                            is_ogg = ogg;
                            is_mp3 = !ogg && fmt.starts_with("MP3_");
                        }
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

            let want_uri = format!("spotify:{media_type}:{media_id}");
            if manifest.is_null() {
                if let Some(media) = playback_info["media"].as_object() {
                    match Self::_select_relinked_manifest(media, &want_uri, &relink_uris) {
                        Some((m, md)) => {
                            manifest = m;
                            item_meta = md;
                        }
                        None => {
                            let actual: Vec<String> = media.keys().cloned().collect();
                            return Err(MhError::Unsupported(format!(
                                "Spotify could not deliver a playable manifest for {want_uri} \
                                 (offered: {actual:?}). The track may use Spotify's UUID-based audio \
                                 delivery, which isn't supported yet."
                            )));
                        }
                    }
                }
            }

            if let Some(d) = item_meta["duration"].as_u64() {
                duration_ms = d;
            }

            let no_mp4 = manifest["file_ids_mp4"]
                .as_array()
                .is_none_or(|a| a.is_empty());
            let no_ogg = manifest["file_ids_ogg"]
                .as_array()
                .is_none_or(|a| a.is_empty());
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

            let user_picked_ogg = preferred_quality.starts_with("vorbis");
            let user_picked_mp3 = preferred_quality.starts_with("mp3");
            let user_picked_aac = preferred_quality.starts_with("aac");
            let user_picked_strict = user_picked_ogg || user_picked_mp3;
            let allowed_formats: Option<&'static [&'static str]> =
                Self::_format_ladder_for_quality(preferred_quality);

            if let Some(ogg_files) = manifest["file_ids_ogg"].as_array() {
                if !ogg_files.is_empty() && !user_picked_mp3 && !user_picked_aac {
                    let preferred =
                        if let Some(ladder) = allowed_formats.filter(|_| user_picked_ogg) {
                            ladder.iter().find_map(|fmt| {
                                ogg_files
                                    .iter()
                                    .find(|f| f["format"].as_str() == Some(*fmt))
                            })
                        } else {
                            ogg_files
                                .iter()
                                .find(|f| f["format"].as_str() == Some("OGG_VORBIS_320"))
                                .or_else(|| {
                                    ogg_files
                                        .iter()
                                        .find(|f| f["format"].as_str() == Some("OGG_VORBIS_160"))
                                })
                                .or_else(|| ogg_files.first())
                        };
                    if let Some(f) = preferred {
                        file_id = f["file_id"].as_str().map(String::from);
                        is_ogg = true;
                    }
                }
            }

            if file_id.is_none() {
                if let Some(mp3_files) = manifest["file_ids_mp3"].as_array() {
                    if !mp3_files.is_empty() && !user_picked_ogg && !user_picked_aac {
                        let preferred =
                            if let Some(ladder) = allowed_formats.filter(|_| user_picked_mp3) {
                                ladder.iter().find_map(|fmt| {
                                    mp3_files
                                        .iter()
                                        .find(|f| f["format"].as_str() == Some(*fmt))
                                })
                            } else {
                                ["MP3_320", "MP3_256", "MP3_160", "MP3_128", "MP3_96"]
                                    .iter()
                                    .find_map(|fmt| {
                                        mp3_files
                                            .iter()
                                            .find(|f| f["format"].as_str() == Some(*fmt))
                                    })
                                    .or_else(|| mp3_files.first())
                            };
                        if let Some(f) = preferred {
                            file_id = f["file_id"].as_str().map(String::from);
                            is_mp3 = true;
                        }
                    }
                }
            }

            let ceiling = Self::_aac_ceiling_kbps_for_quality(preferred_quality);
            let capture_bitrate = |f: &Value| -> Option<u32> {
                f.get("bitrate")
                    .and_then(|v| v.as_u64())
                    .map(|b| (b / 1000) as u32)
                    .or_else(|| match f.get("format").and_then(|v| v.as_str()) {
                        Some("11") | Some("13") => Some(256),
                        Some("10") | Some("12") => Some(128),
                        _ => None,
                    })
            };
            let capture_format_id = |f: &Value| -> Option<u32> {
                f.get("format")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u32>().ok())
            };

            if file_id.is_none() {
                if let Some(mp4_files) = manifest["file_ids_mp4"].as_array() {
                    if !mp4_files.is_empty() {
                        if let Some(f) = Self::_select_mp4_for_ceiling(mp4_files, ceiling) {
                            file_id = f["file_id"].as_str().map(String::from);
                            chosen_bitrate_kbps = capture_bitrate(f);
                            chosen_format_id = capture_format_id(f);
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
                                || e["qualities"].as_array().is_some_and(|q| !q.is_empty())
                        })
                        .and_then(|e| e["qualities"].as_array())
                        .map(|q| q.to_vec())
                        .unwrap_or_else(|| dual.to_vec());

                    if let Some(f) = Self::_select_mp4_for_ceiling(&entries, ceiling) {
                        file_id = f["file_id"].as_str().map(String::from);
                        chosen_bitrate_kbps = capture_bitrate(f);
                        chosen_format_id = capture_format_id(f);
                    }
                }
            }

            if file_id.is_none() {
                let no_known_formats = [
                    "file_ids_ogg",
                    "file_ids_mp3",
                    "file_ids_mp4",
                    "file_ids_mp4_dual",
                ]
                .iter()
                .all(|k| manifest[*k].as_array().is_none_or(|a| a.is_empty()));
                if no_known_formats {
                    return Err(MhError::Other(
                        "Spotify isn't serving any downloadable stream for this track. \
                         It likely uses Spotify's new UUID-based audio delivery, which \
                         the native sp_dc downloader can't fetch yet. Switch to Votify \
                         in Settings → Spotify → Downloader, or pick a different track."
                            .to_string(),
                    ));
                }
            }

            if file_id.is_none() && user_picked_strict {
                let q = preferred_quality;
                return Err(MhError::Other(format!(
                    "Spotify doesn't have {q} available for this track. \
                     Try a different quality, or switch to Votify in \
                     Settings → Spotify → Downloader if you want the MP4/AAC tier."
                )));
            }

            if file_id.is_none() && media_type == "episode" {
                let show_id = item_meta["group_uri"]
                    .as_str()
                    .and_then(|s| s.split(':').next_back())
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

        let resolve_format_id = chosen_format_id
            .or(match chosen_bitrate_kbps {
                Some(k) if k <= 128 => Some(10),
                Some(_) => Some(11),
                None => None,
            })
            .unwrap_or(if self.is_known_free() { 10 } else { 11 });
        let cdn_url = self
            ._resolve_storage_for_format(&fid, resolve_format_id)
            .await?;

        if is_ogg || is_mp3 {
            return Err(MhError::Unsupported(
                "Native OGG/MP3 download is no longer supported; choose an AAC quality.".into(),
            ));
        }

        let bytes = self
            ._stream_mp4_with_widevine(&cdn_url, &fid, venv_python, progress)
            .await?;
        let content_type = match chosen_bitrate_kbps {
            Some(kbps) => format!("audio/aac; bitrate={}", kbps * 1000),
            None => "audio/aac".to_string(),
        };
        Ok((bytes, content_type))
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::parse_track_download_meta;
    use serde_json::json;

    /// The album object arrives nested in the same extended-metadata payload as the
    /// track, so its date, barcode, copyright, genre and disc count are all in hand —
    /// they were simply never read.
    #[test]
    fn the_nested_album_supplies_the_release_level_tags() {
        let meta = json!({
            "name": "Face to Face",
            "artist": [ { "name": "Daft Punk" }, { "name": "Todd Edwards" } ],
            "number": 5,
            "disc_number": 1,
            "external_id": [ { "type": "isrc", "id": "GBDUW0000057" } ],
            "album": {
                "name": "Discovery",
                "label": "Daft Life Ltd.",
                "artist": [ { "name": "Daft Punk" } ],
                "date": { "year": 2001, "month": 3, "day": 7 },
                "genre": [ "Electronic" ],
                "external_id": [ { "type": "upc", "id": "724384960650" } ],
                "copyright": [ { "text": "(P) 2001 Daft Life" } ],
                "disc": [
                    { "track": [ {}, {} ] },
                    { "track": [ {} ] }
                ]
            }
        });
        let parsed = parse_track_download_meta("abc", &meta);

        assert_eq!(parsed.year.as_deref(), Some("2001"));
        assert_eq!(parsed.date.as_deref(), Some("2001-03-07"));
        assert_eq!(parsed.upc.as_deref(), Some("724384960650"));
        assert_eq!(parsed.copyright.as_deref(), Some("(P) 2001 Daft Life"));
        assert_eq!(parsed.genre.as_deref(), Some("Electronic"));
        assert_eq!(parsed.disc_total, Some(2));
        assert_eq!(parsed.total_tracks, Some(3));
        assert_eq!(parsed.artists, ["Daft Punk", "Todd Edwards"]);
    }

    #[test]
    fn a_year_only_date_does_not_invent_a_month_or_day() {
        let meta = json!({
            "name": "Track",
            "album": { "name": "Album", "date": { "year": 1999 } }
        });
        let parsed = parse_track_download_meta("abc", &meta);
        assert_eq!(parsed.date.as_deref(), Some("1999"));
        assert_eq!(parsed.year.as_deref(), Some("1999"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WANT: &str = "spotify:track:2yLllJi0bcfeX1EHpkmEUQ";
    const RELINK: &str = "spotify:track:1gYxCk85V8XUhnOOvmTwLm";

    fn manifest_entry(linked_from: Option<&str>) -> Value {
        let mut metadata = json!({ "duration": 186371 });
        if let Some(lf) = linked_from {
            metadata["linked_from_uri"] = json!(lf);
        }
        json!({
            "item": {
                "manifest": { "file_ids_mp4_dual": [{ "bitrate": 256000, "file_id": "deadbeef" }] },
                "metadata": metadata,
            }
        })
    }

    fn empty_set() -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }

    #[test]
    fn relink_followed_via_alternative_set() {
        let media = json!({ RELINK: manifest_entry(None) });
        let media = media.as_object().unwrap();
        let mut relinks = std::collections::HashSet::new();
        relinks.insert(RELINK.to_string());

        let picked = LibrespotService::_select_relinked_manifest(media, WANT, &relinks);
        let (manifest, _) = picked.expect("verified relink should be followed");
        assert!(!manifest["file_ids_mp4_dual"].is_null());
    }

    #[test]
    fn relink_followed_via_linked_from_marker() {
        let media = json!({ RELINK: manifest_entry(Some(WANT)) });
        let media = media.as_object().unwrap();

        let picked = LibrespotService::_select_relinked_manifest(media, WANT, &empty_set());
        assert!(picked.is_some(), "linked_from marker should be honored");
    }

    #[test]
    fn exact_match_selected() {
        let media = json!({ WANT: manifest_entry(None) });
        let media = media.as_object().unwrap();

        let picked = LibrespotService::_select_relinked_manifest(media, WANT, &empty_set());
        assert!(picked.is_some());
    }

    #[test]
    fn unverified_substitution_refused() {
        let media = json!({ RELINK: manifest_entry(None) });
        let media = media.as_object().unwrap();

        let picked = LibrespotService::_select_relinked_manifest(media, WANT, &empty_set());
        assert!(picked.is_none(), "unverified substitution must be refused");
    }

    #[test]
    fn manifestless_match_skipped_for_manifest_bearing_relink() {
        let media = json!({
            WANT: { "item": { "metadata": { "duration": 0 } } },
            RELINK: manifest_entry(Some(WANT)),
        });
        let media = media.as_object().unwrap();

        let picked = LibrespotService::_select_relinked_manifest(media, WANT, &empty_set());
        let (manifest, _) = picked.expect("should fall through to the manifest-bearing relink");
        assert!(!manifest["file_ids_mp4_dual"].is_null());
    }

    #[test]
    fn gid_id_roundtrip_matches_known_relink() {
        let alt_id = LibrespotService::_gid_to_id("29da8124e83343d28cd4c5b19c6df940").unwrap();
        assert_eq!(alt_id, "1gYxCk85V8XUhnOOvmTwLm");
    }

    fn svc_with_plan(plan: Option<Value>) -> LibrespotService {
        let mut svc = LibrespotService::new();
        svc.user_profile = plan.map(|p| json!({ "plan": p }));
        svc
    }

    #[test]
    fn known_free_only_for_explicit_non_premium_plan() {
        assert!(!svc_with_plan(Some(json!("premium"))).is_known_free());
        assert!(!svc_with_plan(Some(json!("PREMIUM"))).is_known_free());
        assert!(svc_with_plan(Some(json!("free"))).is_known_free());
        assert!(svc_with_plan(Some(json!("open"))).is_known_free());
        assert!(!svc_with_plan(Some(Value::Null)).is_known_free());
        assert!(!svc_with_plan(None).is_known_free());
        let mut svc = LibrespotService::new();
        svc.user_profile = Some(json!({ "name": "Spotify User" }));
        assert!(!svc.is_known_free());
    }

    #[test]
    fn account_max_quality_caps_only_known_free() {
        assert_eq!(
            svc_with_plan(Some(json!("free"))).account_max_quality(),
            "aac-medium"
        );
        assert_eq!(
            svc_with_plan(Some(json!("premium"))).account_max_quality(),
            "aac-high"
        );
        assert_eq!(svc_with_plan(None).account_max_quality(), "aac-high");
    }

    #[test]
    fn effective_quality_clamps_high_to_max() {
        assert_eq!(
            LibrespotService::_effective_quality(Some("aac-high"), "aac-medium"),
            "aac-medium"
        );
        assert_eq!(
            LibrespotService::_effective_quality(Some("aac-medium"), "aac-medium"),
            "aac-medium"
        );
        assert_eq!(
            LibrespotService::_effective_quality(None, "aac-medium"),
            "aac-medium"
        );
        assert_eq!(
            LibrespotService::_effective_quality(Some("aac-high"), "aac-high"),
            "aac-high"
        );
        assert_eq!(
            LibrespotService::_effective_quality(Some("aac-medium"), "aac-high"),
            "aac-medium"
        );
        assert_eq!(
            LibrespotService::_effective_quality(None, "aac-high"),
            "aac-high"
        );
    }

    #[test]
    fn mp4_selection_picks_128_class_at_low_ceiling() {
        let files = vec![
            json!({ "file_id": "hi", "format": "11", "bitrate": 256000 }),
            json!({ "file_id": "lo", "format": "10", "bitrate": 128000 }),
        ];
        let picked = LibrespotService::_select_mp4_for_ceiling(&files, 128).unwrap();
        assert_eq!(picked["file_id"].as_str(), Some("lo"));
        let fmt: u32 = picked["format"].as_str().unwrap().parse().unwrap();
        assert_eq!(fmt, 10);
        let picked_hi = LibrespotService::_select_mp4_for_ceiling(&files, 256).unwrap();
        assert_eq!(picked_hi["file_id"].as_str(), Some("hi"));
    }

    const RSS_WITH_ENTITIES: &str = r#"<?xml version="1.0"?>
<rss><channel>
  <item>
    <title>Robots &amp; Machines</title>
    <guid>spotify:episode:aaa</guid>
    <itunes:duration> 00:31:00 </itunes:duration>
    <enclosure url="https://example.invalid/one.mp3" />
  </item>
  <item>
    <title>Discovery &#8212; Part &#x32;</title>
    <guid>spotify:episode:bbb</guid>
    <itunes:duration>00:42:00</itunes:duration>
    <enclosure url="https://example.invalid/two.mp3" />
  </item>
</channel></rss>"#;

    #[test]
    fn rss_title_with_named_entity_is_matched_whole() {
        let got = LibrespotService::_rss_extract_item_texts(
            RSS_WITH_ENTITIES,
            "unknown",
            Some("Robots & Machines"),
            0,
        );
        assert_eq!(got.as_deref(), Some("https://example.invalid/one.mp3"));
    }

    #[test]
    fn rss_title_with_numeric_entities_is_matched_whole() {
        let got = LibrespotService::_rss_extract_item_texts(
            RSS_WITH_ENTITIES,
            "unknown",
            Some("Discovery — Part 2"),
            0,
        );
        assert_eq!(got.as_deref(), Some("https://example.invalid/two.mp3"));
    }

    #[test]
    fn rss_guid_and_duration_still_resolve() {
        let by_guid = LibrespotService::_rss_extract_item_texts(RSS_WITH_ENTITIES, "bbb", None, 0);
        assert_eq!(by_guid.as_deref(), Some("https://example.invalid/two.mp3"));

        let by_duration = LibrespotService::_rss_extract_item_texts(
            RSS_WITH_ENTITIES,
            "unknown",
            None,
            1_860_000,
        );
        assert_eq!(
            by_duration.as_deref(),
            Some("https://example.invalid/one.mp3")
        );
    }
}

/// Stream ids Spotify playback has already registered, so replaying a track does not
/// mint a second one. Mirrors [`crate::services::youtube::stream::YtAudioStreamCache`]:
/// the type owns its own eviction rule rather than leaving it written out at the call
/// site, which is what a bare `DashMap<String, (String, String)>` on `BackendState` did.
#[derive(Default)]
pub struct SpotifyStreamMemo {
    inner: dashmap::DashMap<String, (String, String)>,
}

impl SpotifyStreamMemo {
    /// The `(stream id, url)` remembered for a track, if any.
    pub fn get(&self, track_id: &str) -> Option<(String, String)> {
        self.inner.get(track_id).map(|e| e.value().clone())
    }

    /// Remember a freshly registered stream, dropping the whole table once it grows
    /// past `CAPACITY` — the streaming server evicts its own entries anyway, so a
    /// precise LRU would only be tracking something it does not own.
    pub fn remember(&self, track_id: String, stream_id: String, url: String) {
        const CAPACITY: usize = 8;
        if self.inner.len() >= CAPACITY {
            self.inner.clear();
        }
        self.inner.insert(track_id, (stream_id, url));
    }
}

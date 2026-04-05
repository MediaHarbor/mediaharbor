
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bytes::Bytes;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, COOKIE, ORIGIN, REFERER},
    Client,
};
use serde_json::{json, Value};

use crate::{
    crypto::mp4decrypt,
    defaults::Settings,
    errors::{MhError, MhResult},
    http_client::{UA_CHROME_LATEST, PLATFORM_HEADER},
    venv_manager,
};

const APPLE_MUSIC_HOMEPAGE: &str = "https://music.apple.com";
const AMP_API_URL: &str = "https://amp-api.music.apple.com";
const WEBPLAYBACK_API_URL: &str =
    "https://play.itunes.apple.com/WebObjects/MZPlay.woa/wa/webPlayback";
const WIDEVINE_SYSTEM_ID: [u8; 16] = [
    0xed, 0xef, 0x8b, 0xa9, 0x79, 0xd6, 0x4a, 0xce,
    0xa3, 0xc8, 0x27, 0xdc, 0xd5, 0x1d, 0x21, 0xed,
];

pub struct AppleTrackStream {
    pub data: bytes::Bytes,
    pub content_type: String,
    pub duration_ms: u64,
}

struct HlsStreamInfo {
    stream_url: String,
    widevine_pssh: Option<String>,
    media_id: String,
    legacy: bool,
}

struct DevTokenCache {
    token: Option<String>,
    expires_at: Option<Instant>,
}

impl DevTokenCache {
    fn new() -> Self {
        Self { token: None, expires_at: None }
    }

    fn get(&self) -> Option<&str> {
        if let (Some(t), Some(exp)) = (&self.token, &self.expires_at) {
            if Instant::now() < *exp {
                return Some(t.as_str());
            }
        }
        None
    }

    fn set(&mut self, token: String) {
        self.token = Some(token);
        self.expires_at = Some(Instant::now() + Duration::from_secs(3600));
    }
}

struct WvdPathCache {
    path: Option<PathBuf>,
}

impl WvdPathCache {
    fn new() -> Self {
        Self { path: None }
    }
}

pub struct AppleMusicService {
    configured: bool,
    cookies_path: Option<String>,
    wvd_path: Option<String>,
    dev_token_cache: Arc<Mutex<DevTokenCache>>,
    wvd_path_cache: Arc<Mutex<WvdPathCache>>,
}

impl AppleMusicService {
    pub fn new() -> Self {
        Self {
            configured: false,
            cookies_path: None,
            wvd_path: None,
            dev_token_cache: Arc::new(Mutex::new(DevTokenCache::new())),
            wvd_path_cache: Arc::new(Mutex::new(WvdPathCache::new())),
        }
    }

    pub fn from_settings(settings: &Settings) -> Self {
        let configured = !settings.apple_cookies_path.is_empty();
        Self {
            configured,
            cookies_path: if configured { Some(settings.apple_cookies_path.clone()) } else { None },
            wvd_path: if settings.apple_wvd_path.is_empty() { None } else { Some(settings.apple_wvd_path.clone()) },
            dev_token_cache: Arc::new(Mutex::new(DevTokenCache::new())),
            wvd_path_cache: Arc::new(Mutex::new(WvdPathCache::new())),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.configured
    }

    pub async fn get_gamdl_wvd_path(&self) -> MhResult<PathBuf> {
        get_gamdl_wvd_path(&self.wvd_path, &self.wvd_path_cache).await
    }

    async fn get_dev_token(&self) -> MhResult<String> {
        {
            let cache = self.dev_token_cache.lock().unwrap();
            if let Some(t) = cache.get() {
                return Ok(t.to_string());
            }
        }
        let token = get_developer_token().await?;
        {
            let mut cache = self.dev_token_cache.lock().unwrap();
            cache.set(token.clone());
        }
        Ok(token)
    }

    pub async fn get_track_stream(
        &self,
        url: &str,
        _quality: Option<u8>,
    ) -> MhResult<AppleTrackStream> {
        let cookies_path = self
            .cookies_path
            .as_deref()
            .ok_or_else(|| MhError::Auth("Apple Music cookies path not set".into()))?;

        let parsed = crate::meta::apple_music_meta::parse_apple_music_url(url)
            .ok_or_else(|| MhError::Parse(format!("Could not parse Apple Music URL: {}", url)))?;
        let song_id = parsed.content_id.clone();

        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;

        let client = build_apple_client()?;

        let account_storefront =
            get_account_storefront(&client, &dev_token, &media_user_token).await;
        let storefront = account_storefront
            .unwrap_or_else(|| parsed.storefront.clone());

        if parsed.content_type == "music-video" {
            let hls_url =
                get_music_video_hls_url(&client, &song_id, &dev_token, &media_user_token, cookies_path)
                    .await?;
            let hls_info =
                parse_hls_for_stream(&client, &hls_url, &song_id, &dev_token, &media_user_token)
                    .await?;
            if hls_info.widevine_pssh.is_none() {
                return Err(MhError::Other(
                    "No Widevine PSSH found in Apple Music music-video HLS manifest".into(),
                ));
            }
            let wvd_path = self.get_gamdl_wvd_path().await?;
            return decrypt_and_collect_hls(
                &client,
                hls_info,
                &wvd_path,
                &dev_token,
                &media_user_token,
                cookies_path,
                0,
                "video/mp4",
            )
            .await;
        }

        let song_metadata =
            get_song_metadata(&client, &song_id, &storefront, &dev_token, &media_user_token)
                .await?;
        let duration_ms = song_metadata["attributes"]["durationInMillis"]
            .as_u64()
            .unwrap_or(0);
        let m3u8_url = song_metadata["attributes"]["extendedAssetUrls"]["enhancedHls"]
            .as_str()
            .ok_or_else(|| {
                MhError::Other(
                    "No HLS stream URL available. Check that your Apple Music subscription is active.".into(),
                )
            })?
            .to_string();

        let wvd_path = self.get_gamdl_wvd_path().await?;

        let legacy_client = client.clone();
        let legacy_song_id = song_id.clone();
        let legacy_dev_token = dev_token.clone();
        let legacy_media_user_token = media_user_token.clone();
        let legacy_cookies_path = cookies_path.to_string();
        let legacy_fut = tokio::spawn(async move {
            get_legacy_stream_info(
                &legacy_client,
                &legacy_song_id,
                &legacy_dev_token,
                &legacy_media_user_token,
                &legacy_cookies_path,
            )
            .await
        });

        let hls_info =
            parse_hls_for_stream(&client, &m3u8_url, &song_id, &dev_token, &media_user_token)
                .await?;

        let final_hls_info = if hls_info.widevine_pssh.is_none() {
            legacy_fut
                .await
                .map_err(|e| MhError::Other(format!("legacy stream task panic: {}", e)))?
                .map_err(|e| {
                    MhError::Other(format!(
                        "No Widevine PSSH in HLS manifest — legacy fallback failed: {}",
                        e
                    ))
                })?
        } else {
            hls_info
        };

        decrypt_and_collect_hls(
            &client,
            final_hls_info,
            &wvd_path,
            &dev_token,
            &media_user_token,
            cookies_path,
            duration_ms,
            "audio/mp4",
        )
        .await
    }
}

impl Default for AppleMusicService {
    fn default() -> Self {
        Self::new()
    }
}

fn build_apple_client() -> MhResult<Client> {
    reqwest::ClientBuilder::new()
        .user_agent(UA_CHROME_LATEST)
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(MhError::Network)
}

fn apple_headers(dev_token: &str, media_user_token: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", dev_token)).unwrap(),
    );
    h.insert(
        "Media-User-Token",
        HeaderValue::from_str(media_user_token).unwrap(),
    );
    h.insert(ORIGIN, HeaderValue::from_static(APPLE_MUSIC_HOMEPAGE));
    h.insert(REFERER, HeaderValue::from_static(APPLE_MUSIC_HOMEPAGE));
    h.insert("accept", HeaderValue::from_static("*/*"));
    h.insert("accept-language", HeaderValue::from_static("en-US"));
    h.insert("priority", HeaderValue::from_static("u=1, i"));
    h.insert(
        "sec-ch-ua",
        HeaderValue::from_static(
            r#""Google Chrome";v="137", "Chromium";v="137", "Not/A)Brand";v="24""#,
        ),
    );
    h.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
    h.insert("sec-ch-ua-platform", HeaderValue::from_static(PLATFORM_HEADER));
    h.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
    h.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    h.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
    h
}

fn seg_fetch_headers(dev_token: &str, media_user_token: &str) -> HeaderMap {
    let mut h = apple_headers(dev_token, media_user_token);
    h.insert(
        COOKIE,
        HeaderValue::from_str(&format!("media-user-token={}", media_user_token)).unwrap(),
    );
    h.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
    h
}

pub fn get_media_user_token(cookies_path: &str) -> MhResult<String> {
    let content = std::fs::read_to_string(cookies_path).map_err(|e| {
        MhError::Io(e)
    })?;

    for line in content.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 7 && parts[5] == "media-user-token" {
            let domain = parts[0];
            if domain == ".music.apple.com" || domain == "music.apple.com" {
                return Ok(parts[6].trim().to_string());
            }
        }
    }

    for line in content.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 7
            && parts[5] == "media-user-token"
            && parts[0].contains("apple.com")
        {
            return Ok(parts[6].trim().to_string());
        }
    }

    Err(MhError::Auth(
        "media-user-token not found in cookies file. \
         Make sure you exported cookies from music.apple.com while logged in."
            .into(),
    ))
}

fn build_apple_cookie_header(cookies_path: &str) -> String {
    let content = match std::fs::read_to_string(cookies_path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let mut pairs = Vec::new();
    for line in content.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        let domain = parts[0];
        let name = parts[5];
        let value = parts[6].trim();
        if domain == ".music.apple.com" || domain == "music.apple.com" {
            pairs.push(format!("{}={}", name, value));
        }
    }
    pairs.join("; ")
}

async fn get_developer_token() -> MhResult<String> {
    let client = reqwest::ClientBuilder::new()
        .user_agent(UA_CHROME_LATEST)
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(MhError::Network)?;

    let html = client
        .get(APPLE_MUSIC_HOMEPAGE)
        .send()
        .await
        .map_err(MhError::Network)?
        .text()
        .await
        .map_err(MhError::Network)?;

    let js_path = regex::Regex::new(r"/(assets/index-legacy[~-][^/&quot;]+\.js)")
        .unwrap()
        .captures(&html)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .ok_or_else(|| MhError::Parse("Could not find index.js URI in Apple Music homepage".into()))?;

    let js_url = format!("{}/{}", APPLE_MUSIC_HOMEPAGE, js_path);
    let js = client
        .get(&js_url)
        .send()
        .await
        .map_err(MhError::Network)?
        .text()
        .await
        .map_err(MhError::Network)?;

    let token = regex::Regex::new(r#"(eyJh[^"]+)"#)
        .unwrap()
        .captures(&js)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .ok_or_else(|| MhError::Parse("Could not extract developer token from Apple Music JS".into()))?;

    Ok(token)
}

async fn get_account_storefront(
    client: &Client,
    dev_token: &str,
    media_user_token: &str,
) -> Option<String> {
    let url = format!("{}/v1/me/account", AMP_API_URL);
    let resp = client
        .get(&url)
        .headers(apple_headers(dev_token, media_user_token))
        .send()
        .await
        .ok()?;
    let data: Value = resp.json().await.ok()?;
    data["meta"]["subscription"]["storefront"]
        .as_str()
        .map(str::to_string)
}

async fn get_song_metadata(
    client: &Client,
    song_id: &str,
    storefront: &str,
    dev_token: &str,
    media_user_token: &str,
) -> MhResult<Value> {
    let url = format!(
        "{}/v1/catalog/{}/songs/{}?extend=extendedAssetUrls&include=albums",
        AMP_API_URL, storefront, song_id
    );
    let resp = client
        .get(&url)
        .headers(apple_headers(dev_token, media_user_token))
        .send()
        .await
        .map_err(MhError::Network)?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        return Err(MhError::Other(format!(
            "Apple Music API song request failed: {} — {}",
            status,
            &text[..text.len().min(200)]
        )));
    }

    let data: Value = resp.json().await.map_err(MhError::Network)?;
    data["data"]
        .as_array()
        .and_then(|arr| arr.first().cloned())
        .ok_or_else(|| MhError::NotFound(format!("Apple Music returned no song metadata for {}", song_id)))
}

async fn get_web_playback(
    client: &Client,
    song_id: &str,
    dev_token: &str,
    media_user_token: &str,
    cookies_path: &str,
) -> MhResult<Value> {
    let cookie_header = build_apple_cookie_header(cookies_path);
    let url = format!("{}?l=en-US", WEBPLAYBACK_API_URL);
    let mut headers = apple_headers(dev_token, media_user_token);
    headers.insert(
        COOKIE,
        HeaderValue::from_str(&cookie_header).unwrap_or_else(|_| HeaderValue::from_static("")),
    );

    let resp = client
        .post(&url)
        .headers(headers)
        .json(&json!({
            "salableAdamId": song_id,
            "language": "en-US"
        }))
        .send()
        .await
        .map_err(MhError::Network)?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        return Err(MhError::Other(format!(
            "WebPlayback API returned {}: {}",
            status,
            &text[..text.len().min(200)]
        )));
    }

    let data: Value = resp.json().await.map_err(MhError::Network)?;
    if data.get("dialog").is_some() || data.get("failureType").is_some() {
        let msg = data["customerMessage"]
            .as_str()
            .or_else(|| data["failureType"].as_str())
            .unwrap_or("subscription may be inactive");
        return Err(MhError::Auth(format!(
            "WebPlayback returned failure: {}",
            msg
        )));
    }
    Ok(data)
}

async fn get_music_video_hls_url(
    client: &Client,
    video_id: &str,
    dev_token: &str,
    media_user_token: &str,
    cookies_path: &str,
) -> MhResult<String> {
    let data = get_web_playback(client, video_id, dev_token, media_user_token, cookies_path).await?;
    data["songList"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|s| s["hls-playlist-url"].as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            MhError::Other(
                "No HLS playlist URL in Apple Music WebPlayback response for music video".into(),
            )
        })
}

fn is_prefetch_pssh(uri: &str) -> bool {
    let b64 = if uri.starts_with("data:") {
        uri.split(',').last().unwrap_or(uri)
    } else {
        uri
    };
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map(|decoded| {
            let needle = b"\x73\x31\x2f\x65\x31";
            decoded.windows(needle.len()).any(|w| w == needle)
        })
        .unwrap_or(false)
}

fn extract_pssh_from_mp4(buf: &[u8]) -> Option<String> {
    
    let mut results: Vec<String> = Vec::new();
    scan_mp4_for_pssh(buf, 0, buf.len(), &mut results);
    results
        .iter()
        .find(|b64| !is_prefetch_pssh(&format!("data:text/plain;base64,{}", b64)))
        .or_else(|| results.first())
        .cloned()
}

fn scan_mp4_for_pssh(data: &[u8], start: usize, end: usize, out: &mut Vec<String>) {
    use base64::Engine;
    let mut off = start;
    while off + 8 <= end {
        if off + 4 > data.len() {
            break;
        }
        let size_bytes: [u8; 4] = data[off..off + 4].try_into().unwrap_or([0; 4]);
        let size = u32::from_be_bytes(size_bytes) as usize;
        if size < 8 || off + size > end || off + size > data.len() {
            break;
        }
        let box_type = std::str::from_utf8(&data[off + 4..off + 8]).unwrap_or("");
        if box_type == "pssh" && size >= 28 {
            let sys_id = &data[off + 12..off + 28.min(off + size)];
            if sys_id == WIDEVINE_SYSTEM_ID {
                let b64 = base64::engine::general_purpose::STANDARD
                    .encode(&data[off..off + size]);
                out.push(b64);
            }
        }
        if matches!(box_type, "moov" | "trak" | "mdia" | "minf" | "stbl" | "udta" | "moof" | "traf") {
            scan_mp4_for_pssh(data, off + 8, off + size, out);
        }
        off += size;
    }
}

fn get_session_data_value<'a>(text: &'a str, data_id: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with("#EXT-X-SESSION-DATA:") {
            continue;
        }
        if !t.contains(data_id) {
            continue;
        }
        if let Some(m) = regex::Regex::new(r#"VALUE="([^"]+)""#)
            .ok()
            .and_then(|re| re.captures(t))
            .and_then(|c| c.get(1))
        {
            return Some(m.as_str().to_string());
        }
    }
    None
}

fn find_widevine_pssh_in_manifest(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with("#EXT-X-KEY") && !t.starts_with("#EXT-X-SESSION-KEY") {
            continue;
        }
        let lower = t.to_lowercase();
        if !lower.contains("edef8ba9") && !lower.contains("com.widevine") {
            continue;
        }
        if let Some(m) = regex::Regex::new(r#"URI="([^"]+)""#)
            .ok()
            .and_then(|re| re.captures(t))
            .and_then(|c| c.get(1))
        {
            let uri = m.as_str();
            if !is_prefetch_pssh(uri) {
                return Some(uri.to_string());
            }
        }
    }
    None
}

fn find_widevine_in_session_key_info(session_key_info: &Value) -> Option<String> {
    let obj = session_key_info.as_object()?;
    for drm_map in obj.values() {
        let drm_obj = drm_map.as_object()?;
        for (urn_key, entry) in drm_obj {
            if urn_key.to_lowercase().contains("edef8ba9") {
                if let Some(uri) = entry["URI"].as_str() {
                    if !is_prefetch_pssh(uri) {
                        return Some(uri.to_string());
                    }
                }
            }
        }
    }
    None
}

fn get_init_segment_url(playlist_text: &str, base_url: &str) -> Option<String> {
    for line in playlist_text.lines() {
        if let Some(m) = regex::Regex::new(r#"#EXT-X-MAP:URI="([^"]+)""#)
            .ok()
            .and_then(|re| re.captures(line.trim()))
            .and_then(|c| c.get(1))
        {
            let u = m.as_str();
            return Some(if u.starts_with("http") {
                u.to_string()
            } else {
                format!("{}{}", base_url, u)
            });
        }
    }
    None
}

async fn parse_hls_for_stream(
    client: &Client,
    m3u8_url: &str,
    song_id: &str,
    dev_token: &str,
    media_user_token: &str,
) -> MhResult<HlsStreamInfo> {
    let headers = seg_fetch_headers(dev_token, media_user_token);

    let resp = client
        .get(m3u8_url)
        .headers(headers.clone())
        .send()
        .await
        .map_err(MhError::Network)?;
    if !resp.status().is_success() {
        return Err(MhError::Network(reqwest::Error::from(
            resp.error_for_status().unwrap_err(),
        )));
    }
    let m3u8_text = resp.text().await.map_err(MhError::Network)?;
    let lines: Vec<&str> = m3u8_text.lines().collect();

    let base_url = {
        let idx = m3u8_url.rfind('/').map(|i| i + 1).unwrap_or(m3u8_url.len());
        &m3u8_url[..idx]
    };

    let (best_stream, _best_bw) = select_best_audio_stream(&lines, base_url);
    let stream_url = best_stream
        .ok_or_else(|| MhError::Other("No audio stream found in HLS master manifest".into()))?;

    let mut widevine_pssh: Option<String> = None;

    if let Some(ski_b64) =
        get_session_data_value(&m3u8_text, "com.apple.hls.AudioSessionKeyInfo")
    {
        use base64::Engine;
        if let Ok(ski_json) =
            base64::engine::general_purpose::STANDARD.decode(&ski_b64)
        {
            if let Ok(session_key_info) = serde_json::from_slice::<Value>(&ski_json) {
                if let Some(asset_meta_b64) =
                    get_session_data_value(&m3u8_text, "com.apple.hls.audioAssetMetadata")
                {
                    if let Ok(meta_json) =
                        base64::engine::general_purpose::STANDARD.decode(&asset_meta_b64)
                    {
                        if let Ok(asset_metadata) =
                            serde_json::from_slice::<Value>(&meta_json)
                        {
                            if let Some(obj) = asset_metadata.as_object() {
                                let best_base = stream_url
                                    .trim_end_matches(".m3u8")
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or("")
                                    .to_string();
                                let mut entries: Vec<&Value> = obj.values().collect();
                                entries.sort_by(|a, b| {
                                    let a_match = a["FIRST-SEGMENT-URI"]
                                        .as_str()
                                        .map(|s| s.contains(&best_base))
                                        .unwrap_or(false);
                                    let b_match = b["FIRST-SEGMENT-URI"]
                                        .as_str()
                                        .map(|s| s.contains(&best_base))
                                        .unwrap_or(false);
                                    b_match.cmp(&a_match)
                                });

                                'outer: for meta in entries {
                                    let key_ids = meta
                                        .get("AUDIO-SESSION-KEY-IDS")
                                        .or_else(|| meta.get("audio-session-key-ids"))
                                        .and_then(|v| v.as_array());
                                    if let Some(key_ids) = key_ids {
                                        for drm_id in key_ids {
                                            let drm_id_str =
                                                drm_id.as_str().unwrap_or("");
                                            if let Some(drm_map) =
                                                session_key_info.get(drm_id_str)
                                            {
                                                if let Some(obj) = drm_map.as_object() {
                                                    for (urn_key, entry) in obj {
                                                        if urn_key
                                                            .to_lowercase()
                                                            .contains("edef8ba9")
                                                        {
                                                            if let Some(uri) =
                                                                entry["URI"].as_str()
                                                            {
                                                                if !is_prefetch_pssh(uri) {
                                                                    widevine_pssh = Some(
                                                                        uri.to_string(),
                                                                    );
                                                                    break 'outer;
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if widevine_pssh.is_none() {
                    widevine_pssh =
                        find_widevine_in_session_key_info(&session_key_info);
                }
            }
        }
    }

    if widevine_pssh.is_none() {
        widevine_pssh = find_widevine_pssh_in_manifest(&m3u8_text);
    }

    if widevine_pssh.is_none() {
        let mut audio_rendition_urls: Vec<String> = Vec::new();
        for line in &lines {
            let t = line.trim();
            if !t.starts_with("#EXT-X-MEDIA:") || !t.to_lowercase().contains("type=audio") {
                continue;
            }
            if let Some(m) = regex::Regex::new(r#"URI="([^"]+)""#)
                .ok()
                .and_then(|re| re.captures(t))
                .and_then(|c| c.get(1))
            {
                let u = m.as_str();
                audio_rendition_urls.push(if u.starts_with("http") {
                    u.to_string()
                } else {
                    format!("{}{}", base_url, u)
                });
            }
        }

        let mut fetch_targets = vec![stream_url.clone()];
        fetch_targets.extend(audio_rendition_urls.into_iter().take(3));

        let plain_client = client.clone();
        let plain_ua = UA_CHROME_LATEST;

        for target_url in &fetch_targets {
            if widevine_pssh.is_some() {
                break;
            }
            let sub_resp = match client
                .get(target_url)
                .headers(headers.clone())
                .send()
                .await
            {
                Ok(r) => r,
                Err(_) => continue,
            };
            if !sub_resp.status().is_success() {
                continue;
            }
            let sub_text = match sub_resp.text().await {
                Ok(t) => t,
                Err(_) => continue,
            };
            let t_base = {
                let idx = target_url.rfind('/').map(|i| i + 1).unwrap_or(target_url.len());
                &target_url[..idx]
            };

            widevine_pssh = find_widevine_pssh_in_manifest(&sub_text);
            if widevine_pssh.is_some() {
                break;
            }

            if let Some(init_url) = get_init_segment_url(&sub_text, t_base) {
                if let Ok(init_resp) = plain_client
                    .get(&init_url)
                    .header(reqwest::header::USER_AGENT, plain_ua)
                    .send()
                    .await
                {
                    if init_resp.status().is_success() {
                        if let Ok(init_bytes) = init_resp.bytes().await {
                            if let Some(pssh_b64) = extract_pssh_from_mp4(&init_bytes) {
                                widevine_pssh = Some(format!(
                                    "data:text/plain;base64,{}",
                                    pssh_b64
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(HlsStreamInfo {
        stream_url,
        widevine_pssh,
        media_id: song_id.to_string(),
        legacy: false,
    })
}

fn select_best_audio_stream(lines: &[&str], base_url: &str) -> (Option<String>, u64) {
    let mut best_stream: Option<String> = None;
    let mut best_bw: u64 = 0;

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("#EXT-X-STREAM-INF:") {
            let bw = regex::Regex::new(r"AVERAGE-BANDWIDTH=(\d+)")
                .ok()
                .and_then(|re| re.captures(line))
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u64>().ok())
                .unwrap_or(0);
            let audio = regex::Regex::new(r#"AUDIO="([^"]+)""#)
                .ok()
                .and_then(|re| re.captures(line))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            if regex::Regex::new(r"^audio-stereo-\d+$")
                .ok()
                .map(|re| re.is_match(&audio))
                .unwrap_or(false)
                && bw > best_bw
            {
                best_bw = bw;
                if let Some(next) = lines.get(i + 1).map(|l| l.trim()) {
                    if !next.is_empty() && !next.starts_with('#') {
                        best_stream = Some(if next.starts_with("http") {
                            next.to_string()
                        } else {
                            format!("{}{}", base_url, next)
                        });
                    }
                }
            }
        }
        i += 1;
    }

    if best_stream.is_none() {
        best_bw = 0;
        i = 0;
        while i < lines.len() {
            let line = lines[i].trim();
            if line.starts_with("#EXT-X-STREAM-INF:") {
                let bw = regex::Regex::new(r"AVERAGE-BANDWIDTH=(\d+)")
                    .ok()
                    .and_then(|re| re.captures(line))
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u64>().ok())
                    .unwrap_or(0);
                if bw > best_bw {
                    best_bw = bw;
                    if let Some(next) = lines.get(i + 1).map(|l| l.trim()) {
                        if !next.is_empty() && !next.starts_with('#') {
                            best_stream = Some(if next.starts_with("http") {
                                next.to_string()
                            } else {
                                format!("{}{}", base_url, next)
                            });
                        }
                    }
                }
            }
            i += 1;
        }
    }

    (best_stream, best_bw)
}

async fn get_legacy_stream_info(
    client: &Client,
    song_id: &str,
    dev_token: &str,
    media_user_token: &str,
    cookies_path: &str,
) -> MhResult<HlsStreamInfo> {
    let data = get_web_playback(client, song_id, dev_token, media_user_token, cookies_path).await?;

    let assets = data["songList"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|s| s["assets"].as_array())
        .ok_or_else(|| {
            MhError::Other(format!(
                "No assets in webplayback for legacy stream (keys: {})",
                data.as_object()
                    .map(|o| o.keys().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            ))
        })?;

    let asset = assets
        .iter()
        .find(|a| a["flavor"].as_str() == Some("28:ctrp256"))
        .or_else(|| assets.iter().find(|a| a["flavor"].as_str() == Some("32:ctrp64")))
        .or_else(|| assets.iter().find(|a| a.get("URL").is_some()))
        .ok_or_else(|| {
            let flavors: Vec<&str> = assets
                .iter()
                .filter_map(|a| a["flavor"].as_str())
                .collect();
            MhError::Other(format!(
                "No legacy asset found (available flavors: {})",
                flavors.join(", ")
            ))
        })?;

    let stream_url = asset["URL"]
        .as_str()
        .ok_or_else(|| {
            MhError::Other(format!(
                "Legacy asset has no URL (flavor: {})",
                asset["flavor"].as_str().unwrap_or("unknown")
            ))
        })?
        .to_string();

    let m3u8_resp = client
        .get(&stream_url)
        .header(reqwest::header::USER_AGENT, UA_CHROME_LATEST)
        .send()
        .await
        .map_err(MhError::Network)?;
    if !m3u8_resp.status().is_success() {
        return Err(MhError::Other(format!(
            "Legacy m3u8 fetch failed: {}",
            m3u8_resp.status()
        )));
    }
    let m3u8_text = m3u8_resp.text().await.map_err(MhError::Network)?;

    for line in m3u8_text.lines() {
        let t = line.trim();
        if !t.starts_with("#EXT-X-KEY:") && !t.starts_with("#EXT-X-SESSION-KEY:") {
            continue;
        }
        if let Some(m) = regex::Regex::new(r#"URI="([^"]+)""#)
            .ok()
            .and_then(|re| re.captures(t))
            .and_then(|c| c.get(1))
        {
            return Ok(HlsStreamInfo {
                stream_url,
                widevine_pssh: Some(m.as_str().to_string()),
                media_id: song_id.to_string(),
                legacy: true,
            });
        }
    }

    let snippet = &m3u8_text[..m3u8_text.len().min(500)];
    Err(MhError::Other(format!(
        "No EXT-X-KEY found in legacy m3u8. Manifest snippet: {}",
        snippet.replace('\n', "\\n")
    )))
}

const WIDEVINE_PYTHON_SCRIPT: &str = r#"
import sys, json, base64, asyncio
from pywidevine import PSSH, Cdm, Device
from gamdl.api.apple_music_api import AppleMusicApi

args = json.loads(sys.stdin.readline())

async def get_key():
    api = await AppleMusicApi.create_from_netscape_cookies(args["cookies_path"])
    device = Device.load(args["wvd_path"])
    cdm = Cdm.from_device(device)
    session_id = cdm.open()
    try:
        pssh_b64 = args["pssh"].split(",")[-1]
        try:
            decoded = base64.b64decode(pssh_b64 + '==')
            if len(decoded) < 32:
                raise ValueError("raw key ID")
            pssh_obj = PSSH(pssh_b64)
        except Exception:
            from pywidevine.license_protocol_pb2 import WidevinePsshData
            key_id_bytes = base64.b64decode(pssh_b64)
            widevine_pssh_data = WidevinePsshData()
            widevine_pssh_data.algorithm = 1
            widevine_pssh_data.key_ids.append(key_id_bytes)
            pssh_obj = PSSH(widevine_pssh_data.SerializeToString())
        challenge = cdm.get_license_challenge(session_id, pssh_obj)
        challenge_b64 = base64.b64encode(challenge).decode()
        license_data = await api.get_license_exchange(
            track_id=str(args["song_id"]),
            track_uri=args["pssh"],
            challenge=challenge_b64,
        )
        cdm.parse_license(session_id, license_data["license"])
        keys = [k for k in cdm.get_keys(session_id) if k.type == "CONTENT"]
        if not keys:
            return {"error": "No content keys in license response"}
        kid_hex = keys[0].kid.hex if isinstance(keys[0].kid.hex, str) else keys[0].kid.hex()
        key_hex = keys[0].key.hex() if callable(keys[0].key.hex) else keys[0].key.hex
        return {"key": key_hex, "kid": kid_hex}
    finally:
        cdm.close(session_id)

try:
    result = asyncio.run(get_key())
    sys.stdout.write(json.dumps(result) + "\n")
except Exception as e:
    sys.stdout.write(json.dumps({"error": str(e)}) + "\n")
    sys.exit(1)
"#;

async fn get_widevine_key_via_python(
    pssh: &str,
    wvd_path: &Path,
    song_id: &str,
    cookies_path: &str,
) -> MhResult<String> {
    let python = venv_manager::get_venv_python();
    let input = serde_json::to_string(&json!({
        "pssh": pssh,
        "wvd_path": wvd_path.to_string_lossy(),
        "song_id": song_id,
        "cookies_path": cookies_path,
    }))
    .unwrap()
        + "\n";

    let mut child = tokio::process::Command::new(&python)
        .args(["-c", WIDEVINE_PYTHON_SCRIPT])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| MhError::Subprocess(format!("pywidevine spawn failed: {}", e)))?;

    if let Some(stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let mut stdin = stdin;
        stdin
            .write_all(input.as_bytes())
            .await
            .map_err(|e| MhError::Subprocess(format!("stdin write: {}", e)))?;
    }

    let output = tokio::time::timeout(
        Duration::from_secs(120),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| MhError::Subprocess("pywidevine timed out after 120s".into()))?
    .map_err(|e| MhError::Subprocess(format!("pywidevine wait: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout.lines().last().unwrap_or("").trim().to_string();

    if last_line.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MhError::Subprocess(format!(
            "pywidevine exited {} with no output. stderr: {}",
            output.status,
            &stderr[..stderr.len().min(300)]
        )));
    }

    let result: Value = serde_json::from_str(&last_line).map_err(|e| {
        MhError::Subprocess(format!(
            "Failed to parse pywidevine output: {} ({})",
            &last_line[..last_line.len().min(200)],
            e
        ))
    })?;

    if let Some(err) = result["error"].as_str() {
        return Err(MhError::Subprocess(format!("pywidevine error: {}", err)));
    }

    result["key"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| MhError::Subprocess("No key in pywidevine output".into()))
}

async fn decrypt_and_collect_hls(
    client: &Client,
    hls_info: HlsStreamInfo,
    wvd_path: &Path,
    dev_token: &str,
    media_user_token: &str,
    cookies_path: &str,
    duration_ms: u64,
    content_type: &str,
) -> MhResult<AppleTrackStream> {
    let is_video = content_type.starts_with("video/");

    let seg_headers = seg_fetch_headers(dev_token, media_user_token);
    let plain_ua = UA_CHROME_LATEST;

    let pssh = hls_info
        .widevine_pssh
        .as_deref()
        .ok_or_else(|| MhError::Other(format!(
            "No Widevine PSSH available {}",
            if hls_info.legacy { "(legacy fallback)" } else { "(primary)" }
        )))?;

    let key_fut = get_widevine_key_via_python(pssh, wvd_path, &hls_info.media_id, cookies_path);
    let seg_list_fut = fetch_segment_list(client, &hls_info.stream_url, &seg_headers);

    let (key_hex, (init_url, seg_urls)) = tokio::try_join!(key_fut, seg_list_fut)?;

    let init_bytes = client
        .get(&init_url)
        .header(reqwest::header::USER_AGENT, plain_ua)
        .send()
        .await
        .map_err(MhError::Network)?
        .bytes()
        .await
        .map_err(MhError::Network)?;

    if is_video {
        let mut seg_bufs: Vec<Bytes> = Vec::with_capacity(seg_urls.len());
        const BATCH: usize = 10;
        for chunk in seg_urls.chunks(BATCH) {
            let futs: Vec<_> = chunk
                .iter()
                .map(|u| {
                    let c = client.clone();
                    let u = u.clone();
                    async move {
                        c.get(&u)
                            .header(reqwest::header::USER_AGENT, plain_ua)
                            .send()
                            .await
                            .map_err(MhError::Network)?
                            .bytes()
                            .await
                            .map_err(MhError::Network)
                    }
                })
                .collect();
            let results = futures_util::future::try_join_all(futs).await?;
            seg_bufs.extend(results);
        }

        let total: usize = init_bytes.len() + seg_bufs.iter().map(|b| b.len()).sum::<usize>();
        let mut combined = Vec::with_capacity(total);
        combined.extend_from_slice(&init_bytes);
        for buf in &seg_bufs {
            combined.extend_from_slice(buf);
        }

        let decrypted = mp4decrypt::decrypt_mp4(&combined, &key_hex)?;

        let temp_dir = std::env::temp_dir().join("mediaharbor");
        let _ = std::fs::create_dir_all(&temp_dir);
        let dec_file = temp_dir.join(format!("apple_dec_{}.mp4", &hls_info.media_id));
        std::fs::write(&dec_file, &decrypted)
            .map_err(MhError::Io)?;

        let ffmpeg_bin = crate::venv_manager::resolve_ffmpeg();
        let ffmpeg_out = tokio::process::Command::new(&ffmpeg_bin)
            .args([
                "-y", "-loglevel", "error",
                "-i", dec_file.to_str().unwrap_or(""),
                "-c", "copy", "-f", "mpegts", "pipe:1",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
            .map_err(|e| MhError::Subprocess(format!("ffmpeg remux: {}", e)))?;

        let _ = std::fs::remove_file(&dec_file);

        if !ffmpeg_out.status.success() {
            let err = String::from_utf8_lossy(&ffmpeg_out.stderr);
            return Err(MhError::Subprocess(format!("ffmpeg remux failed: {}", &err[..err.len().min(300)])));
        }

        return Ok(AppleTrackStream {
            data: Bytes::from(ffmpeg_out.stdout),
            content_type: "video/mp2t".to_string(),
            duration_ms,
        });
    }

    let mut state = mp4decrypt::create_decrypt_state(&init_bytes)?;
    let mut output: Vec<u8> = Vec::new();

    output.extend_from_slice(&state.header);

    const BATCH: usize = 4;
    for chunk in seg_urls.chunks(BATCH) {
        let futs: Vec<_> = chunk
            .iter()
            .map(|u| {
                let c = client.clone();
                let u = u.clone();
                async move {
                    c.get(&u)
                        .header(reqwest::header::USER_AGENT, plain_ua)
                        .send()
                        .await
                        .map_err(MhError::Network)?
                        .bytes()
                        .await
                        .map_err(MhError::Network)
                }
            })
            .collect();
        let seg_results = futures_util::future::try_join_all(futs).await?;
        for seg_bytes in seg_results {
            let decrypted = mp4decrypt::decrypt_segment_buf(&mut state, &seg_bytes, &key_hex)?;
            output.extend_from_slice(&decrypted);
        }
    }

    Ok(AppleTrackStream {
        data: Bytes::from(output),
        content_type: "audio/mp4".to_string(),
        duration_ms,
    })
}

async fn fetch_segment_list(
    client: &Client,
    stream_url: &str,
    headers: &HeaderMap,
) -> MhResult<(String, Vec<String>)> {
    let resp = client
        .get(stream_url)
        .headers(headers.clone())
        .send()
        .await
        .map_err(MhError::Network)?;
    if !resp.status().is_success() {
        return Err(MhError::Other(format!(
            "Failed to fetch stream m3u8: {}",
            resp.status()
        )));
    }
    let text = resp.text().await.map_err(MhError::Network)?;
    let base_url = {
        let idx = stream_url.rfind('/').map(|i| i + 1).unwrap_or(stream_url.len());
        &stream_url[..idx]
    };

    let mut init_url: Option<String> = None;
    let mut seg_urls: Vec<String> = Vec::new();

    for line in text.lines() {
        let t = line.trim();
        if let Some(m) = regex::Regex::new(r#"#EXT-X-MAP:URI="([^"]+)""#)
            .ok()
            .and_then(|re| re.captures(t))
            .and_then(|c| c.get(1))
        {
            let u = m.as_str();
            init_url = Some(if u.starts_with("http") {
                u.to_string()
            } else {
                format!("{}{}", base_url, u)
            });
            continue;
        }
        if !t.is_empty() && !t.starts_with('#') {
            seg_urls.push(if t.starts_with("http") {
                t.to_string()
            } else {
                format!("{}{}", base_url, t)
            });
        }
    }

    let init_url = init_url.ok_or_else(|| {
        MhError::Other(format!(
            "No EXT-X-MAP init segment in stream m3u8 (segments: {})",
            seg_urls.len()
        ))
    })?;

    if seg_urls.is_empty() {
        return Err(MhError::Other(
            "No media segments found in stream m3u8".into(),
        ));
    }

    Ok((init_url, seg_urls))
}

async fn get_gamdl_wvd_path(
    settings_wvd_path: &Option<String>,
    cache: &Arc<Mutex<WvdPathCache>>,
) -> MhResult<PathBuf> {
    if let Some(p) = settings_wvd_path {
        if !p.is_empty() {
            let pb = PathBuf::from(p);
            if pb.exists() {
                return Ok(pb);
            }
        }
    }

    {
        let c = cache.lock().unwrap();
        if let Some(p) = &c.path {
            if p.exists() {
                return Ok(p.clone());
            }
        }
    }

    let temp_dir = std::env::temp_dir().join("mediaharbor");
    let _ = std::fs::create_dir_all(&temp_dir);
    let wvd_file = temp_dir.join("apple_music.wvd");

    if wvd_file.exists() {
        let mut c = cache.lock().unwrap();
        c.path = Some(wvd_file.clone());
        return Ok(wvd_file);
    }

    let python = venv_manager::get_venv_python();
    let output = tokio::process::Command::new(&python)
        .args([
            "-c",
            "from gamdl.downloader.hardcoded_wvd import HARDCODED_WVD; \
             print(HARDCODED_WVD.strip())",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| MhError::Subprocess(format!("gamdl WVD extract failed: {}", e)))?;

    if output.status.success() {
        let b64 = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !b64.is_empty() {
            use base64::Engine;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(&b64)
                .map_err(|e| MhError::Crypto(format!("WVD base64 decode: {}", e)))?;
            std::fs::write(&wvd_file, &decoded).map_err(MhError::Io)?;
            let mut c = cache.lock().unwrap();
            c.path = Some(wvd_file.clone());
            return Ok(wvd_file);
        }
    }

    Err(MhError::Other(
        "WVD file required for Apple Music. Set it in Settings → Apple → WVD Path, \
         or ensure gamdl is installed in the venv."
            .into(),
    ))
}

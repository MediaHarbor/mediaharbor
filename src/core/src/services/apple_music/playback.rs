use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bytes::Bytes;
use regex::Regex;
use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, COOKIE, ORIGIN, REFERER},
    Client,
};
use serde_json::{json, Value};
use std::sync::LazyLock;

use crate::{
    defaults::Settings,
    drm::mp4decrypt,
    errors::{MhError, MhResult},
    http_client::{PLATFORM_HEADER, UA_CHROME_LATEST},
    services::common::ids::now_secs,
};

/// The HLS/TTML attribute patterns this module scans for, compiled once.
///
/// Every one of these was rebuilt on each call, several inside per-rendition and
/// per-segment loops, so parsing one playlist paid the regex compiler dozens of times.
static URI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"URI="([^"]+)""#).unwrap());
static GROUP_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"GROUP-ID="([^"]+)""#).unwrap());
static AUDIO_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"AUDIO="([^"]+)""#).unwrap());
static VALUE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"VALUE="([^"]+)""#).unwrap());
static BANDWIDTH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:AVERAGE-)?BANDWIDTH=(\d+)").unwrap());
static RESOLUTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"RESOLUTION=(\d+)x(\d+)").unwrap());
static CODECS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"CODECS="([^"]+)""#).unwrap());
static MAP_URI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"#EXT-X-MAP:URI="([^"]+)""#).unwrap());
static MAP_TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#EXT-X-MAP:(.+)$").unwrap());
static ATTR_BYTERANGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"BYTERANGE="(\d+)(?:@(\d+))?""#).unwrap());
static BYTERANGE_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#EXT-X-BYTERANGE:(\d+)(?:@(\d+))?$").unwrap());
static JWT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+)").unwrap()
});
static ASSETS_INDEX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/(assets/index[~-][^/\x22]+\.js)").unwrap());
static ASSETS_INDEX_LEGACY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/(assets/index-legacy[~-][^/\x22]+\.js)").unwrap());
static TTML_P_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)<p\b([^>]*)>(.*?)</p>"#).unwrap());
static TTML_SPAN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)<span\b([^>]*)>(.*?)</span>"#).unwrap());
static BEGIN_ATTR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bbegin="([^"]+)""#).unwrap());
static END_ATTR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bend="([^"]+)""#).unwrap());
static TAG_STRIP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());

use crate::services::apple_music::APPLE_MUSIC_HOMEPAGE;
const AMP_API_URL: &str = "https://amp-api.music.apple.com";
const WEBPLAYBACK_API_URL: &str =
    "https://play.itunes.apple.com/WebObjects/MZPlay.woa/wa/webPlayback";
const WEBPLAYBACK_LICENSE_URL: &str =
    "https://play.itunes.apple.com/WebObjects/MZPlay.woa/wa/acquireWebPlaybackLicense";
const WIDEVINE_SYSTEM_ID: [u8; 16] = [
    0xed, 0xef, 0x8b, 0xa9, 0x79, 0xd6, 0x4a, 0xce, 0xa3, 0xc8, 0x27, 0xdc, 0xd5, 0x1d, 0x21, 0xed,
];

pub struct AppleTrackStream {
    pub data: bytes::Bytes,
    pub content_type: String,
    pub duration_ms: u64,
    pub codec: String,
    /// The HLS audio group Apple actually served (`audio-stereo-256`,
    /// `audio-atmos-2768`…). A request can be silently downgraded when the
    /// rendition is missing from the account's variant set, so this — not the
    /// requested quality — is what the file gets named after.
    pub rendition: Option<String>,
}

/// A music video's two decrypted renditions, still to be muxed together.
#[derive(Debug, Clone, Default)]
pub struct AppleVideoDownload {
    pub video: bytes::Bytes,
    pub audio: bytes::Bytes,
}

#[derive(Debug, Clone, Default)]
pub struct DownloadLyrics {
    pub ttml: Option<String>,
    pub lrc: Option<String>,
    pub plain: Option<String>,
}

pub struct AppleSeekableStream {
    pub content_type: String,
    pub duration_ms: u64,
    pub total_size: u64,
    header: Bytes,
    seg_refs: Vec<SegRef>,
    seg_offsets: Vec<u64>,
    seg_dec_sizes: Vec<u32>,
    decrypt_state: Arc<Mutex<mp4decrypt::DecryptState>>,
    widevine_key_hex: String,
    key_mode: Arc<tokio::sync::OnceCell<AppleKeyMode>>,
    client: Client,
    cache: Arc<Mutex<std::collections::HashMap<usize, Bytes>>>,
    cache_order: Arc<Mutex<std::collections::VecDeque<usize>>>,
    seg_locks: Arc<Mutex<std::collections::HashMap<usize, Arc<tokio::sync::Mutex<()>>>>>,
}

#[derive(Clone, Copy, Debug)]
enum AppleKeyMode {
    DescIndexDispatch,
    SingleWidevine,
}

const APPLE_SEG_CACHE_CAP: usize = 4;

impl AppleSeekableStream {
    pub async fn read_range(&self, start: u64, end_inclusive: u64) -> MhResult<Bytes> {
        if start >= self.total_size {
            return Ok(Bytes::new());
        }
        let end_exclusive = end_inclusive.saturating_add(1).min(self.total_size);
        let mut out: Vec<u8> = Vec::with_capacity((end_exclusive - start) as usize);
        let mut cursor = start;

        let header_len = self.header.len() as u64;
        if cursor < header_len {
            let h_end = end_exclusive.min(header_len);
            out.extend_from_slice(&self.header[cursor as usize..h_end as usize]);
            cursor = h_end;
        }

        while cursor < end_exclusive {
            let idx = self.locate_segment(cursor)?;
            let seg_start = self.seg_offsets[idx];
            let seg_size = self.seg_dec_sizes[idx] as u64;
            let dec = self.get_decrypted_segment(idx).await?;
            let local_start = (cursor - seg_start) as usize;
            let seg_end_byte = seg_start + seg_size;
            let chunk_end = end_exclusive.min(seg_end_byte);
            let local_end = (chunk_end - seg_start) as usize;
            let local_end = local_end.min(dec.len());
            let local_start = local_start.min(local_end);
            out.extend_from_slice(&dec[local_start..local_end]);
            cursor = chunk_end;
        }

        Ok(Bytes::from(out))
    }

    fn locate_segment(&self, byte_pos: u64) -> MhResult<usize> {
        match self.seg_offsets.binary_search(&byte_pos) {
            Ok(idx) => Ok(idx),
            Err(0) => Err(MhError::Other(format!(
                "byte position {} precedes first segment",
                byte_pos
            ))),
            Err(idx) => Ok(idx - 1),
        }
    }

    async fn get_decrypted_segment(&self, idx: usize) -> MhResult<Bytes> {
        if let Some(b) = self.cache.lock().unwrap().get(&idx).cloned() {
            return Ok(b);
        }

        let lock = {
            let mut locks = self.seg_locks.lock().unwrap();
            locks
                .entry(idx)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let _g = lock.lock().await;

        if let Some(b) = self.cache.lock().unwrap().get(&idx).cloned() {
            return Ok(b);
        }

        let seg = &self.seg_refs[idx];
        let enc = fetch_seg_bytes(&self.client, seg, UA_CHROME_LATEST).await?;

        let mode = self.resolve_key_mode(&enc).await?;
        let dec_vec = {
            let mut state = self.decrypt_state.lock().unwrap();
            match mode {
                AppleKeyMode::DescIndexDispatch => {
                    mp4decrypt::decrypt_segment_buf(&mut state, &enc, &self.widevine_key_hex)?
                }
                AppleKeyMode::SingleWidevine => mp4decrypt::decrypt_segment_buf_single_key(
                    &mut state,
                    &enc,
                    &self.widevine_key_hex,
                )?,
            }
        };
        let dec = Bytes::from(dec_vec);

        {
            let mut cache = self.cache.lock().unwrap();
            let mut order = self.cache_order.lock().unwrap();
            cache.insert(idx, dec.clone());
            order.push_back(idx);
            while order.len() > APPLE_SEG_CACHE_CAP {
                if let Some(evict) = order.pop_front() {
                    cache.remove(&evict);
                }
            }
        }

        Ok(dec)
    }

    async fn resolve_key_mode(&self, first_seg: &[u8]) -> MhResult<AppleKeyMode> {
        if let Some(m) = self.key_mode.get() {
            return Ok(*m);
        }
        let chosen = self
            .trial_key_mode(first_seg)
            .unwrap_or(AppleKeyMode::DescIndexDispatch);
        let _ = self.key_mode.set(chosen);
        Ok(chosen)
    }

    fn trial_key_mode(&self, first_seg: &[u8]) -> Option<AppleKeyMode> {
        for mode in [
            AppleKeyMode::DescIndexDispatch,
            AppleKeyMode::SingleWidevine,
        ] {
            let mut trial = {
                let s = self.decrypt_state.lock().unwrap();
                s.clone_for_trial()
            };
            let dec = match mode {
                AppleKeyMode::DescIndexDispatch => {
                    mp4decrypt::decrypt_segment_buf(&mut trial, first_seg, &self.widevine_key_hex)
                }
                AppleKeyMode::SingleWidevine => mp4decrypt::decrypt_segment_buf_single_key(
                    &mut trial,
                    first_seg,
                    &self.widevine_key_hex,
                ),
            };
            let Ok(dec) = dec else { continue };
            if first_sample_looks_like_aac(&dec) {
                return Some(mode);
            }
        }
        None
    }
}

fn first_sample_looks_like_aac(decrypted_seg: &[u8]) -> bool {
    first_sample_looks_valid(decrypted_seg, "mp4a")
}

fn first_sample_looks_valid(decrypted_seg: &[u8], codec: &str) -> bool {
    let is_eac3 = matches!(codec, "ec-3" | "ec3" | "eac3");
    let mut off = 0;
    while off + 8 <= decrypted_seg.len() {
        let size = u32::from_be_bytes([
            decrypted_seg[off],
            decrypted_seg[off + 1],
            decrypted_seg[off + 2],
            decrypted_seg[off + 3],
        ]) as usize;
        if size < 8 || off + size > decrypted_seg.len() {
            return false;
        }
        let kind = &decrypted_seg[off + 4..off + 8];
        if kind == b"mdat" {
            let payload_start = off + 8;
            if payload_start >= decrypted_seg.len() {
                return false;
            }
            if is_eac3 {
                return decrypted_seg.len() >= payload_start + 2
                    && decrypted_seg[payload_start] == 0x0B
                    && decrypted_seg[payload_start + 1] == 0x77;
            }
            let b0 = decrypted_seg[payload_start];
            return matches!(b0, 0x00 | 0x01 | 0x20 | 0x21);
        }
        off += size;
    }
    false
}

struct HlsStreamInfo {
    stream_url: String,
    widevine_pssh: Option<String>,
    media_id: String,
    legacy: bool,
    codec: Option<String>,
    /// The audio group this URL belongs to, carried through so the caller can
    /// tell a downgrade from a hit.
    group: Option<String>,
}

/// One `#EXT-X-STREAM-INF` variant picked out of a master playlist.
struct SelectedRendition {
    url: String,
    group: String,
    bandwidth: u64,
}

struct TokenCache {
    token: Option<String>,
    expires_at: Option<Instant>,
}

impl TokenCache {
    const FALLBACK_TTL: Duration = Duration::from_secs(3600);
    const EXPIRY_SKEW: Duration = Duration::from_secs(300);

    fn new() -> Self {
        Self {
            token: None,
            expires_at: None,
        }
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
        let ttl = jwt_ttl(&token).unwrap_or(Self::FALLBACK_TTL);
        self.expires_at = Some(Instant::now() + ttl);
        self.token = Some(token);
    }

    fn clear(&mut self) {
        self.token = None;
        self.expires_at = None;
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
    dev_token_cache: Arc<Mutex<TokenCache>>,
    wvd_path_cache: Arc<Mutex<WvdPathCache>>,
}

impl AppleMusicService {
    pub fn new() -> Self {
        Self {
            configured: false,
            cookies_path: None,
            wvd_path: None,
            dev_token_cache: Arc::new(Mutex::new(TokenCache::new())),
            wvd_path_cache: Arc::new(Mutex::new(WvdPathCache::new())),
        }
    }

    pub fn from_settings(settings: &Settings) -> Self {
        let configured = !settings.apple_cookies_path.is_empty();
        Self {
            configured,
            cookies_path: if configured {
                Some(settings.apple_cookies_path.clone())
            } else {
                None
            },
            wvd_path: if settings.apple_wvd_path.is_empty() {
                None
            } else {
                Some(settings.apple_wvd_path.clone())
            },
            dev_token_cache: Arc::new(Mutex::new(TokenCache::new())),
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

    pub async fn fetch_lyrics(
        &self,
        url: &str,
    ) -> Option<(Option<String>, Option<String>, Option<String>)> {
        let cookies_path = self.cookies_path.as_deref()?;
        let parsed = crate::services::apple_music::meta::parse_apple_music_url(url)?;
        let song_id = parsed.content_id.clone();

        let media_user_token = get_media_user_token(cookies_path).ok()?;
        let dev_token = self.get_dev_token().await.ok()?;
        let client = build_apple_client().ok()?;

        let account_storefront =
            get_account_storefront(&client, &dev_token, &media_user_token).await;
        let storefront = account_storefront.unwrap_or(parsed.storefront);

        let lyrics_url = format!(
            "{}/v1/catalog/{}/songs/{}/syllable-lyrics",
            AMP_API_URL, storefront, song_id
        );
        let resp = client
            .get(&lyrics_url)
            .headers(apple_headers(&dev_token, &media_user_token))
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            return Some((None, None, None));
        }

        let body: serde_json::Value = resp.json().await.ok()?;
        let ttml_str = body["data"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|item| item["attributes"]["ttml"].as_str())
            .map(|s| s.to_string());

        if let Some(ref ttml) = ttml_str {
            let (synced, plain, word_synced) = parse_ttml_to_lrc(ttml);
            return Some((synced, plain, word_synced));
        }

        Some((None, None, None))
    }

    /// `Ok(None)` means the catalog genuinely has no lyrics for this song;
    /// `Err` means the lookup failed and the reason is worth showing the user.
    pub async fn fetch_download_lyrics(&self, url: &str) -> MhResult<Option<DownloadLyrics>> {
        let cookies_path = self.cookies_path.as_deref().ok_or_else(|| {
            MhError::Auth(
                "Apple Music cookies path not set — synced lyrics need a signed-in account".into(),
            )
        })?;
        let parsed = crate::services::apple_music::meta::parse_apple_music_url(url)
            .ok_or_else(|| MhError::Parse(format!("Could not parse Apple Music URL: {url}")))?;
        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;
        let client = build_apple_client()?;
        let storefront = get_account_storefront(&client, &dev_token, &media_user_token)
            .await
            .unwrap_or(parsed.storefront);

        for endpoint in ["syllable-lyrics", "lyrics"] {
            let lyrics_url = format!(
                "{AMP_API_URL}/v1/catalog/{storefront}/songs/{}/{endpoint}",
                parsed.content_id
            );
            let resp = client
                .get(&lyrics_url)
                .query(&[
                    ("l[lyrics]", "en-US"),
                    ("extend", "ttmlLocalizations"),
                    ("l[script]", "en-Latn"),
                ])
                .headers(apple_headers(&dev_token, &media_user_token))
                .send()
                .await?;
            let status = resp.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                continue;
            }
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(crate::services::common::http::api_error(
                    &format!(
                        "Apple Music {endpoint} lookup for song {}",
                        parsed.content_id
                    ),
                    status,
                    &body,
                ));
            }
            let body: serde_json::Value = resp.json().await?;
            let ttml = body["data"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|item| item["attributes"]["ttml"].as_str())
                .map(str::to_string);
            if let Some(ttml) = ttml {
                let (lrc, plain, _word) = parse_ttml_to_lrc(&ttml);
                return Ok(Some(DownloadLyrics {
                    ttml: Some(ttml),
                    lrc,
                    plain,
                }));
            }
        }
        Ok(None)
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

        let parsed = crate::services::apple_music::meta::parse_apple_music_url(url)
            .ok_or_else(|| MhError::Parse(format!("Could not parse Apple Music URL: {}", url)))?;
        let song_id = parsed.content_id.clone();

        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;

        let client = build_apple_client()?;

        let account_storefront =
            get_account_storefront(&client, &dev_token, &media_user_token).await;
        let storefront = account_storefront.unwrap_or_else(|| parsed.storefront.clone());

        if parsed.content_type == "music-video" {
            let hls_url = get_music_video_hls_url(
                &client,
                &song_id,
                &dev_token,
                &media_user_token,
                cookies_path,
            )
            .await?;
            let hls_info = parse_hls_for_stream(
                &client,
                &hls_url,
                &song_id,
                &dev_token,
                &media_user_token,
                None,
            )
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
                "mp4a",
                None,
            )
            .await;
        }

        let song_metadata = get_song_metadata(
            &client,
            &song_id,
            &storefront,
            &dev_token,
            &media_user_token,
        )
        .await?;
        let duration_ms = song_metadata["attributes"]["durationInMillis"]
            .as_u64()
            .unwrap_or(0);
        let m3u8_url = song_metadata["attributes"]["extendedAssetUrls"]["enhancedHls"]
            .as_str()
            .ok_or_else(|| missing_asset_error(&song_metadata, &song_id))?
            .to_string();

        let wvd_path = self.get_gamdl_wvd_path().await?;

        let final_hls_info = hls_info_with_legacy_fallback(
            &client,
            &m3u8_url,
            &song_id,
            &dev_token,
            &media_user_token,
            cookies_path,
            None,
        )
        .await?;

        decrypt_and_collect_hls(
            &client,
            final_hls_info,
            &wvd_path,
            &dev_token,
            &media_user_token,
            cookies_path,
            duration_ms,
            "audio/mp4",
            "mp4a",
            None,
        )
        .await
    }

    pub async fn get_song_catalog_metadata(&self, song_id: &str) -> MhResult<Value> {
        self.get_song_catalog_metadata_in(song_id, None).await
    }

    pub async fn get_song_catalog_metadata_in(
        &self,
        song_id: &str,
        storefront_hint: Option<&str>,
    ) -> MhResult<Value> {
        let cookies_path = self
            .cookies_path
            .as_deref()
            .ok_or_else(|| MhError::Auth("Apple Music cookies path not set".into()))?;
        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;
        let client = build_apple_client()?;
        let account_sf = get_account_storefront(&client, &dev_token, &media_user_token).await;
        let hint = storefront_hint
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let primary = hint
            .clone()
            .or_else(|| account_sf.clone())
            .unwrap_or_else(|| "us".to_string());
        match get_song_metadata(&client, song_id, &primary, &dev_token, &media_user_token).await {
            Ok(v) => Ok(v),
            Err(e) => {
                let fallback = account_sf.filter(|sf| *sf != primary);
                if let Some(sf) = fallback {
                    get_song_metadata(&client, song_id, &sf, &dev_token, &media_user_token).await
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Every song id in a catalog or library playlist, following Apple's `next`
    /// cursor so playlists longer than one page are not silently truncated.
    /// Returns the playlist name alongside the ids.
    pub async fn playlist_track_ids(
        &self,
        playlist_id: &str,
        storefront_hint: Option<&str>,
    ) -> MhResult<(String, Vec<String>)> {
        const MAX_PAGES: u32 = 200;

        let cookies_path = self
            .cookies_path
            .as_deref()
            .ok_or_else(|| MhError::Auth("Apple Music cookies path not set".into()))?;
        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;
        let client = build_apple_client()?;
        let storefront = storefront_hint
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or(get_account_storefront(&client, &dev_token, &media_user_token).await)
            .unwrap_or_else(|| "us".to_string());

        let is_library = playlist_id.starts_with('p') && !playlist_id.starts_with("pl.");
        let root = if is_library {
            format!("{AMP_API_URL}/v1/me/library/playlists/{playlist_id}")
        } else {
            format!("{AMP_API_URL}/v1/catalog/{storefront}/playlists/{playlist_id}")
        };

        let head: serde_json::Value = {
            let resp = client
                .get(&root)
                .headers(apple_headers(&dev_token, &media_user_token))
                .send()
                .await?;
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(crate::services::common::http::api_error(
                    &format!("Apple Music playlist {playlist_id}"),
                    status,
                    &body,
                ));
            }
            resp.json().await?
        };
        let name = head
            .pointer("/data/0/attributes/name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let mut ids: Vec<String> = Vec::new();
        let mut next = Some(format!("{root}/tracks?limit=100"));
        for _ in 0..MAX_PAGES {
            let Some(url) = next.take() else { break };
            let url = if url.starts_with("http") {
                url
            } else {
                format!("{AMP_API_URL}{url}")
            };
            let resp = client
                .get(&url)
                .headers(apple_headers(&dev_token, &media_user_token))
                .send()
                .await?;
            if !resp.status().is_success() {
                break;
            }
            let body: serde_json::Value = resp.json().await?;
            let items = body["data"].as_array().cloned().unwrap_or_default();
            for it in &items {
                let id = it
                    .pointer("/attributes/playParams/catalogId")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        it.pointer("/attributes/playParams/id")
                            .and_then(|v| v.as_str())
                    })
                    .or_else(|| it["id"].as_str());
                if let Some(id) = id.filter(|s| s.chars().all(|c| c.is_ascii_digit())) {
                    ids.push(id.to_string());
                }
            }
            next = body["next"].as_str().map(str::to_string);
        }

        if ids.is_empty() {
            return Err(MhError::NotFound(format!(
                "Apple Music playlist {playlist_id} has no downloadable catalog tracks"
            )));
        }
        Ok((name, ids))
    }

    /// The album's own attributes plus every track object on it, in track order.
    ///
    /// The legacy `itunes.apple.com/lookup` index this used to go through does not
    /// enumerate every release — it answers with the collection and no song rows for
    /// some albums — and it has no storefront parameter, so it always answered from
    /// the US store. The catalog API is the same source the album detail view already
    /// reads, so the tracklist on screen and the tracklist that gets downloaded agree.
    pub async fn album_catalog_tracks(
        &self,
        album_id: &str,
        storefront_hint: Option<&str>,
    ) -> MhResult<(Value, Vec<Value>)> {
        const MAX_PAGES: u32 = 100;

        let cookies_path = self
            .cookies_path
            .as_deref()
            .ok_or_else(|| MhError::Auth("Apple Music cookies path not set".into()))?;
        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;
        let client = build_apple_client()?;
        let storefront = storefront_hint
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or(get_account_storefront(&client, &dev_token, &media_user_token).await)
            .unwrap_or_else(|| "us".to_string());

        let root = format!("{AMP_API_URL}/v1/catalog/{storefront}/albums/{album_id}");

        let head: Value = {
            let resp = client
                .get(&root)
                .headers(apple_headers(&dev_token, &media_user_token))
                .send()
                .await?;
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(crate::services::common::http::api_error(
                    &format!("Apple Music album {album_id} (storefront {storefront})"),
                    status,
                    &body,
                ));
            }
            resp.json().await?
        };
        let album_attrs = head
            .pointer("/data/0/attributes")
            .cloned()
            .unwrap_or(Value::Null);

        let mut tracks: Vec<Value> = Vec::new();
        let mut next = Some(format!("{root}/tracks?limit=100"));
        for _ in 0..MAX_PAGES {
            let Some(url) = next.take() else { break };
            let url = if url.starts_with("http") {
                url
            } else {
                format!("{AMP_API_URL}{url}")
            };
            let resp = client
                .get(&url)
                .headers(apple_headers(&dev_token, &media_user_token))
                .send()
                .await?;
            if !resp.status().is_success() {
                break;
            }
            let body: Value = resp.json().await?;
            if let Some(items) = body["data"].as_array() {
                tracks.extend(items.iter().cloned());
            }
            next = body["next"].as_str().map(str::to_string);
        }

        if tracks.is_empty() {
            if let Some(items) = head
                .pointer("/data/0/relationships/tracks/data")
                .and_then(|v| v.as_array())
            {
                tracks.extend(items.iter().cloned());
            }
        }

        if tracks.is_empty() {
            return Err(MhError::NotFound(format!(
                "Apple Music album {album_id} has no catalog tracks in storefront {storefront}"
            )));
        }
        Ok((album_attrs, tracks))
    }

    /// Downloads a music video at the requested height and codec preference.
    /// Video and audio are separate encrypted renditions with their own Widevine
    /// keys, so both are fetched and decrypted before being muxed together.
    pub async fn get_music_video_for_download(
        &self,
        url: &str,
        max_height: u32,
        codec_priority: &[String],
        progress: Option<&crate::downloads::ByteProgress>,
    ) -> MhResult<AppleVideoDownload> {
        let cookies_path = self
            .cookies_path
            .as_deref()
            .ok_or_else(|| MhError::Auth("Apple Music cookies path not set".into()))?;
        let parsed = crate::services::apple_music::meta::parse_apple_music_url(url)
            .ok_or_else(|| MhError::Parse(format!("Could not parse Apple Music URL: {url}")))?;
        let video_id = parsed.content_id.clone();

        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;
        let client = build_apple_client()?;
        let storefront = get_account_storefront(&client, &dev_token, &media_user_token)
            .await
            .unwrap_or_else(|| parsed.storefront.clone());

        let hls_url = match music_video_master_url(&client, &video_id, &storefront).await {
            Ok(u) => u,
            Err(store_err) => get_music_video_hls_url(
                &client,
                &video_id,
                &dev_token,
                &media_user_token,
                cookies_path,
            )
            .await
            .map_err(|wp_err| {
                MhError::Other(format!(
                    "Could not resolve a stream for music video {video_id}. \
                     Store page: {store_err}. WebPlayback: {wp_err}"
                ))
            })?,
        };

        let master = client
            .get(&hls_url)
            .headers(seg_fetch_headers(&dev_token, &media_user_token))
            .send()
            .await?;
        if !master.status().is_success() {
            return Err(MhError::Other(format!(
                "Apple Music music-video manifest returned HTTP {}",
                master.status().as_u16()
            )));
        }
        let master_text = master.text().await?;
        let lines: Vec<&str> = master_text.lines().collect();
        let base_url = {
            let idx = hls_url.rfind('/').map(|i| i + 1).unwrap_or(hls_url.len());
            &hls_url[..idx]
        };

        let video_url = select_video_variant(&lines, base_url, max_height, codec_priority)
            .ok_or_else(|| {
                MhError::Other(format!(
                    "No video renditions in the Apple Music manifest for {video_id}"
                ))
            })?;
        let audio_url = select_audio_media_uri(&lines, base_url).ok_or_else(|| {
            MhError::Other(format!(
                "No audio rendition in the Apple Music manifest for {video_id}"
            ))
        })?;

        let wvd_path = self.get_gamdl_wvd_path().await?;
        let mut out = AppleVideoDownload::default();
        for (idx, (stream_url, content_type)) in
            [(video_url, "video/mp4"), (audio_url, "audio/mp4")]
                .into_iter()
                .enumerate()
        {
            let hls_info = HlsStreamInfo {
                stream_url,
                widevine_pssh: None,
                media_id: video_id.clone(),
                legacy: false,
                codec: None,
                group: None,
            };
            let hls_info =
                fill_widevine_pssh(&client, hls_info, &dev_token, &media_user_token).await?;
            let stream = decrypt_and_collect_hls(
                &client,
                hls_info,
                &wvd_path,
                &dev_token,
                &media_user_token,
                cookies_path,
                0,
                content_type,
                if idx == 0 { "avc1" } else { "mp4a" },
                progress.filter(|_| idx == 0),
            )
            .await?;
            if idx == 0 {
                out.video = stream.data;
            } else {
                out.audio = stream.data;
            }
        }
        Ok(out)
    }

    /// Downloads a rendition through a wrapper daemon. The wrapper's `_default`
    /// master is a superset of the web one and its renditions are FairPlay-licensed,
    /// so both the manifest and the sample decryption come from the daemon.
    pub async fn get_wrapper_stream_for_download(
        &self,
        url: &str,
        wrapper: &crate::services::apple_music::wrapper::WrapperClient,
        group_pattern: &str,
        codec_label: &str,
        progress: Option<&crate::downloads::ByteProgress>,
        on_log: Option<&(dyn Fn(String) + Send + Sync)>,
    ) -> MhResult<AppleTrackStream> {
        let log = |m: String| {
            if let Some(f) = on_log {
                f(m)
            }
        };
        use crate::drm::mp4decrypt;
        use crate::services::apple_music::wrapper as wrap;

        let cookies_path = self
            .cookies_path
            .as_deref()
            .ok_or_else(|| MhError::Auth("Apple Music cookies path not set".into()))?;
        let parsed = crate::services::apple_music::meta::parse_apple_music_url(url)
            .ok_or_else(|| MhError::Parse(format!("Could not parse Apple Music URL: {url}")))?;
        let song_id = parsed.content_id.clone();

        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;
        let client = build_apple_client()?;

        let playback = wrapper.playback(&song_id).await?;
        let master_url = wrap::master_url_from_playback(&playback).ok_or_else(|| {
            MhError::Other(format!(
                "wrapper-v2 returned no playable manifest for song {song_id}. If the track is \
                 listed on its release but greyed out or without a duration, Apple has no \
                 asset for it and no downloader can reach it."
            ))
        })?;

        log(format!("  wrapper master manifest: {master_url}"));

        let headers = seg_fetch_headers(&dev_token, &media_user_token);
        let master_text = client
            .get(&master_url)
            .headers(headers.clone())
            .send()
            .await?
            .error_for_status()
            .map_err(MhError::Network)?
            .text()
            .await?;
        let lines: Vec<&str> = master_text.lines().collect();
        let base_url = {
            let idx = master_url
                .rfind('/')
                .map(|i| i + 1)
                .unwrap_or(master_url.len());
            &master_url[..idx]
        };
        let selected =
            select_stream_by_group_pattern(&lines, base_url, group_pattern).ok_or_else(|| {
                MhError::Other(format!(
                    "No {codec_label} rendition (audio group {group_pattern}) in the Apple \
                     Music manifest for {song_id} — the account may not be entitled to it"
                ))
            })?;
        let stream_url = selected.url;

        let media_text = client
            .get(&stream_url)
            .headers(headers.clone())
            .send()
            .await?
            .error_for_status()
            .map_err(MhError::Network)?
            .text()
            .await?;
        let fallback_key = find_fairplay_key_uri(&media_text).ok_or_else(|| {
            MhError::Other(format!(
                "No FairPlay key in the {codec_label} rendition playlist for {song_id}"
            ))
        })?;
        let segment_keys = fairplay_key_uris_per_segment(&media_text);

        let (init_seg, seg_refs) = fetch_segment_list(&client, &stream_url, &headers).await?;
        log(format!(
            "  wrapper rendition {} → {} segment(s), {} distinct key(s)",
            selected.group,
            seg_refs.len(),
            segment_keys
                .iter()
                .flatten()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        ));
        let init_bytes = fetch_seg_bytes(&client, &init_seg, UA_CHROME_LATEST).await?;

        let moov = mp4decrypt::read_boxes(&init_bytes)
            .into_iter()
            .find(|(t, _, _, _)| t == "moov")
            .map(|(_, off, size, _)| init_bytes[off..off + size].to_vec())
            .ok_or_else(|| MhError::Other("init segment has no moov box".into()))?;

        if let Some(p) = progress {
            use std::sync::atomic::Ordering;
            let planned: u64 = seg_refs
                .iter()
                .map(|s| s.byterange.map(|(_, len)| len).unwrap_or(0))
                .sum();
            p.total.store(planned, Ordering::Relaxed);
            p.done.store(0, Ordering::Relaxed);
        }

        const BATCH: usize = 512;
        let mut session = wrapper.decrypt_session().await?;
        let mut fragments: Vec<mp4decrypt::Fragment> = Vec::new();
        for (i, seg) in seg_refs.iter().enumerate() {
            let bytes = fetch_seg_bytes(&client, seg, UA_CHROME_LATEST).await?;
            let seg_bytes = bytes.len() as u64;
            let key_uri = segment_keys
                .get(i)
                .and_then(|k| k.clone())
                .unwrap_or_else(|| fallback_key.clone());

            let mut seg_frags = mp4decrypt::extract_fragments_with_samples(&bytes, &moov);
            let seg_samples: usize = seg_frags.iter().map(|f| f.samples.len()).sum();
            log(format!(
                "  seg {}/{}: requested {} bytes, got {}, {} fragment(s), {} sample(s)",
                i + 1,
                seg_refs.len(),
                seg.byterange
                    .map(|(_, l)| l.to_string())
                    .unwrap_or_else(|| "all".into()),
                bytes.len(),
                seg_frags.len(),
                seg_samples
            ));
            if seg_samples == 0 {
                return Err(MhError::Other(format!(
                    "segment {} of {} yielded no audio samples from {} byte(s) — the \
                     fragment layout did not parse, so the rip would be silently truncated",
                    i + 1,
                    seg_refs.len(),
                    bytes.len()
                )));
            }
            for frag in seg_frags.iter_mut() {
                let mut idx = 0usize;
                while idx < frag.samples.len() {
                    let end = (idx + BATCH).min(frag.samples.len());
                    let plans: Vec<wrap::SamplePlan> = frag.samples[idx..end]
                        .iter()
                        .map(|s| {
                            let subs: Vec<(usize, usize)> = s
                                .subsamples
                                .iter()
                                .map(|(c, e)| (*c as usize, *e as usize))
                                .collect();
                            wrap::plan_sample(&s.data, &subs)
                        })
                        .collect();
                    let payload: Vec<Vec<u8>> = plans
                        .iter()
                        .filter(|p| !p.encrypted.is_empty())
                        .map(|p| p.encrypted.clone())
                        .collect();
                    let plains = session
                        .decrypt_samples(&song_id, &key_uri, &payload)
                        .await?;

                    let mut plain_iter = plains.into_iter();
                    for (offset, plan) in plans.iter().enumerate() {
                        let sample = &mut frag.samples[idx + offset];
                        let plain = if plan.encrypted.is_empty() {
                            Vec::new()
                        } else {
                            plain_iter.next().ok_or_else(|| {
                                MhError::Other("wrapper-v2 returned too few plaintexts".into())
                            })?
                        };
                        sample.data = wrap::reassemble_sample(&sample.data, plan, &plain)?;
                    }
                    idx = end;
                }
            }
            fragments.extend(seg_frags);

            if let Some(p) = progress {
                use std::sync::atomic::Ordering;
                p.done.fetch_add(seg_bytes, Ordering::Relaxed);
            }
        }
        session.close().await;

        let sample_total: usize = fragments.iter().map(|f| f.samples.len()).sum();
        log(format!(
            "  wrapper decrypt done: {} fragment(s), {sample_total} sample(s)",
            fragments.len()
        ));
        if fragments.is_empty() || sample_total == 0 {
            return Err(MhError::Other(format!(
                "The {codec_label} rendition produced {} fragment(s) and {sample_total} samples \
                 to decrypt — nothing to download",
                fragments.len()
            )));
        }

        let out = mp4decrypt::build_non_fragmented_m4a(&fragments, &moov)?;

        Ok(AppleTrackStream {
            data: Bytes::from(out),
            content_type: "audio/mp4".to_string(),
            duration_ms: 0,
            codec: codec_label.to_ascii_lowercase(),
            rendition: Some(selected.group),
        })
    }

    pub async fn debug_audio_variants(
        &self,
        song_id: &str,
    ) -> MhResult<(String, String, Vec<String>)> {
        let cookies_path = self
            .cookies_path
            .as_deref()
            .ok_or_else(|| MhError::Auth("Apple Music cookies path not set".into()))?;
        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;
        let client = build_apple_client()?;
        let storefront = get_account_storefront(&client, &dev_token, &media_user_token)
            .await
            .unwrap_or_else(|| "us".to_string());

        let song_metadata =
            get_song_metadata(&client, song_id, &storefront, &dev_token, &media_user_token).await?;
        let m3u8_url = song_metadata["attributes"]["extendedAssetUrls"]["enhancedHls"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let master_text = if m3u8_url.is_empty() {
            String::new()
        } else {
            let headers = seg_fetch_headers(&dev_token, &media_user_token);
            let resp = client
                .get(&m3u8_url)
                .headers(headers)
                .send()
                .await
                .map_err(MhError::Network)?;
            crate::services::common::http::read_body("Apple Music", resp).await?
        };

        let mut flavors = Vec::new();
        if let Ok(data) = get_web_playback(
            &client,
            song_id,
            &dev_token,
            &media_user_token,
            cookies_path,
        )
        .await
        {
            if let Some(assets) = data["songList"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|s| s["assets"].as_array())
            {
                for a in assets {
                    if let Some(f) = a["flavor"].as_str() {
                        flavors.push(f.to_string());
                    }
                }
            }
        }

        Ok((m3u8_url, master_text, flavors))
    }

    pub async fn debug_try_codec_key(&self, song_id: &str, codec: &str) -> MhResult<String> {
        let cookies_path = self
            .cookies_path
            .as_deref()
            .ok_or_else(|| MhError::Auth("Apple Music cookies path not set".into()))?;
        let wvd_path = self.get_gamdl_wvd_path().await?;
        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;
        let client = build_apple_client()?;
        let storefront = get_account_storefront(&client, &dev_token, &media_user_token)
            .await
            .unwrap_or_else(|| "us".to_string());
        let song_metadata =
            get_song_metadata(&client, song_id, &storefront, &dev_token, &media_user_token).await?;
        let m3u8_url = song_metadata["attributes"]["extendedAssetUrls"]["enhancedHls"]
            .as_str()
            .ok_or_else(|| MhError::Other("no enhancedHls in metadata".into()))?
            .to_string();
        let headers = seg_fetch_headers(&dev_token, &media_user_token);
        let master = client
            .get(&m3u8_url)
            .headers(headers.clone())
            .send()
            .await
            .map_err(MhError::Network)?
            .text()
            .await
            .map_err(MhError::Network)?;
        let base_url = {
            let idx = m3u8_url.rfind('/').map(|i| i + 1).unwrap_or(m3u8_url.len());
            &m3u8_url[..idx]
        };

        let lines: Vec<&str> = master.lines().collect();
        let want = codec.to_lowercase();
        let mut variant_url: Option<String> = None;
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim();
            if t.starts_with("#EXT-X-STREAM-INF:") && t.to_lowercase().contains(&want) {
                if let Some(next) = lines.get(i + 1).map(|l| l.trim()) {
                    if !next.is_empty() && !next.starts_with('#') {
                        variant_url = Some(if next.starts_with("http") {
                            next.to_string()
                        } else {
                            format!("{}{}", base_url, next)
                        });
                        break;
                    }
                }
            }
        }
        let variant_url =
            variant_url.ok_or_else(|| MhError::Other(format!("no variant with codec {codec}")))?;

        let vtext = client
            .get(&variant_url)
            .headers(headers.clone())
            .send()
            .await
            .map_err(MhError::Network)?
            .text()
            .await
            .map_err(MhError::Network)?;
        let vbase = {
            let idx = variant_url
                .rfind('/')
                .map(|i| i + 1)
                .unwrap_or(variant_url.len());
            &variant_url[..idx]
        };

        let pssh = if let Some(p) = find_widevine_pssh_in_manifest(&vtext) {
            p
        } else {
            let init_url = get_init_segment_url(&vtext, vbase)
                .ok_or_else(|| MhError::Other("no init segment in variant playlist".into()))?;
            let init_bytes = client
                .get(&init_url)
                .header(reqwest::header::USER_AGENT, UA_CHROME_LATEST)
                .send()
                .await
                .map_err(MhError::Network)?
                .bytes()
                .await
                .map_err(MhError::Network)?;
            let b64 = extract_pssh_from_mp4(&init_bytes)
                .ok_or_else(|| MhError::Other("no Widevine PSSH in variant init segment".into()))?;
            format!("data:text/plain;base64,{}", b64)
        };

        get_widevine_key(
            &pssh,
            &wvd_path,
            song_id,
            cookies_path,
            &dev_token,
            &media_user_token,
        )
        .await
    }

    pub async fn get_track_stream_for_download(
        &self,
        url: &str,
        flavor: &str,
        progress: Option<&crate::downloads::ByteProgress>,
    ) -> MhResult<AppleTrackStream> {
        let cookies_path = self
            .cookies_path
            .as_deref()
            .ok_or_else(|| MhError::Auth("Apple Music cookies path not set".into()))?;

        let parsed = crate::services::apple_music::meta::parse_apple_music_url(url)
            .ok_or_else(|| MhError::Parse(format!("Could not parse Apple Music URL: {}", url)))?;
        let song_id = parsed.content_id.clone();

        if parsed.content_type == "music-video" {
            return Err(MhError::Unsupported(
                "Native Apple Music downloader can't handle music videos. \
                 Switch to Gamdl in Settings → Apple Music → Downloader."
                    .into(),
            ));
        }

        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;
        let client = build_apple_client()?;

        let storefront = get_account_storefront(&client, &dev_token, &media_user_token)
            .await
            .unwrap_or_else(|| parsed.storefront.clone());

        let song_metadata = get_song_metadata(
            &client,
            &song_id,
            &storefront,
            &dev_token,
            &media_user_token,
        )
        .await?;
        let duration_ms = song_metadata["attributes"]["durationInMillis"]
            .as_u64()
            .unwrap_or(0);

        let wvd_path = self.get_gamdl_wvd_path().await?;

        let hls_info = get_legacy_stream_info_with_preference(
            &client,
            &song_id,
            &dev_token,
            &media_user_token,
            cookies_path,
            Some(flavor),
        )
        .await?;
        if hls_info.widevine_pssh.is_none() {
            return Err(MhError::Other(
                "No Widevine PSSH in legacy Apple Music HLS manifest".into(),
            ));
        }

        decrypt_and_collect_hls(
            &client,
            hls_info,
            &wvd_path,
            &dev_token,
            &media_user_token,
            cookies_path,
            duration_ms,
            "audio/mp4",
            "mp4a",
            progress,
        )
        .await
    }

    pub async fn get_track_stream_for_download_rendition(
        &self,
        url: &str,
        group_pattern: &str,
        progress: Option<&crate::downloads::ByteProgress>,
    ) -> MhResult<AppleTrackStream> {
        let cookies_path = self
            .cookies_path
            .as_deref()
            .ok_or_else(|| MhError::Auth("Apple Music cookies path not set".into()))?;

        let parsed = crate::services::apple_music::meta::parse_apple_music_url(url)
            .ok_or_else(|| MhError::Parse(format!("Could not parse Apple Music URL: {}", url)))?;
        let song_id = parsed.content_id.clone();

        if parsed.content_type == "music-video" {
            return Err(MhError::Unsupported(
                "Native Apple Music downloader can't handle music videos. \
                 Switch to Gamdl in Settings → Apple Music → Downloader."
                    .into(),
            ));
        }

        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;
        let client = build_apple_client()?;

        let storefront = get_account_storefront(&client, &dev_token, &media_user_token)
            .await
            .unwrap_or_else(|| parsed.storefront.clone());

        let song_metadata = get_song_metadata(
            &client,
            &song_id,
            &storefront,
            &dev_token,
            &media_user_token,
        )
        .await?;
        let duration_ms = song_metadata["attributes"]["durationInMillis"]
            .as_u64()
            .unwrap_or(0);
        let m3u8_url = song_metadata["attributes"]["extendedAssetUrls"]["enhancedHls"]
            .as_str()
            .ok_or_else(|| missing_asset_error(&song_metadata, &song_id))?
            .to_string();

        let wvd_path = self.get_gamdl_wvd_path().await?;

        let hls_info = hls_info_with_legacy_fallback(
            &client,
            &m3u8_url,
            &song_id,
            &dev_token,
            &media_user_token,
            cookies_path,
            Some(group_pattern),
        )
        .await?;

        let codec = match hls_info.codec.as_deref() {
            Some(c) if c.starts_with("ec-3") || c.starts_with("ec3") => "ec-3",
            Some("alac") => "alac",
            Some("ac-3") => "ac-3",
            _ => "mp4a",
        };
        let rendition = hls_info.group.clone();

        let mut stream = decrypt_and_collect_hls(
            &client,
            hls_info,
            &wvd_path,
            &dev_token,
            &media_user_token,
            cookies_path,
            duration_ms,
            "audio/mp4",
            codec,
            progress,
        )
        .await?;
        stream.rendition = rendition;
        Ok(stream)
    }

    pub async fn prepare_seekable_audio(&self, url: &str) -> MhResult<AppleSeekableStream> {
        let cookies_path = self
            .cookies_path
            .as_deref()
            .ok_or_else(|| MhError::Auth("Apple Music cookies path not set".into()))?;

        let parsed = crate::services::apple_music::meta::parse_apple_music_url(url)
            .ok_or_else(|| MhError::Parse(format!("Could not parse Apple Music URL: {}", url)))?;
        if parsed.content_type == "music-video" {
            return Err(MhError::Unsupported(
                "music videos use the non-seekable mpeg-ts path".into(),
            ));
        }
        let song_id = parsed.content_id.clone();

        let media_user_token = get_media_user_token(cookies_path)?;
        let dev_token = self.get_dev_token().await?;

        let client = build_apple_client()?;

        let account_sf = get_account_storefront(&client, &dev_token, &media_user_token).await;
        let primary_sf = if !parsed.storefront.is_empty() {
            parsed.storefront.clone()
        } else {
            account_sf.clone().unwrap_or_else(|| "us".into())
        };

        let song_metadata = match get_song_metadata(
            &client,
            &song_id,
            &primary_sf,
            &dev_token,
            &media_user_token,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => match account_sf.filter(|sf| *sf != primary_sf) {
                Some(sf) => {
                    get_song_metadata(&client, &song_id, &sf, &dev_token, &media_user_token).await?
                }
                None => return Err(e),
            },
        };
        let duration_ms = song_metadata["attributes"]["durationInMillis"]
            .as_u64()
            .unwrap_or(0);
        let m3u8_url = song_metadata["attributes"]["extendedAssetUrls"]["enhancedHls"]
            .as_str()
            .ok_or_else(|| missing_asset_error(&song_metadata, &song_id))?
            .to_string();

        let wvd_path = self.get_gamdl_wvd_path().await?;

        let hls_info = parse_hls_for_stream(
            &client,
            &m3u8_url,
            &song_id,
            &dev_token,
            &media_user_token,
            None,
        )
        .await?;
        let final_hls_info = if hls_info.widevine_pssh.is_none() {
            get_legacy_stream_info(
                &client,
                &song_id,
                &dev_token,
                &media_user_token,
                cookies_path,
            )
            .await?
        } else {
            hls_info
        };

        let pssh = final_hls_info
            .widevine_pssh
            .as_deref()
            .ok_or_else(|| MhError::Other("No Widevine PSSH in HLS manifest".into()))?;

        let seg_headers = seg_fetch_headers(&dev_token, &media_user_token);
        let key_fut = get_widevine_key(
            pssh,
            &wvd_path,
            &final_hls_info.media_id,
            cookies_path,
            &dev_token,
            &media_user_token,
        );
        let seg_list_fut = fetch_segment_list(&client, &final_hls_info.stream_url, &seg_headers);
        let (key_hex, (init_seg, seg_refs)) = tokio::try_join!(key_fut, seg_list_fut)?;

        let init_bytes = fetch_seg_bytes(&client, &init_seg, UA_CHROME_LATEST).await?;

        let mut state = mp4decrypt::create_decrypt_state(&init_bytes)?;
        // Before the header is captured: it shrinks, and every segment offset
        // below is counted from its length.
        mp4decrypt::single_sample_entry(&mut state);
        if duration_ms > 0 {
            mp4decrypt::patch_moov_duration(&mut state.header, duration_ms);
        }
        let header = Bytes::from(state.header.clone());

        const MOOF_PEEK: u64 = 16 * 1024;
        let peek_refs: Vec<SegRef> = seg_refs
            .iter()
            .map(|s| {
                let byterange = s.byterange.map(|(off, len)| (off, len.min(MOOF_PEEK)));
                SegRef {
                    url: s.url.clone(),
                    byterange,
                }
            })
            .collect();
        let mut peeks: Vec<Bytes> = Vec::with_capacity(peek_refs.len());
        const PEEK_BATCH: usize = 4;
        for chunk in peek_refs.chunks(PEEK_BATCH) {
            let futs = chunk
                .iter()
                .map(|s| fetch_seg_bytes(&client, s, UA_CHROME_LATEST));
            let part = futures_util::future::try_join_all(futs).await?;
            peeks.extend(part);
        }

        let mut seg_dec_sizes: Vec<u32> = Vec::with_capacity(seg_refs.len());
        let mut seg_offsets: Vec<u64> = Vec::with_capacity(seg_refs.len());
        let mut cursor: u64 = header.len() as u64;

        for (i, peek) in peeks.iter().enumerate() {
            if peek.len() < 8 {
                return Err(MhError::Other(format!(
                    "segment {} moof peek too small ({} bytes)",
                    i,
                    peek.len()
                )));
            }
            let moof_size = u32::from_be_bytes([peek[0], peek[1], peek[2], peek[3]]) as usize;
            if &peek[4..8] != b"moof" {
                return Err(MhError::Other(format!(
                    "segment {} does not start with moof box",
                    i
                )));
            }
            if moof_size == 0 || moof_size > peek.len() {
                return Err(MhError::Other(format!(
                    "segment {} moof ({}B) larger than peek window ({}B); raise MOOF_PEEK",
                    i,
                    moof_size,
                    peek.len()
                )));
            }
            let cleaned_moof = mp4decrypt::remove_senc_from_moof(&peek[..moof_size]);
            let enc_total = seg_refs[i]
                .byterange
                .map(|(_, len)| len as usize)
                .ok_or_else(|| {
                    MhError::Other("seekable audio path requires byterange m3u8".into())
                })?;
            let mdat_size = enc_total
                .checked_sub(moof_size)
                .ok_or_else(|| MhError::Other("encrypted segment shorter than moof".into()))?;
            let dec_size = cleaned_moof.len() + mdat_size;
            seg_offsets.push(cursor);
            seg_dec_sizes.push(dec_size as u32);
            cursor += dec_size as u64;
        }

        let total_size = cursor;

        Ok(AppleSeekableStream {
            content_type: "audio/mp4".to_string(),
            duration_ms,
            total_size,
            header,
            seg_refs,
            seg_offsets,
            seg_dec_sizes,
            decrypt_state: Arc::new(Mutex::new(state)),
            widevine_key_hex: key_hex,
            key_mode: Arc::new(tokio::sync::OnceCell::new()),
            client,
            cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
            cache_order: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            seg_locks: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }
}

impl Default for AppleMusicService {
    fn default() -> Self {
        Self::new()
    }
}

fn build_apple_client() -> MhResult<Client> {
    static CLIENT: std::sync::OnceLock<Option<Client>> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::ClientBuilder::new()
                .user_agent(UA_CHROME_LATEST)
                .redirect(reqwest::redirect::Policy::limited(10))
                .timeout(Duration::from_secs(30))
                .build()
                .ok()
        })
        .clone()
        .ok_or_else(|| MhError::Other("Apple Music HTTP client init failed".into()))
}

fn apple_headers(dev_token: &str, media_user_token: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    if let Ok(v) = HeaderValue::from_str(&format!("Bearer {}", dev_token)) {
        h.insert(AUTHORIZATION, v);
    }
    if let Ok(v) = HeaderValue::from_str(media_user_token) {
        h.insert("Media-User-Token", v);
    }
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
    h.insert(
        "sec-ch-ua-platform",
        HeaderValue::from_static(PLATFORM_HEADER),
    );
    h.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
    h.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    h.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
    h
}

fn seg_fetch_headers(dev_token: &str, media_user_token: &str) -> HeaderMap {
    let mut h = apple_headers(dev_token, media_user_token);
    if let Ok(v) = HeaderValue::from_str(&format!("media-user-token={}", media_user_token)) {
        h.insert(COOKIE, v);
    }
    h.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
    h
}

pub fn get_media_user_token(cookies_path: &str) -> MhResult<String> {
    let content = std::fs::read_to_string(cookies_path).map_err(MhError::Io)?;

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
        if parts.len() >= 7 && parts[5] == "media-user-token" && parts[0].contains("apple.com") {
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

pub async fn get_developer_token() -> MhResult<String> {
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

    let js_path = ASSETS_INDEX_LEGACY_RE
        .captures(&html)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .ok_or_else(|| {
            MhError::Parse("Could not find index.js URI in Apple Music homepage".into())
        })?;

    let js_url = format!("{}/{}", APPLE_MUSIC_HOMEPAGE, js_path);
    let js = client
        .get(&js_url)
        .send()
        .await
        .map_err(MhError::Network)?
        .text()
        .await
        .map_err(MhError::Network)?;

    let token = JWT_RE
        .captures(&js)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        .ok_or_else(|| {
            MhError::Parse("Could not extract developer token from Apple Music JS".into())
        })?;

    Ok(token)
}

fn jwt_payload(token: &str) -> Option<Value> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64U, Engine};
    let payload = token.split('.').nth(1)?;
    let bytes = B64U.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn jwt_issuer(token: &str) -> Option<String> {
    let payload = jwt_payload(token)?;
    payload["iss"].as_str().map(str::to_string)
}

/// How long the token is still good for, less a safety margin so it cannot
/// expire between the cache hit and the request that carries it.
fn jwt_ttl(token: &str) -> Option<Duration> {
    let payload = jwt_payload(token)?;
    let exp = payload["exp"].as_u64()?;
    Some(
        Duration::from_secs(exp.saturating_sub(now_secs())).saturating_sub(TokenCache::EXPIRY_SKEW),
    )
}

fn webplay_token_cache() -> &'static Mutex<TokenCache> {
    static CACHE: std::sync::OnceLock<Mutex<TokenCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(TokenCache::new()))
}

/// Drops the memoised token so the next call re-scrapes. Called when Apple
/// rejects a license request, which is the only proof the token went stale
/// ahead of its own `exp`.
fn invalidate_webplay_token() {
    if let Ok(mut cache) = webplay_token_cache().lock() {
        cache.clear();
    }
}

pub async fn get_webplay_token() -> MhResult<String> {
    if let Ok(cache) = webplay_token_cache().lock() {
        if let Some(t) = cache.get() {
            return Ok(t.to_string());
        }
    }
    let token = scrape_webplay_token().await?;
    if let Ok(mut cache) = webplay_token_cache().lock() {
        cache.set(token.clone());
    }
    Ok(token)
}

async fn scrape_webplay_token() -> MhResult<String> {
    let client = build_apple_client()?;

    let html = client
        .get(APPLE_MUSIC_HOMEPAGE)
        .send()
        .await
        .map_err(MhError::Network)?
        .text()
        .await
        .map_err(MhError::Network)?;

    let re_js = &*ASSETS_INDEX_RE;
    let jwt_re = &*JWT_RE;

    for cap in re_js.captures_iter(&html) {
        let js_url = format!("{APPLE_MUSIC_HOMEPAGE}/{}", &cap[1]);
        let js = match client.get(&js_url).send().await {
            Ok(r) => r.text().await.unwrap_or_default(),
            Err(_) => continue,
        };
        for m in jwt_re.find_iter(&js) {
            let tok = m.as_str();
            if jwt_issuer(tok).as_deref() == Some("AMPWebPlay") {
                return Ok(tok.to_string());
            }
        }
    }
    Err(MhError::Parse(
        "Could not extract AMPWebPlay token from Apple Music JS".into(),
    ))
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
            status, text
        )));
    }

    let data: Value = resp.json().await.map_err(MhError::Network)?;
    data["data"]
        .as_array()
        .and_then(|arr| arr.first().cloned())
        .ok_or_else(|| {
            MhError::NotFound(format!(
                "Apple Music returned no song metadata for {}",
                song_id
            ))
        })
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
            status, text
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
    let data =
        get_web_playback(client, video_id, dev_token, media_user_token, cookies_path).await?;
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

/// Picks a music-video variant honouring the user's height cap and codec order.
/// Falls back to the smallest available variant when everything exceeds the cap,
/// so a 4K-only release still downloads instead of failing.
fn select_video_variant(
    lines: &[&str],
    base_url: &str,
    max_height: u32,
    codec_priority: &[String],
) -> Option<String> {
    let res_re = &*RESOLUTION_RE;
    let codec_re = &*CODECS_RE;
    let bw_re = &*BANDWIDTH_RE;

    let codec_rank = |codecs: &str| -> usize {
        let lower = codecs.to_ascii_lowercase();
        for (i, want) in codec_priority.iter().enumerate() {
            let want = want.trim().to_ascii_lowercase();
            let matches = match want.as_str() {
                "h264" | "avc" | "avc1" => lower.contains("avc1"),
                "h265" | "hevc" => lower.contains("hvc1") || lower.contains("hev1"),
                other => !other.is_empty() && lower.contains(other),
            };
            if matches {
                return i;
            }
        }
        codec_priority.len()
    };

    let mut best: Option<(usize, u32, u64, String)> = None;
    let mut smallest: Option<(u32, String)> = None;

    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if !line.starts_with("#EXT-X-STREAM-INF:") {
            continue;
        }
        let Some(next) = lines
            .get(i + 1)
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        else {
            continue;
        };
        let url = if next.starts_with("http") {
            next.to_string()
        } else {
            format!("{}{}", base_url, next)
        };
        let Some(height) = res_re
            .captures(line)
            .and_then(|c| c.get(2))
            .and_then(|m| m.as_str().parse::<u32>().ok())
        else {
            continue;
        };
        let bw = bw_re
            .captures(line)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<u64>().ok())
            .unwrap_or(0);
        let rank = codec_rank(
            codec_re
                .captures(line)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
                .unwrap_or(""),
        );

        if smallest.as_ref().map(|(h, _)| height < *h).unwrap_or(true) {
            smallest = Some((height, url.clone()));
        }
        if height > max_height {
            continue;
        }
        let key = (rank, std::cmp::Reverse(height), std::cmp::Reverse(bw));
        let better = match &best {
            None => true,
            Some((r, h, b, _)) => key < (*r, std::cmp::Reverse(*h), std::cmp::Reverse(*b)),
        };
        if better {
            best = Some((rank, height, bw, url));
        }
    }

    best.map(|(_, _, _, u)| u).or(smallest.map(|(_, u)| u))
}

const MUSIC_KIT_URL: &str = "https://music.apple.com/includes/js-cdn/musickit/v3/amp/musickit.js";

/// Apple's numeric storefront id (US = 143441), which the iTunes page endpoint
/// requires in its `X-Apple-Store-Front` header. MusicKit's own bundle is the
/// published mapping from a two-letter storefront to that id.
async fn get_storefront_id(client: &Client, storefront: &str) -> MhResult<u32> {
    let body = client
        .get(MUSIC_KIT_URL)
        .send()
        .await?
        .error_for_status()
        .map_err(MhError::Network)?
        .text()
        .await?;

    let upper = storefront.to_uppercase();
    let three = regex::Regex::new(&format!(r#"{}:"([A-Z]{{3}})""#, regex::escape(&upper)))
        .ok()
        .and_then(|re| re.captures(&body))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| {
            MhError::Other(format!(
                "storefront {storefront} not present in Apple's MusicKit bundle"
            ))
        })?;

    regex::Regex::new(&format!(r#"{}:"(\d+)""#, three))
        .ok()
        .and_then(|re| re.captures(&body))
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())
        .ok_or_else(|| {
            MhError::Other(format!(
                "no numeric storefront id for {storefront} ({three})"
            ))
        })
}

/// Music videos are not songs: the WebPlayback endpoint answers "This song is
/// currently unavailable" for them. The playable master manifest comes from the
/// iTunes store page instead.
async fn music_video_master_url(
    client: &Client,
    video_id: &str,
    storefront: &str,
) -> MhResult<String> {
    let storefront_id = get_storefront_id(client, storefront).await?;
    let resp = client
        .get(format!("https://music.apple.com/music-video/{video_id}"))
        .header(
            "X-Apple-Store-Front",
            format!("{storefront_id},32 t:music31"),
        )
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(crate::services::common::http::api_error(
            &format!("Apple Music store page for video {video_id}"),
            status,
            &body,
        ));
    }
    let page: Value = serde_json::from_str(&body).map_err(|e| {
        MhError::Parse(format!(
            "Apple Music store page for video {video_id} was not JSON: {e}"
        ))
    })?;
    let hls_url = page
        .pointer("/storePlatformData/product-dv/results")
        .and_then(|r| r.get(video_id))
        .and_then(|r| r.pointer("/offers/0/assets/0/hlsUrl"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            MhError::Other(format!(
                "Apple Music store page for video {video_id} carries no playable asset — \
                 it may not be available in the {storefront} storefront"
            ))
        })?;

    let mut parsed = url::Url::parse(hls_url)
        .map_err(|e| MhError::Parse(format!("bad Apple Music HLS url: {e}")))?;
    let existing: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(k, _)| k != "aec" && k != "dsid")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    {
        let mut q = parsed.query_pairs_mut();
        q.clear();
        for (k, v) in &existing {
            q.append_pair(k, v);
        }
        q.append_pair("aec", "HD");
        q.append_pair("dsid", "1");
    }
    Ok(parsed.to_string())
}

/// The audio for a music video lives in an `#EXT-X-MEDIA` rendition group, not in
/// the `#EXT-X-STREAM-INF` variant, so it has to be selected and decrypted
/// separately and muxed back in.
fn select_audio_media_uri(lines: &[&str], base_url: &str) -> Option<String> {
    let group_re = &*GROUP_ID_RE;
    let uri_re = &*URI_RE;
    let mut best: Option<(u32, String)> = None;

    for line in lines {
        let line = line.trim();
        if !line.starts_with("#EXT-X-MEDIA:") || !line.contains("TYPE=AUDIO") {
            continue;
        }
        let (Some(group), Some(uri)) = (
            group_re.captures(line).and_then(|c| c.get(1)),
            uri_re.captures(line).and_then(|c| c.get(1)),
        ) else {
            continue;
        };
        let group = group.as_str();
        let bitrate: u32 = group
            .rsplit('-')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let uri = uri.as_str();
        let full = if uri.starts_with("http") {
            uri.to_string()
        } else {
            format!("{}{}", base_url, uri)
        };
        if best.as_ref().map(|(b, _)| bitrate > *b).unwrap_or(true) {
            best = Some((bitrate, full));
        }
    }
    best.map(|(_, u)| u)
}

/// Reads the Widevine key URI out of a media playlist. Each music-video rendition
/// carries its own `#EXT-X-KEY`, so video and audio need separate licences.
async fn fill_widevine_pssh(
    client: &Client,
    mut info: HlsStreamInfo,
    dev_token: &str,
    media_user_token: &str,
) -> MhResult<HlsStreamInfo> {
    let text = client
        .get(&info.stream_url)
        .headers(seg_fetch_headers(dev_token, media_user_token))
        .send()
        .await?
        .error_for_status()
        .map_err(MhError::Network)?
        .text()
        .await?;
    info.widevine_pssh = find_widevine_pssh_in_manifest(&text);
    if info.widevine_pssh.is_none() {
        return Err(MhError::Other(format!(
            "No Widevine key in the Apple Music rendition playlist for {}",
            info.media_id
        )));
    }
    Ok(info)
}

/// Picks the highest-bandwidth variant whose `AUDIO="…"` group fully matches
/// `group_pattern`. Apple names a whole family per codec, so the pattern selects
/// the family and bandwidth breaks the tie — the same rule gamdl applies.
fn select_stream_by_group_pattern(
    lines: &[&str],
    base_url: &str,
    group_pattern: &str,
) -> Option<SelectedRendition> {
    let group_re = regex::Regex::new(&format!("^(?:{group_pattern})$")).ok()?;
    let audio_re = &*AUDIO_RE;
    let bw_re = &*BANDWIDTH_RE;
    let mut best: Option<SelectedRendition> = None;

    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if !line.starts_with("#EXT-X-STREAM-INF:") {
            continue;
        }
        let Some(group) = audio_re.captures(line).and_then(|c| c.get(1)) else {
            continue;
        };
        if !group_re.is_match(group.as_str()) {
            continue;
        }
        let Some(uri) = lines
            .get(i + 1)
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        else {
            continue;
        };
        let bw = bw_re
            .captures(line)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<u64>().ok())
            .unwrap_or(0);
        let full = if uri.starts_with("http") {
            uri.to_string()
        } else {
            format!("{}{}", base_url, uri)
        };
        if best.as_ref().map(|b| bw > b.bandwidth).unwrap_or(true) {
            best = Some(SelectedRendition {
                url: full,
                group: group.as_str().to_string(),
                bandwidth: bw,
            });
        }
    }
    best
}

/// The FairPlay key URI (`skd://…`) for a rendition. ALAC is licensed through
/// FairPlay, not Widevine, so this is what the wrapper needs alongside the samples.
/// Why a song has no `enhancedHls`. A catalog entry that carries no `playParams`
/// is listed on its release but has no asset behind it at all — that is a property
/// of the release, not of the account — so blaming the subscription sends the user
/// looking in the wrong place.
fn missing_asset_error(song_metadata: &Value, song_id: &str) -> MhError {
    let listed_but_unplayable = song_metadata
        .pointer("/attributes/playParams/id")
        .and_then(|v| v.as_str())
        .is_none_or(str::is_empty);
    if listed_but_unplayable {
        let name = song_metadata
            .pointer("/attributes/name")
            .and_then(|v| v.as_str())
            .unwrap_or(song_id);
        MhError::NotFound(format!(
            "Apple has no playable asset for {name} ({song_id}) — the track is listed \
             on its release but only a 30-second preview exists"
        ))
    } else {
        MhError::Other(format!(
            "Apple returned no HLS stream URL for song {song_id} even though the track is \
             playable. Check that your Apple Music subscription is active."
        ))
    }
}

fn find_fairplay_key_uri(text: &str) -> Option<String> {
    let uri_re = &*URI_RE;
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with("#EXT-X-KEY") && !t.starts_with("#EXT-X-SESSION-KEY") {
            continue;
        }
        if !t.contains("com.apple.streamingkeydelivery") {
            continue;
        }
        if let Some(uri) = uri_re.captures(t).and_then(|c| c.get(1)) {
            let uri = uri.as_str();
            if !is_prefetch_pssh(uri) {
                return Some(uri.to_string());
            }
        }
    }
    None
}

/// Apple ships a throwaway `…/s1/e1` key that only unlocks the first segment, so
/// that an unentitled client can still play a preview. A `skd://` URI carries that
/// marker in the clear and never survives base64 decoding, so the plain form has
/// to be matched directly — decoding alone silently accepts the preview key.
fn is_prefetch_pssh(uri: &str) -> bool {
    const NEEDLE: &[u8] = b"s1/e1";
    if uri.as_bytes().windows(NEEDLE.len()).any(|w| w == NEEDLE) {
        return true;
    }
    let b64 = if uri.starts_with("data:") {
        uri.split(',').next_back().unwrap_or(uri)
    } else {
        uri
    };
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map(|decoded| decoded.windows(NEEDLE.len()).any(|w| w == NEEDLE))
        .unwrap_or(false)
}

/// The FairPlay key in effect for each media segment, in playlist order.
///
/// `#EXT-X-KEY` applies to every segment that follows it until another replaces
/// it, and Apple rotates from the `s1/e1` preview key to the real track key after
/// the first segment. Taking only the first key decrypts segment one and leaves
/// the rest as noise, so each segment has to carry its own.
fn fairplay_key_uris_per_segment(text: &str) -> Vec<Option<String>> {
    let uri_re = &*URI_RE;
    let mut current: Option<String> = None;
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("#EXT-X-KEY") {
            if t.contains("com.apple.streamingkeydelivery") {
                current = uri_re
                    .captures(t)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string());
            }
            continue;
        }
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        out.push(current.clone());
    }
    out
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
                let b64 = base64::engine::general_purpose::STANDARD.encode(&data[off..off + size]);
                out.push(b64);
            }
        }
        if matches!(
            box_type,
            "moov" | "trak" | "mdia" | "minf" | "stbl" | "udta" | "moof" | "traf"
        ) {
            scan_mp4_for_pssh(data, off + 8, off + size, out);
        }
        off += size;
    }
}

fn get_session_data_value(text: &str, data_id: &str) -> Option<String> {
    let value_re = &*VALUE_RE;
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with("#EXT-X-SESSION-DATA:") {
            continue;
        }
        if !t.contains(data_id) {
            continue;
        }
        if let Some(m) = value_re.captures(t).and_then(|c| c.get(1)) {
            return Some(m.as_str().to_string());
        }
    }
    None
}

fn find_widevine_pssh_in_manifest(text: &str) -> Option<String> {
    let uri_re = &*URI_RE;
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with("#EXT-X-KEY") && !t.starts_with("#EXT-X-SESSION-KEY") {
            continue;
        }
        let lower = t.to_lowercase();
        if !lower.contains("edef8ba9") && !lower.contains("com.widevine") {
            continue;
        }
        if let Some(m) = uri_re.captures(t).and_then(|c| c.get(1)) {
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
    let map_uri_re = &*MAP_URI_RE;
    for line in playlist_text.lines() {
        if let Some(m) = map_uri_re.captures(line.trim()).and_then(|c| c.get(1)) {
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

/// Resolves the HLS info for a song, racing the legacy webplayback manifest so a
/// missing Widevine PSSH in the enhanced manifest still yields a usable stream.
/// Both the playback and the download paths go through here — keeping them apart is
/// what made tracks playable but not downloadable.
async fn hls_info_with_legacy_fallback(
    client: &Client,
    m3u8_url: &str,
    song_id: &str,
    dev_token: &str,
    media_user_token: &str,
    cookies_path: &str,
    group_pattern: Option<&str>,
) -> MhResult<HlsStreamInfo> {
    let legacy_client = client.clone();
    let legacy_song_id = song_id.to_string();
    let legacy_dev_token = dev_token.to_string();
    let legacy_media_user_token = media_user_token.to_string();
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

    let hls_info = parse_hls_for_stream(
        client,
        m3u8_url,
        song_id,
        dev_token,
        media_user_token,
        group_pattern,
    )
    .await?;

    if hls_info.widevine_pssh.is_some() {
        legacy_fut.abort();
        return Ok(hls_info);
    }

    let rendition = match group_pattern {
        Some(group) => format!(" for rendition {}", group),
        None => String::new(),
    };
    legacy_fut
        .await
        .map_err(|e| MhError::Other(format!("legacy stream task panic: {}", e)))?
        .map_err(|e| {
            MhError::Other(format!(
                "No Widevine PSSH in the Apple Music HLS manifest{} — legacy fallback failed: {}",
                rendition, e
            ))
        })
}

async fn parse_hls_for_stream(
    client: &Client,
    m3u8_url: &str,
    song_id: &str,
    dev_token: &str,
    media_user_token: &str,
    group_pattern: Option<&str>,
) -> MhResult<HlsStreamInfo> {
    let headers = seg_fetch_headers(dev_token, media_user_token);

    let resp = client
        .get(m3u8_url)
        .headers(headers.clone())
        .send()
        .await
        .map_err(MhError::Network)?;
    let m3u8_text = crate::services::common::http::read_body("Apple Music", resp).await?;
    let lines: Vec<&str> = m3u8_text.lines().collect();

    let base_url = {
        let idx = m3u8_url.rfind('/').map(|i| i + 1).unwrap_or(m3u8_url.len());
        &m3u8_url[..idx]
    };

    let selected = match group_pattern {
        Some(pattern) => select_stream_by_group_pattern(&lines, base_url, pattern)
            .or_else(|| select_best_audio_stream(&lines, base_url)),
        None => select_best_audio_stream(&lines, base_url),
    }
    .ok_or_else(|| MhError::Other("No audio stream found in HLS master manifest".into()))?;
    let selected_codec = Some(codec_for_audio_group(&selected.group).to_string());
    let selected_group = Some(selected.group.clone());
    let stream_url = selected.url;

    let mut widevine_pssh: Option<String> = None;

    if let Some(ski_b64) = get_session_data_value(&m3u8_text, "com.apple.hls.AudioSessionKeyInfo") {
        use base64::Engine;
        if let Ok(ski_json) = base64::engine::general_purpose::STANDARD.decode(&ski_b64) {
            if let Ok(session_key_info) = serde_json::from_slice::<Value>(&ski_json) {
                if let Some(asset_meta_b64) =
                    get_session_data_value(&m3u8_text, "com.apple.hls.audioAssetMetadata")
                {
                    if let Ok(meta_json) =
                        base64::engine::general_purpose::STANDARD.decode(&asset_meta_b64)
                    {
                        if let Ok(asset_metadata) = serde_json::from_slice::<Value>(&meta_json) {
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
                                            let drm_id_str = drm_id.as_str().unwrap_or("");
                                            if let Some(drm_map) = session_key_info.get(drm_id_str)
                                            {
                                                if let Some(obj) = drm_map.as_object() {
                                                    for (urn_key, entry) in obj {
                                                        if urn_key
                                                            .to_lowercase()
                                                            .contains("edef8ba9")
                                                        {
                                                            if let Some(uri) = entry["URI"].as_str()
                                                            {
                                                                if !is_prefetch_pssh(uri) {
                                                                    widevine_pssh =
                                                                        Some(uri.to_string());
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
                    widevine_pssh = find_widevine_in_session_key_info(&session_key_info);
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
            if let Some(m) = URI_RE.captures(t).and_then(|c| c.get(1)) {
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
            let sub_resp = match client.get(target_url).headers(headers.clone()).send().await {
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
                let idx = target_url
                    .rfind('/')
                    .map(|i| i + 1)
                    .unwrap_or(target_url.len());
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
                                widevine_pssh =
                                    Some(format!("data:text/plain;base64,{}", pssh_b64));
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
        codec: selected_codec,
        group: selected_group,
    })
}

/// The media codec an Apple audio group serves. The group id is authoritative —
/// `CODECS=` on the variant describes the whole variant, not just its audio.
fn codec_for_audio_group(group: &str) -> &'static str {
    if group.contains("alac") {
        "alac"
    } else if group.contains("atmos") {
        "ec-3"
    } else if group.contains("ac3") {
        "ac-3"
    } else {
        "mp4a"
    }
}

fn select_best_audio_stream(lines: &[&str], base_url: &str) -> Option<SelectedRendition> {
    select_stream_by_group_pattern(lines, base_url, r"audio-stereo-\d+")
        .or_else(|| select_stream_by_group_pattern(lines, base_url, r".*"))
}

async fn get_legacy_stream_info(
    client: &Client,
    song_id: &str,
    dev_token: &str,
    media_user_token: &str,
    cookies_path: &str,
) -> MhResult<HlsStreamInfo> {
    get_legacy_stream_info_with_preference(
        client,
        song_id,
        dev_token,
        media_user_token,
        cookies_path,
        None,
    )
    .await
}

async fn get_legacy_stream_info_with_preference(
    client: &Client,
    song_id: &str,
    dev_token: &str,
    media_user_token: &str,
    cookies_path: &str,
    preferred_flavor: Option<&str>,
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

    let preferred =
        preferred_flavor.and_then(|p| assets.iter().find(|a| a["flavor"].as_str() == Some(p)));

    let asset = preferred
        .or_else(|| {
            assets
                .iter()
                .find(|a| a["flavor"].as_str() == Some("28:ctrp256"))
        })
        .or_else(|| {
            assets
                .iter()
                .find(|a| a["flavor"].as_str() == Some("32:ctrp64"))
        })
        .or_else(|| assets.iter().find(|a| a.get("URL").is_some()))
        .ok_or_else(|| {
            let flavors: Vec<&str> = assets.iter().filter_map(|a| a["flavor"].as_str()).collect();
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
        if let Some(m) = URI_RE.captures(t).and_then(|c| c.get(1)) {
            return Ok(HlsStreamInfo {
                stream_url,
                widevine_pssh: Some(m.as_str().to_string()),
                media_id: song_id.to_string(),
                legacy: true,
                codec: None,
                group: None,
            });
        }
    }

    let snippet = m3u8_text;
    Err(MhError::Other(format!(
        "No EXT-X-KEY found in legacy m3u8. Manifest snippet: {}",
        snippet.replace('\n', "\\n")
    )))
}

async fn get_widevine_key(
    pssh: &str,
    wvd_path: &Path,
    song_id: &str,
    cookies_path: &str,
    dev_token: &str,
    media_user_token: &str,
) -> MhResult<String> {
    let _ = cookies_path;
    match get_widevine_key_native(pssh, wvd_path, song_id, dev_token, media_user_token).await {
        Ok(k) => {
            let _ = std::fs::write("/tmp/mh_cdm_debug.txt", "native OK\n");
            Ok(k)
        }
        Err(e) => {
            let _ = std::fs::write("/tmp/mh_cdm_debug.txt", format!("native FAILED:\n{e}\n"));
            Err(e)
        }
    }
}

async fn post_webplayback_license(
    client: &Client,
    webplay_token: &str,
    media_user_token: &str,
    body: &Value,
) -> MhResult<reqwest::Response> {
    client
        .post(WEBPLAYBACK_LICENSE_URL)
        .header("authorization", format!("Bearer {webplay_token}"))
        .header("cookie", format!("media-user-token={media_user_token}"))
        .header("content-type", "application/json")
        .header("origin", APPLE_MUSIC_HOMEPAGE)
        .json(body)
        .send()
        .await
        .map_err(MhError::Network)
}

async fn get_widevine_key_native(
    pssh: &str,
    wvd_path: &Path,
    song_id: &str,
    dev_token: &str,
    media_user_token: &str,
) -> MhResult<String> {
    use crate::drm::widevine_cdm::{build_challenge, make_request_id, WidevineDevice};
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    let wvd_bytes = tokio::fs::read(wvd_path).await.map_err(MhError::Io)?;
    let device = WidevineDevice::from_wvd_bytes(&wvd_bytes)?;

    let pssh_b64 = pssh.rsplit(',').next().unwrap_or(pssh);
    let pssh_raw = B64
        .decode(pssh_b64.trim())
        .map_err(|e| MhError::Parse(format!("pssh b64: {e}")))?;
    let init_data = crate::drm::widevine_cdm::pssh_init_data(&pssh_raw);

    let request_time = now_secs() as i64;
    let request_id = make_request_id(device.is_android(), request_time as u64);

    let client = build_apple_client()?;

    let (challenge, session) =
        build_challenge(&device, &init_data, &request_id, request_time, None)?;

    let body = json!({
        "challenge": B64.encode(&challenge),
        "key-system": "com.widevine.alpha",
        "uri": pssh,
        "adamId": song_id,
        "isLibrary": false,
        "user-initiated": true,
    });
    let mut resp = post_webplayback_license(
        &client,
        &get_webplay_token().await?,
        media_user_token,
        &body,
    )
    .await?;
    if matches!(resp.status().as_u16(), 401 | 403) {
        invalidate_webplay_token();
        resp = post_webplayback_license(
            &client,
            &get_webplay_token().await?,
            media_user_token,
            &body,
        )
        .await?;
    }
    let _ = dev_token;
    if !resp.status().is_success() {
        let s = resp.status().as_u16();
        let t = resp.text().await.unwrap_or_default();
        return Err(MhError::Other(format!(
            "acquireWebPlaybackLicense {s}: {}",
            t
        )));
    }
    let v: Value = resp.json().await.map_err(MhError::Network)?;
    if v["errorCode"].as_i64().unwrap_or(0) != 0 {
        return Err(MhError::Other(format!(
            "license errorCode {}",
            v["errorCode"]
        )));
    }
    let license_b64 = v["license"]
        .as_str()
        .ok_or_else(|| MhError::Other("no license in response".into()))?;
    let license = B64
        .decode(license_b64)
        .map_err(|e| MhError::Parse(format!("license b64: {e}")))?;

    let keys = crate::drm::widevine_cdm::parse_license(&device, &session, &license)?;
    let content = keys
        .iter()
        .find(|k| k.key_type == 2)
        .or_else(|| keys.first())
        .ok_or_else(|| MhError::Other("no content key in license".into()))?;
    Ok(hex::encode(&content.key))
}

#[allow(clippy::too_many_arguments)]
async fn decrypt_and_collect_hls(
    client: &Client,
    hls_info: HlsStreamInfo,
    wvd_path: &Path,
    dev_token: &str,
    media_user_token: &str,
    cookies_path: &str,
    duration_ms: u64,
    content_type: &str,
    expected_codec: &str,
    progress: Option<&crate::downloads::ByteProgress>,
) -> MhResult<AppleTrackStream> {
    let is_video = content_type.starts_with("video/");

    let seg_headers = seg_fetch_headers(dev_token, media_user_token);
    let plain_ua = UA_CHROME_LATEST;

    let pssh = hls_info.widevine_pssh.as_deref().ok_or_else(|| {
        MhError::Other(format!(
            "No Widevine PSSH available {}",
            if hls_info.legacy {
                "(legacy fallback)"
            } else {
                "(primary)"
            }
        ))
    })?;

    let key_fut = get_widevine_key(
        pssh,
        wvd_path,
        &hls_info.media_id,
        cookies_path,
        dev_token,
        media_user_token,
    );
    let seg_list_fut = fetch_segment_list(client, &hls_info.stream_url, &seg_headers);

    let (key_hex, (init_seg, seg_refs)) = tokio::try_join!(key_fut, seg_list_fut)?;

    let init_bytes = fetch_seg_bytes(client, &init_seg, plain_ua).await?;

    if is_video {
        let mut seg_bufs: Vec<Bytes> = Vec::with_capacity(seg_refs.len());
        const BATCH: usize = 10;
        for chunk in seg_refs.chunks(BATCH) {
            let futs: Vec<_> = chunk
                .iter()
                .map(|s| {
                    let c = client.clone();
                    let s = s.clone();
                    async move { fetch_seg_bytes(&c, &s, plain_ua).await }
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
        let dec_file = temp_dir.join(format!("apple_dec_{}.mp4", hls_info.media_id));
        std::fs::write(&dec_file, &decrypted).map_err(MhError::Io)?;

        let ffmpeg_bin = crate::venv_manager::resolve_ffmpeg();
        let mut ffmpeg_cmd = tokio::process::Command::new(&ffmpeg_bin);
        ffmpeg_cmd
            .args([
                "-y",
                "-loglevel",
                "error",
                "-i",
                dec_file.to_str().unwrap_or(""),
                "-c",
                "copy",
                "-f",
                "mpegts",
                "pipe:1",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        crate::subprocess::apply_no_window(&mut ffmpeg_cmd);
        let ffmpeg_out = ffmpeg_cmd
            .output()
            .await
            .map_err(|e| MhError::Subprocess(format!("ffmpeg remux: {}", e)))?;

        let _ = std::fs::remove_file(&dec_file);

        if !ffmpeg_out.status.success() {
            let err = String::from_utf8_lossy(&ffmpeg_out.stderr);
            return Err(MhError::Subprocess(format!("ffmpeg remux failed: {}", err)));
        }

        return Ok(AppleTrackStream {
            data: Bytes::from(ffmpeg_out.stdout),
            content_type: "video/mp2t".to_string(),
            duration_ms,
            codec: expected_codec.to_string(),
            rendition: None,
        });
    }

    let mut state = mp4decrypt::create_decrypt_state(&init_bytes)?;
    mp4decrypt::single_sample_entry(&mut state);
    if duration_ms > 0 {
        mp4decrypt::patch_moov_duration(&mut state.header, duration_ms);
    }
    let mut output: Vec<u8> = Vec::new();

    output.extend_from_slice(&state.header);

    if let Some(prog) = progress {
        use std::sync::atomic::Ordering;
        let total_enc: u64 = seg_refs
            .iter()
            .filter_map(|s| s.byterange.map(|(_, len)| len))
            .sum();
        prog.total.store(total_enc, Ordering::Relaxed);
        prog.done.store(0, Ordering::Relaxed);
    }
    let mut fetched: u64 = 0;
    let mut key_mode: Option<AppleKeyMode> = None;

    const BATCH: usize = 4;
    for chunk in seg_refs.chunks(BATCH) {
        let futs: Vec<_> = chunk
            .iter()
            .map(|s| {
                let c = client.clone();
                let s = s.clone();
                async move { fetch_seg_bytes(&c, &s, plain_ua).await }
            })
            .collect();
        let seg_results = futures_util::future::try_join_all(futs).await?;
        for seg_bytes in seg_results {
            fetched += seg_bytes.len() as u64;
            let mode = match key_mode {
                Some(m) => m,
                None => {
                    let resolved =
                        trial_key_mode_for_state(&state, &seg_bytes, &key_hex, expected_codec);
                    key_mode = Some(resolved);
                    resolved
                }
            };
            let decrypted = match mode {
                AppleKeyMode::DescIndexDispatch => {
                    mp4decrypt::decrypt_segment_buf(&mut state, &seg_bytes, &key_hex)?
                }
                AppleKeyMode::SingleWidevine => {
                    mp4decrypt::decrypt_segment_buf_single_key(&mut state, &seg_bytes, &key_hex)?
                }
            };
            output.extend_from_slice(&decrypted);
        }
        if let Some(prog) = progress {
            prog.done
                .store(fetched, std::sync::atomic::Ordering::Relaxed);
        }
    }

    Ok(AppleTrackStream {
        data: Bytes::from(output),
        content_type: "audio/mp4".to_string(),
        duration_ms,
        codec: expected_codec.to_string(),
        rendition: None,
    })
}

fn trial_key_mode_for_state(
    state: &mp4decrypt::DecryptState,
    first_seg: &[u8],
    key_hex: &str,
    codec: &str,
) -> AppleKeyMode {
    for mode in [
        AppleKeyMode::DescIndexDispatch,
        AppleKeyMode::SingleWidevine,
    ] {
        let mut trial = state.clone_for_trial();
        let dec = match mode {
            AppleKeyMode::DescIndexDispatch => {
                mp4decrypt::decrypt_segment_buf(&mut trial, first_seg, key_hex)
            }
            AppleKeyMode::SingleWidevine => {
                mp4decrypt::decrypt_segment_buf_single_key(&mut trial, first_seg, key_hex)
            }
        };
        if let Ok(dec) = dec {
            if first_sample_looks_valid(&dec, codec) {
                return mode;
            }
        }
    }
    AppleKeyMode::DescIndexDispatch
}

#[derive(Clone)]
struct SegRef {
    url: String,
    byterange: Option<(u64, u64)>,
}

async fn fetch_seg_bytes(client: &Client, seg: &SegRef, ua: &str) -> MhResult<Bytes> {
    let mut req = client.get(&seg.url).header(reqwest::header::USER_AGENT, ua);
    if let Some((off, len)) = seg.byterange {
        if len > 0 {
            req = req.header(
                reqwest::header::RANGE,
                format!("bytes={}-{}", off, off + len - 1),
            );
        }
    }
    let resp = req.send().await.map_err(MhError::Network)?;
    let status = resp.status();
    if !status.is_success() && status.as_u16() != 206 {
        return Err(MhError::Other(format!(
            "segment fetch {} for {}",
            status, seg.url
        )));
    }
    let bytes = resp.bytes().await.map_err(MhError::Network)?;
    if let Some((off, len)) = seg.byterange {
        if len > 0 && (bytes.len() as u64) < len {
            return Err(MhError::Other(format!(
                "segment fetch returned {} of the {len} bytes requested at offset {off} \
                 (HTTP {}) for {} — the server ignored or truncated the range request",
                bytes.len(),
                status.as_u16(),
                seg.url
            )));
        }
    }
    Ok(bytes)
}

async fn fetch_segment_list(
    client: &Client,
    stream_url: &str,
    headers: &HeaderMap,
) -> MhResult<(SegRef, Vec<SegRef>)> {
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
        let idx = stream_url
            .rfind('/')
            .map(|i| i + 1)
            .unwrap_or(stream_url.len());
        &stream_url[..idx]
    };

    let map_re = &*MAP_TAG_RE;
    let attr_uri_re = &*URI_RE;
    let attr_byterange_re = &*ATTR_BYTERANGE_RE;
    let byterange_tag_re = &*BYTERANGE_TAG_RE;

    let absolutize = |u: &str| -> String {
        if u.starts_with("http") {
            u.to_string()
        } else {
            format!("{}{}", base_url, u)
        }
    };

    let mut init: Option<SegRef> = None;
    let mut segs: Vec<SegRef> = Vec::new();
    let mut pending_byterange: Option<(u64, u64)> = None;
    let mut prev_seg_end: u64 = 0;

    for line in text.lines() {
        let t = line.trim();
        if let Some(cap) = map_re.captures(t) {
            let attrs = &cap[1];
            let uri = attr_uri_re
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .ok_or_else(|| MhError::Other("EXT-X-MAP missing URI attribute".into()))?;
            let byterange = attr_byterange_re.captures(attrs).map(|c| {
                let len: u64 = c[1].parse().unwrap_or(0);
                let off: u64 = c.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
                (off, len)
            });
            init = Some(SegRef {
                url: absolutize(&uri),
                byterange,
            });
            continue;
        }
        if let Some(cap) = byterange_tag_re.captures(t) {
            let len: u64 = cap[1].parse().unwrap_or(0);
            let off: u64 = cap
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(prev_seg_end);
            pending_byterange = Some((off, len));
            prev_seg_end = off + len;
            continue;
        }
        if !t.is_empty() && !t.starts_with('#') {
            segs.push(SegRef {
                url: absolutize(t),
                byterange: pending_byterange.take(),
            });
        }
    }

    let init = init.ok_or_else(|| {
        MhError::Other(format!(
            "No EXT-X-MAP init segment in stream m3u8 (segments: {})",
            segs.len()
        ))
    })?;

    if segs.is_empty() {
        return Err(MhError::Other(
            "No media segments found in stream m3u8".into(),
        ));
    }

    Ok((init, segs))
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

    Err(MhError::Other(
        "WVD file required for Apple Music. Set it in Settings → Apple → WVD Path.".into(),
    ))
}

pub(crate) fn parse_ttml_to_lrc(ttml: &str) -> (Option<String>, Option<String>, Option<String>) {
    let p_re = &*TTML_P_RE;
    let span_re = &*TTML_SPAN_RE;
    let begin_attr = &*BEGIN_ATTR_RE;
    let end_attr = &*END_ATTR_RE;
    let tag_strip = &*TAG_STRIP_RE;

    let mut lrc_lines = Vec::new();
    let mut plain_lines = Vec::new();
    let mut word_lines: Vec<serde_json::Value> = Vec::new();

    let mut paras: Vec<(f64, Option<f64>, String, Vec<serde_json::Value>)> = Vec::new();
    for cap in p_re.captures_iter(ttml) {
        let attrs = &cap[1];
        let inner = &cap[2];
        let begin = match begin_attr.captures(attrs).and_then(|c| c.get(1)) {
            Some(m) => m.as_str(),
            None => continue,
        };
        let p_end = end_attr
            .captures(attrs)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str());

        let text = tag_strip.replace_all(inner, "").trim().to_string();
        if !text.is_empty() {
            plain_lines.push(text.clone());
        }

        let span_caps: Vec<_> = span_re.captures_iter(inner).collect();
        let span_begins: Vec<Option<&str>> = span_caps
            .iter()
            .map(|sc| {
                begin_attr
                    .captures(&sc[1])
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str())
            })
            .collect();
        let mut words: Vec<serde_json::Value> = Vec::new();
        for (si, sc) in span_caps.iter().enumerate() {
            let sbegin = match span_begins[si] {
                Some(s) => s,
                None => continue,
            };
            let send = end_attr
                .captures(&sc[1])
                .and_then(|c| c.get(1))
                .map(|m| m.as_str());
            let stext = tag_strip.replace_all(&sc[2], "").trim().to_string();
            if stext.is_empty() {
                continue;
            }
            if let Some(start_s) = crate::services::common::library::parse_hms_seconds(sbegin) {
                let end_s = send
                    .and_then(crate::services::common::library::parse_hms_seconds)
                    .or_else(|| {
                        span_begins
                            .get(si + 1)
                            .and_then(|nb| *nb)
                            .and_then(crate::services::common::library::parse_hms_seconds)
                    })
                    .unwrap_or(start_s + 0.5);
                words.push(serde_json::json!({
                    "start": start_s,
                    "end": end_s,
                    "text": stext
                }));
            }
        }

        let line_start = crate::services::common::library::parse_hms_seconds(begin).unwrap_or(0.0);
        let line_end_real = p_end
            .and_then(crate::services::common::library::parse_hms_seconds)
            .or_else(|| words.last().and_then(|w| w["end"].as_f64()));
        paras.push((line_start, line_end_real, text, words));
    }

    let mut has_real_words = false;
    for (start, end_opt, text, words) in paras.iter() {
        lrc_lines.push(format!("{}{}", secs_to_lrc(*start), text));
        if !words.is_empty() {
            has_real_words = true;
        }
        let end_val = end_opt.unwrap_or(*start);
        word_lines.push(serde_json::json!({
            "startTime": *start,
            "endTime": end_val,
            "text": text,
            "words": words.clone(),
        }));
    }
    if !has_real_words {
        word_lines.clear();
    }

    let synced = if lrc_lines.is_empty() {
        None
    } else {
        Some(lrc_lines.join("\n"))
    };
    let plain = if plain_lines.is_empty() {
        None
    } else {
        Some(plain_lines.join("\n"))
    };
    let word_synced = if word_lines.is_empty() {
        None
    } else {
        serde_json::to_string(&word_lines).ok()
    };
    (synced, plain, word_synced)
}

fn secs_to_lrc(s: f64) -> String {
    let total_cs = (s * 100.0).round() as u64;
    let cs = total_cs % 100;
    let total_s = total_cs / 100;
    let secs = total_s % 60;
    let mins = total_s / 60;
    format!("[{:02}:{:02}.{:02}]", mins, secs, cs)
}

#[cfg(test)]
mod hls_selection_tests {
    use super::*;

    const MASTER: &str = concat!(
        "#EXTM3U\n",
        "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio-stereo-64\",URI=\"a64/prog.m3u8\"\n",
        "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio-stereo-256\",URI=\"a256/prog.m3u8\"\n",
        "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",URI=\"s/prog.m3u8\"\n",
        "#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"avc1.4d401e\"\n",
        "v360/prog.m3u8\n",
        "#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,CODECS=\"avc1.640028\"\n",
        "v1080/prog.m3u8\n",
        "#EXT-X-STREAM-INF:BANDWIDTH=4000000,RESOLUTION=1920x1080,CODECS=\"hvc1.2.4.L123\"\n",
        "v1080hevc/prog.m3u8\n",
        "#EXT-X-STREAM-INF:BANDWIDTH=9000000,RESOLUTION=3840x2160,CODECS=\"avc1.640033\"\n",
        "v2160/prog.m3u8\n",
    );

    fn lines() -> Vec<&'static str> {
        MASTER.lines().collect()
    }

    const AUDIO_MASTER: &str = concat!(
        "#EXTM3U\n",
        "#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=64000,AUDIO=\"audio-HE-stereo-64\"\n",
        "he64.m3u8\n",
        "#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=128000,AUDIO=\"audio-stereo-128\"\n",
        "s128.m3u8\n",
        "#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=256000,AUDIO=\"audio-stereo-256\"\n",
        "s256.m3u8\n",
        "#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=250000,AUDIO=\"audio-stereo-256-binaural\"\n",
        "bin.m3u8\n",
        "#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=900000,AUDIO=\"audio-alac-stereo-192\"\n",
        "alac192.m3u8\n",
        "#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=1400000,AUDIO=\"audio-alac-stereo-352\"\n",
        "alac352.m3u8\n",
    );

    #[test]
    fn a_codec_family_resolves_to_its_richest_member() {
        let lines: Vec<&str> = AUDIO_MASTER.lines().collect();
        assert_eq!(
            select_stream_by_group_pattern(&lines, "https://x/", r"audio-alac-.*")
                .unwrap()
                .url,
            "https://x/alac352.m3u8"
        );
        assert_eq!(
            select_stream_by_group_pattern(&lines, "https://x/", r"audio-stereo-\d+")
                .unwrap()
                .url,
            "https://x/s256.m3u8"
        );
    }

    /// Apple hands out an `s1/e1` key that only unlocks the first segment and
    /// rotates to the real track key for the rest. Decrypting everything with the
    /// first key leaves all but ~15 seconds of the track as noise.
    const ROTATING_KEY_MEDIA: &str = concat!(
        "#EXTM3U\n",
        "#EXT-X-TARGETDURATION:15\n",
        "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://itunes.apple.com/P000000000/s1/e1\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n",
        "#EXT-X-MAP:URI=\"track.mp4\",BYTERANGE=\"1037@0\"\n",
        "#EXTINF:14.95365,\n",
        "#EXT-X-BYTERANGE:941193@1037\n",
        "track.mp4\n",
        "#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://itunes.apple.com/p322170890/c6\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n",
        "#EXTINF:14.95365,\n",
        "#EXT-X-BYTERANGE:990106@942230\n",
        "track.mp4\n",
        "#EXTINF:14.95365,\n",
        "#EXT-X-BYTERANGE:1049661@1932336\n",
        "track.mp4\n",
    );

    #[test]
    fn every_segment_carries_the_key_in_effect_for_it() {
        let keys = fairplay_key_uris_per_segment(ROTATING_KEY_MEDIA);
        assert_eq!(keys.len(), 3, "one entry per media segment: {keys:?}");
        assert_eq!(
            keys[0].as_deref(),
            Some("skd://itunes.apple.com/P000000000/s1/e1")
        );
        for later in &keys[1..] {
            assert_eq!(
                later.as_deref(),
                Some("skd://itunes.apple.com/p322170890/c6"),
                "segments after the rotation must use the real track key"
            );
        }
    }

    #[test]
    fn the_ext_x_map_line_is_not_counted_as_a_segment() {
        let keys = fairplay_key_uris_per_segment(ROTATING_KEY_MEDIA);
        let segment_lines = ROTATING_KEY_MEDIA
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .count();
        assert_eq!(keys.len(), segment_lines);
    }

    #[test]
    fn the_preview_key_is_recognised_in_a_plain_skd_uri() {
        assert!(is_prefetch_pssh("skd://itunes.apple.com/P000000000/s1/e1"));
        assert!(!is_prefetch_pssh("skd://itunes.apple.com/p322170890/c6"));
    }

    #[test]
    fn the_family_pattern_does_not_swallow_its_variants() {
        let lines: Vec<&str> = AUDIO_MASTER.lines().collect();
        assert_eq!(
            select_stream_by_group_pattern(&lines, "https://x/", r"audio-stereo-\d+-binaural")
                .unwrap()
                .url,
            "https://x/bin.m3u8"
        );
    }

    #[test]
    fn a_bitrate_pinned_pattern_selects_exactly_that_rendition() {
        let lines: Vec<&str> = AUDIO_MASTER.lines().collect();
        assert_eq!(
            select_stream_by_group_pattern(&lines, "https://x/", r"audio-stereo-128")
                .unwrap()
                .url,
            "https://x/s128.m3u8"
        );
    }

    #[test]
    fn an_absent_family_yields_nothing_rather_than_a_wrong_rendition() {
        let lines: Vec<&str> = AUDIO_MASTER.lines().collect();
        assert!(select_stream_by_group_pattern(&lines, "https://x/", r"audio-ac3-.*").is_none());
    }

    #[test]
    fn the_audio_rendition_with_the_highest_bitrate_wins() {
        let uri = select_audio_media_uri(&lines(), "https://x/").expect("audio");
        assert_eq!(uri, "https://x/a256/prog.m3u8");
    }

    #[test]
    fn subtitle_renditions_are_not_mistaken_for_audio() {
        let only_subs = ["#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",URI=\"s.m3u8\""];
        assert!(select_audio_media_uri(&only_subs, "https://x/").is_none());
    }

    #[test]
    fn resolution_cap_is_respected_and_codec_order_breaks_ties() {
        let h264 = vec!["h264".to_string(), "h265".to_string()];
        let hevc = vec!["h265".to_string(), "h264".to_string()];
        assert_eq!(
            select_video_variant(&lines(), "https://x/", 1080, &h264).unwrap(),
            "https://x/v1080/prog.m3u8"
        );
        assert_eq!(
            select_video_variant(&lines(), "https://x/", 1080, &hevc).unwrap(),
            "https://x/v1080hevc/prog.m3u8"
        );
        assert_eq!(
            select_video_variant(&lines(), "https://x/", 720, &h264).unwrap(),
            "https://x/v360/prog.m3u8"
        );
    }

    #[test]
    fn a_manifest_above_the_cap_still_yields_its_smallest_variant() {
        let only_4k: Vec<&str> = MASTER
            .lines()
            .filter(|l| l.contains("2160") || l.starts_with("v2160") || l.starts_with("#EXTM3U"))
            .collect();
        assert_eq!(
            select_video_variant(&only_4k, "https://x/", 720, &["h264".to_string()]).unwrap(),
            "https://x/v2160/prog.m3u8"
        );
    }
}

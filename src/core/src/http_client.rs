use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT},
    Client, ClientBuilder, Response,
};
use serde::de::DeserializeOwned;
use std::time::Duration;

use crate::errors::{MhError, MhResult};

#[cfg(target_os = "windows")]
pub const UA_MOZILLA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[cfg(target_os = "macos")]
pub const UA_MOZILLA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const UA_MOZILLA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// High-version Chrome UA for APIs that check browser version
#[cfg(target_os = "windows")]
pub const UA_CHROME_LATEST: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

#[cfg(target_os = "macos")]
pub const UA_CHROME_LATEST: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const UA_CHROME_LATEST: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";

/// Platform string for sec-ch-ua-platform header
#[cfg(target_os = "windows")]
pub const PLATFORM_HEADER: &str = r#""Windows""#;

#[cfg(target_os = "macos")]
pub const PLATFORM_HEADER: &str = r#""macOS""#;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const PLATFORM_HEADER: &str = r#""Linux""#;

pub const UA_TIDAL_ANDROID: &str = "TIDAL/1099 okhttp/4.9.3";

pub const UA_SPOTIFY: &str = "Spotify/8.7.78.373 Android/31 (Pixel_3)";

fn base_builder() -> ClientBuilder {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(10))
        .gzip(true)
}

/// A client with the shared timeout / connect-timeout / redirect / gzip policy
/// and a caller-chosen user agent. Prefer this over a bare `Client::builder()`
/// so per-service clients cannot silently miss that policy.
pub fn build_ua_client(user_agent: &str) -> MhResult<Client> {
    base_builder()
        .user_agent(user_agent)
        .build()
        .map_err(MhError::Network)
}

pub fn build_mozilla_client() -> MhResult<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(UA_MOZILLA));
    base_builder()
        .default_headers(headers)
        .cookie_store(true)
        .build()
        .map_err(MhError::Network)
}

pub fn build_download_client() -> MhResult<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(UA_MOZILLA));
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(10))
        .gzip(true)
        .default_headers(headers)
        .build()
        .map_err(MhError::Network)
}

pub fn build_tidal_client() -> MhResult<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(UA_TIDAL_ANDROID));
    base_builder()
        .default_headers(headers)
        .build()
        .map_err(MhError::Network)
}

pub fn build_client() -> MhResult<Client> {
    base_builder().build().map_err(MhError::Network)
}

/// Short-timeout client for credential health probes, which must fail fast so a
/// bad credential is reported rather than left hanging behind the default timeout.
pub fn build_probe_client() -> MhResult<Client> {
    base_builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(MhError::Network)
}

/// Long-timeout client for streaming media bodies. Compression is off because the
/// payloads are already-compressed audio and video.
pub fn build_stream_client() -> MhResult<Client> {
    base_builder()
        .timeout(Duration::from_secs(300))
        .gzip(false)
        .build()
        .map_err(MhError::Network)
}

/// Process-wide client for one-off calls, so connection pooling and TLS setup are
/// shared instead of rebuilt per request.
pub fn shared_client() -> MhResult<Client> {
    static SHARED: std::sync::OnceLock<Option<Client>> = std::sync::OnceLock::new();
    SHARED
        .get_or_init(|| build_client().ok())
        .clone()
        .ok_or_else(|| MhError::Other("HTTP client init failed".into()))
}

/// Per-user-agent client, built once and cloned thereafter.
///
/// Building a `reqwest::Client` loads the TLS root store and starts with an
/// empty connection pool, so a fresh one per request re-does the handshake to a
/// host the previous call just finished talking to. Cloning is an `Arc` bump.
pub fn ua_client(user_agent: &'static str) -> MhResult<Client> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Client>>> = OnceLock::new();

    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(map) = cache.lock() {
        if let Some(c) = map.get(user_agent) {
            return Ok(c.clone());
        }
    }
    let client = build_ua_client(user_agent)?;
    if let Ok(mut map) = cache.lock() {
        map.insert(user_agent, client.clone());
    }
    Ok(client)
}

pub fn build_audio_client() -> MhResult<Client> {
    use reqwest::header::{HeaderMap as HMap, HeaderValue, ACCEPT_ENCODING};
    let mut headers = HMap::new();
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    Client::builder()
        .read_timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(10))
        .gzip(false)
        .deflate(false)
        .default_headers(headers)
        .build()
        .map_err(MhError::Network)
}

pub async fn fetch_json<T: DeserializeOwned>(client: &Client, url: &str) -> MhResult<T> {
    let res = client.get(url).send().await?;
    require_success(&res)?;
    Ok(res.json::<T>().await?)
}

pub fn require_success(res: &Response) -> MhResult<()> {
    if res.status().is_success() {
        Ok(())
    } else {
        Err(MhError::Network(res.error_for_status_ref().unwrap_err()))
    }
}

pub fn build_headers(pairs: &[(&str, &str)]) -> MhResult<HeaderMap> {
    let mut map = HeaderMap::new();
    for (k, v) in pairs {
        let name = HeaderName::from_bytes(k.as_bytes())?;
        let value = HeaderValue::from_str(v)?;
        map.insert(name, value);
    }
    Ok(map)
}

pub fn is_transient_http_error(e: &MhError) -> bool {
    let msg = e.to_string();
    msg.contains("HTTP 403")
        || msg.contains("HTTP 429")
        || msg.contains("HTTP 500")
        || msg.contains("HTTP 502")
        || msg.contains("HTTP 503")
        || msg.contains("HTTP 504")
}

pub fn is_transient(e: &MhError) -> bool {
    match e {
        MhError::Network(req) => match req.status() {
            Some(s) => s.as_u16() == 408 || s.as_u16() == 429 || s.is_server_error(),
            None => true,
        },
        _ => is_transient_http_error(e),
    }
}

pub async fn retry_transient<T, F, Fut>(
    max_attempts: u32,
    is_transient: impl Fn(&MhError) -> bool,
    mut f: F,
) -> MhResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = MhResult<T>>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if attempt >= max_attempts || !is_transient(&e) {
                    return Err(e);
                }
                let backoff = 100u64 * (1u64 << (attempt - 1));
                tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
            }
        }
    }
}

pub async fn download_to_file<F>(
    client: &Client,
    url: &str,
    dest: &std::path::Path,
    on_progress: F,
) -> MhResult<()>
where
    F: Fn(u64, Option<u64>),
{
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    const MAX_ATTEMPTS: u32 = 3;
    retry_transient(MAX_ATTEMPTS, is_transient, || async {
        let resp = client.get(url).send().await?;
        require_success(&resp)?;

        let total = resp.content_length();
        let mut stream = resp.bytes_stream();

        let mut file = tokio::fs::File::create(dest).await?;
        let mut downloaded: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            on_progress(downloaded, total);
        }

        Ok(())
    })
    .await
}

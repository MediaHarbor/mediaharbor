use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT},
    Client, ClientBuilder, Response,
};
use serde::de::DeserializeOwned;
use std::time::Duration;

use crate::errors::{MhError, MhResult};

pub const UA_MOZILLA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub const UA_TIDAL_ANDROID: &str =
    "TIDAL/1099 okhttp/4.9.3";

pub const UA_SPOTIFY: &str =
    "Spotify/8.7.78.373 Android/31 (Pixel_3)";

fn base_builder() -> ClientBuilder {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(10))
        .gzip(true)
}

pub fn build_mozilla_client() -> MhResult<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(UA_MOZILLA));
    base_builder()
        .default_headers(headers)
        .cookie_store(true)
        .build()
        .map_err(|e| MhError::Network(e))
}

pub fn build_tidal_client() -> MhResult<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(UA_TIDAL_ANDROID));
    base_builder()
        .default_headers(headers)
        .build()
        .map_err(|e| MhError::Network(e))
}

pub fn build_spotify_client() -> MhResult<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(UA_SPOTIFY));
    base_builder()
        .default_headers(headers)
        .build()
        .map_err(|e| MhError::Network(e))
}

pub fn build_client() -> MhResult<Client> {
    base_builder().build().map_err(|e| MhError::Network(e))
}

pub async fn fetch_json<T: DeserializeOwned>(client: &Client, url: &str) -> MhResult<T> {
    let res = client.get(url).send().await?;
    require_success(&res)?;
    Ok(res.json::<T>().await?)
}

pub async fn fetch_text(client: &Client, url: &str) -> MhResult<String> {
    let res = client.get(url).send().await?;
    require_success(&res)?;
    Ok(res.text().await?)
}

pub async fn fetch_bytes(client: &Client, url: &str) -> MhResult<bytes::Bytes> {
    let res = client.get(url).send().await?;
    require_success(&res)?;
    Ok(res.bytes().await?)
}

pub fn require_success(res: &Response) -> MhResult<()> {
    if res.status().is_success() {
        Ok(())
    } else {
        Err(MhError::Network(
            reqwest::Error::from(res.error_for_status_ref().unwrap_err()),
        ))
    }
}

pub fn build_headers(pairs: &[(&str, &str)]) -> MhResult<HeaderMap> {
    let mut map = HeaderMap::new();
    for (k, v) in pairs {
        let name = HeaderName::from_bytes(k.as_bytes())
            .map_err(|e| MhError::Other(e.to_string()))?;
        let value = HeaderValue::from_str(v)
            .map_err(|e| MhError::Other(e.to_string()))?;
        map.insert(name, value);
    }
    Ok(map)
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
}

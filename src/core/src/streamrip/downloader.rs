use std::path::Path;
use futures_util::StreamExt;
use reqwest::header::HeaderMap;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::errors::{MhError, MhResult};

pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    headers: Option<&HeaderMap>,
    on_progress: impl Fn(u64, u64),
) -> MhResult<()> {
    let mut req = client.get(url);
    if let Some(h) = headers {
        req = req.headers(h.clone());
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(MhError::Other(format!(
            "HTTP {} for {}",
            resp.status().as_u16(),
            url
        )));
    }

    let total = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let mut file = File::create(dest).await?;
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }

    Ok(())
}

pub async fn download_and_decrypt_deezer(
    client: &reqwest::Client,
    url: &str,
    track_id: &str,
    dest: &Path,
    on_progress: impl Fn(u64, u64),
) -> MhResult<()> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(MhError::Other(format!(
            "HTTP {} for Deezer stream",
            resp.status().as_u16()
        )));
    }

    let total = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = if total > 0 { Vec::with_capacity(total as usize) } else { Vec::new() };
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        downloaded += chunk.len() as u64;
        buf.extend_from_slice(&chunk);
        on_progress(downloaded, total);
    }

    let decrypted = crate::crypto::deezer::decrypt_buffer(track_id, &buf);
    tokio::fs::write(dest, &decrypted).await?;
    Ok(())
}

pub async fn download_segments(
    client: &reqwest::Client,
    urls: &[String],
    dest: &Path,
) -> MhResult<()> {
    let mut out = File::create(dest).await?;
    let total = urls.len() as u64;

    for (i, url) in urls.iter().enumerate() {
        let resp = client.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(MhError::Other(format!(
                "HTTP {} for segment {}",
                resp.status().as_u16(),
                i
            )));
        }
        let bytes = resp.bytes().await?;
        out.write_all(&bytes).await?;
        let _ = total; // suppress unused warning
    }

    Ok(())
}

use serde::{Deserialize, Serialize};

use crate::{errors::MhResult, http_client::build_mozilla_client};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpotifyTrackMeta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub thumbnail_url: Option<String>,
}

pub async fn get_track_meta(url: &str) -> MhResult<SpotifyTrackMeta> {
    let client = build_mozilla_client()?;

    let oembed_url = format!(
        "https://open.spotify.com/oembed?url={}",
        url_encode(url)
    );

    let (oembed_res, html_res) = tokio::join!(
        client.get(&oembed_url).send(),
        client.get(url).send()
    );

    let oembed_json: Option<serde_json::Value> = if let Ok(r) = oembed_res {
        r.json().await.ok()
    } else {
        None
    };

    let html: Option<String> = if let Ok(r) = html_res {
        r.text().await.ok()
    } else {
        None
    };

    let mut meta = SpotifyTrackMeta::default();

    if let Some(ref j) = oembed_json {
        meta.thumbnail_url = j["thumbnail_url"].as_str().map(str::to_string);
        meta.title = j["title"].as_str().map(str::to_string);
    }

    if let Some(ref html_str) = html {
        if meta.title.is_none() {
            meta.title = extract_og_meta(html_str, "og:title");
        }
        if let Some(desc) = extract_og_meta(html_str, "og:description") {
            let sep = '\u{00b7}'; // middle dot
            let parts: Vec<&str> = desc.split(sep).map(str::trim).collect();
            if parts.len() >= 3 {
                if meta.title.is_none() {
                    meta.title = Some(parts[0].to_string());
                }
                meta.artist = Some(parts[1].to_string());
                meta.album = Some(parts[2].to_string());
                if parts.len() >= 4 {
                    meta.year = Some(parts[3].to_string());
                }
            }
        }
    }

    if meta.artist.is_none() {
        if let Some(ref title_str) = meta.title.clone() {
            if title_str.contains(" - ") {
                let parts: Vec<&str> = title_str.splitn(2, " - ").collect();
                if parts.len() == 2 {
                    meta.artist = Some(parts[0].trim().to_string());
                    meta.title = Some(parts[1].trim().to_string());
                }
            }
        }
    }

    Ok(meta)
}

fn extract_og_meta(html: &str, property: &str) -> Option<String> {
    for pattern in &[
        format!(r#"<meta\s+property="{}"\s+content="([^"]+)""#, property),
        format!(r#"<meta\s+content="([^"]+)"\s+property="{}""#, property),
    ] {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(caps) = re.captures(html) {
                return caps.get(1).map(|m| m.as_str().to_string());
            }
        }
    }
    None
}

fn url_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

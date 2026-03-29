use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{
    errors::MhResult,
    http_client::{build_mozilla_client, fetch_json},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppleMusicUrlInfo {
    pub storefront: String,
    pub content_type: String,
    pub content_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppleMusicTrackInfo {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub cover_url: Option<String>,
}

pub fn parse_apple_music_url(url: &str) -> Option<AppleMusicUrlInfo> {
    let re = Regex::new(
        r"https://(?:classical\.)?music\.apple\.com/([a-z]{2})/(artist|album|playlist|song|music-video|post)/[^/]*(?:/([^/?]*))?(?:\?i=)?([0-9a-z]*)?"
    ).ok()?;

    let caps = re.captures(url)?;
    let storefront = caps.get(1)?.as_str().to_string();
    let resource_type = caps.get(2)?.as_str().to_string();
    let primary_id = caps.get(3).map(|m| m.as_str()).unwrap_or("");
    let sub_id = caps.get(4).map(|m| m.as_str()).unwrap_or("");

    let (content_id, content_type) = if !sub_id.is_empty() {
        (sub_id.to_string(), "song".to_string())
    } else {
        (primary_id.to_string(), resource_type)
    };

    if content_id.is_empty() {
        return None;
    }

    Some(AppleMusicUrlInfo {
        storefront,
        content_type,
        content_id,
    })
}

pub fn scale_artwork_url(url: &str, size: u32) -> String {
    let re = Regex::new(r"\d+x\d+(bb|cc|sr)").unwrap();
    re.replace(url, format!("{}x{}$1", size, size).as_str())
        .to_string()
}

pub async fn resolve_metadata(url: &str) -> MhResult<Option<AppleMusicTrackInfo>> {
    let parsed = match parse_apple_music_url(url) {
        Some(p) => p,
        None => return Ok(None),
    };

    if !parsed.content_id.chars().all(|c| c.is_ascii_digit()) {
        return Ok(None);
    }

    let client = build_mozilla_client()?;
    let lookup_url = format!(
        "https://itunes.apple.com/lookup?id={}&country={}&entity=song",
        parsed.content_id, parsed.storefront
    );

    let data: serde_json::Value = fetch_json(&client, &lookup_url).await?;
    let results = match data["results"].as_array() {
        Some(r) if !r.is_empty() => r,
        _ => return Ok(None),
    };

    let info = match parsed.content_type.as_str() {
        "song" => {
            let song = results
                .iter()
                .find(|r| {
                    r["wrapperType"].as_str() == Some("track")
                        && r["trackId"].to_string() == parsed.content_id
                })
                .or_else(|| {
                    results
                        .iter()
                        .find(|r| r["wrapperType"].as_str() == Some("track"))
                });
            song.map(|s| AppleMusicTrackInfo {
                title: s["trackName"].as_str().map(str::to_string),
                artist: s["artistName"].as_str().map(str::to_string),
                album: s["collectionName"].as_str().map(str::to_string),
                cover_url: s["artworkUrl100"]
                    .as_str()
                    .map(|u| scale_artwork_url(u, 1200)),
            })
        }
        "album" => {
            let album = results
                .iter()
                .find(|r| r["wrapperType"].as_str() == Some("collection"))
                .or_else(|| results.first());
            album.map(|a| AppleMusicTrackInfo {
                title: a["collectionName"].as_str().map(str::to_string),
                artist: a["artistName"].as_str().map(str::to_string),
                album: a["collectionName"].as_str().map(str::to_string),
                cover_url: a["artworkUrl100"]
                    .as_str()
                    .map(|u| scale_artwork_url(u, 1200)),
            })
        }
        _ => results.first().map(|f| AppleMusicTrackInfo {
            title: f["trackName"]
                .as_str()
                .or_else(|| f["collectionName"].as_str())
                .or_else(|| f["artistName"].as_str())
                .map(str::to_string),
            artist: f["artistName"].as_str().map(str::to_string),
            album: f["collectionName"].as_str().map(str::to_string),
            cover_url: f["artworkUrl100"]
                .as_str()
                .map(|u| scale_artwork_url(u, 1200)),
        }),
    };

    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_song_url() {
        let url = "https://music.apple.com/us/album/bohemian-rhapsody/1440806041?i=1440806768";
        let info = parse_apple_music_url(url).unwrap();
        assert_eq!(info.storefront, "us");
        assert_eq!(info.content_type, "song");
        assert_eq!(info.content_id, "1440806768");
    }

    #[test]
    fn scale_url() {
        let url = "https://is1-ssl.mzstatic.com/image/thumb/Music/a.jpg/100x100bb.jpg";
        let scaled = scale_artwork_url(url, 1200);
        assert!(scaled.contains("1200x1200bb"));
    }
}

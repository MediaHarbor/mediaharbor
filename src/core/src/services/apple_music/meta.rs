use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppleMusicUrlInfo {
    pub storefront: String,
    pub content_type: String,
    pub content_id: String,
}

fn normalize_for_parse(url: &str) -> String {
    let base = crate::services::apple_music::native_engine::normalize_apple_url(url);
    if base != url {
        return base;
    }
    let kinds = [
        "song",
        "album",
        "playlist",
        "artist",
        "music-video",
        "post",
        "audiobook",
    ];
    for prefix in [
        "https://music.apple.com/",
        "https://classical.music.apple.com/",
    ] {
        let Some(after) = url.strip_prefix(prefix) else {
            continue;
        };
        let mut parts = after.splitn(3, '/');
        let storefront = parts.next().unwrap_or("");
        let kind = parts.next().unwrap_or("");
        let tail = parts.next().unwrap_or("");
        let is_storefront =
            storefront.len() == 2 && storefront.chars().all(|c| c.is_ascii_lowercase());
        if !is_storefront || !kinds.contains(&kind) {
            continue;
        }
        let qs = tail.strip_prefix('?').unwrap_or(tail);
        let id = qs
            .split('&')
            .find_map(|kv| kv.strip_prefix("i="))
            .map(|s| s.split(['#', '&']).next().unwrap_or(s))
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()));
        if let Some(id) = id {
            return format!("{prefix}{storefront}/{kind}/x/{id}");
        }
    }
    url.to_string()
}

pub fn parse_apple_music_url(url: &str) -> Option<AppleMusicUrlInfo> {
    let url_owned = normalize_for_parse(url);
    let url = url_owned.as_str();

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
    fn parse_short_song_url() {
        let info = parse_apple_music_url("https://music.apple.com/song/1053934846").unwrap();
        assert_eq!(info.content_type, "song");
        assert_eq!(info.content_id, "1053934846");
    }

    #[test]
    fn parse_library_song_url() {
        let info = parse_apple_music_url("https://music.apple.com/library/song/417723943").unwrap();
        assert_eq!(info.content_type, "song");
        assert_eq!(info.content_id, "417723943");
    }

    #[test]
    fn parse_album_song_with_extra_query() {
        let info = parse_apple_music_url(
            "https://music.apple.com/us/album/kissing-a-fool/395918916?i=395918966&uo=4",
        )
        .unwrap();
        assert_eq!(info.content_type, "song");
        assert_eq!(info.content_id, "395918966");
    }

    #[test]
    fn parse_storefront_song_query_only() {
        let info = parse_apple_music_url("https://music.apple.com/us/song/?i=395918966").unwrap();
        assert_eq!(info.storefront, "us");
        assert_eq!(info.content_type, "song");
        assert_eq!(info.content_id, "395918966");
    }

    #[test]
    fn parse_storefront_song_canonical_slug() {
        let info = parse_apple_music_url("https://music.apple.com/us/song/x/395918966").unwrap();
        assert_eq!(info.storefront, "us");
        assert_eq!(info.content_type, "song");
        assert_eq!(info.content_id, "395918966");
    }
}

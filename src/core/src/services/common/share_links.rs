use serde::{Deserialize, Serialize};

use crate::errors::{MhError, MhResult};
use crate::http_client::build_client;
use crate::ipc_contract::{SearchPlatform, SearchType};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedLink {
    pub platform: SearchPlatform,
    pub kind: SearchType,
    pub id: String,
}

fn map_kind(seg: &str) -> Option<SearchType> {
    match seg.to_ascii_lowercase().as_str() {
        "track" | "song" | "songs" => Some(SearchType::Track),
        "album" | "albums" => Some(SearchType::Album),
        "artist" | "artists" => Some(SearchType::Artist),
        "playlist" | "playlists" => Some(SearchType::Playlist),
        "episode" | "episodes" => Some(SearchType::Episode),
        "show" | "shows" => Some(SearchType::Show),
        "podcast" | "podcasts" => Some(SearchType::Podcast),
        _ => None,
    }
}

fn host_platform(host: &str) -> Option<SearchPlatform> {
    let h = host.trim_start_matches("www.").to_ascii_lowercase();
    if h.contains("spotify") {
        Some(SearchPlatform::Spotify)
    } else if h.contains("deezer") {
        Some(SearchPlatform::Deezer)
    } else if h.contains("tidal") {
        Some(SearchPlatform::Tidal)
    } else if h.contains("qobuz") {
        Some(SearchPlatform::Qobuz)
    } else if h.contains("apple") {
        Some(SearchPlatform::AppleMusic)
    } else if h.contains("music.youtube") {
        Some(SearchPlatform::YoutubeMusic)
    } else if h.contains("youtu") {
        Some(SearchPlatform::Youtube)
    } else {
        None
    }
}

fn is_short_link(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h.starts_with("link.")
        || h.contains(".link")
        || h.contains(".page.link")
        || h.starts_with("dzr.page")
        || h.starts_with("spotify.link")
        || h == "youtu.be"
}

async fn follow_redirects(url: &str) -> MhResult<String> {
    let client = build_client()?;
    let resp = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(MhError::Network)?;
    Ok(resp.url().to_string())
}

fn parse_canonical(url: &str) -> Option<ResolvedLink> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let platform = host_platform(host)?;

    if matches!(
        platform,
        SearchPlatform::Youtube | SearchPlatform::YoutubeMusic
    ) {
        return parse_youtube(&parsed, platform);
    }

    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();

    for (i, seg) in segments.iter().enumerate() {
        if let Some(kind) = map_kind(seg) {
            let id = if platform == SearchPlatform::AppleMusic {
                segments.get(i + 2).or_else(|| segments.get(i + 1))?
            } else {
                segments.get(i + 1)?
            };
            let id = apple_track_query(&parsed, &platform, id);
            return Some(ResolvedLink { platform, kind, id });
        }
    }
    None
}

fn apple_track_query(parsed: &url::Url, platform: &SearchPlatform, id: &str) -> String {
    if *platform == SearchPlatform::AppleMusic {
        if let Some((_, v)) = parsed.query_pairs().find(|(k, _)| k == "i") {
            return v.to_string();
        }
    }
    id.to_string()
}

fn parse_youtube(parsed: &url::Url, platform: SearchPlatform) -> Option<ResolvedLink> {
    if let Some((_, v)) = parsed.query_pairs().find(|(k, _)| k == "list") {
        return Some(ResolvedLink {
            platform,
            kind: SearchType::Playlist,
            id: v.to_string(),
        });
    }
    if let Some((_, v)) = parsed.query_pairs().find(|(k, _)| k == "v") {
        return Some(ResolvedLink {
            platform,
            kind: SearchType::Track,
            id: v.to_string(),
        });
    }
    let seg: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    if let Some(id) = seg.first() {
        return Some(ResolvedLink {
            platform,
            kind: SearchType::Track,
            id: id.to_string(),
        });
    }
    None
}

/// The canonical download URL for a resolved link, in the spelling the pipeline's own
/// `detect_platform_and_type` recognises. `None` for anything the pipeline cannot
/// download — Spotify, Apple Music and YouTube have their own engines.
pub fn canonical_pipeline_url(link: &ResolvedLink) -> Option<String> {
    let kind = match link.kind {
        SearchType::Track | SearchType::Song => "track",
        SearchType::Album => "album",
        SearchType::Playlist => "playlist",
        SearchType::Artist => "artist",
        _ => return None,
    };
    let id = &link.id;
    match link.platform {
        SearchPlatform::Deezer => Some(format!("https://www.deezer.com/{kind}/{id}")),
        SearchPlatform::Qobuz => Some(format!("https://play.qobuz.com/{kind}/{id}")),
        SearchPlatform::Tidal => Some(format!("https://tidal.com/browse/{kind}/{id}")),
        _ => None,
    }
}

pub async fn resolve_share_url(input: &str) -> MhResult<ResolvedLink> {
    let trimmed = input.trim();
    let parsed = url::Url::parse(trimmed)
        .map_err(|_| MhError::Other(format!("Not a valid URL: {trimmed}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| MhError::Other("URL has no host".to_string()))?;

    let canonical = if is_short_link(host) {
        follow_redirects(trimmed).await?
    } else {
        trimmed.to_string()
    };

    parse_canonical(&canonical).ok_or_else(|| {
        MhError::Other(format!(
            "Could not resolve share link to a known item: {canonical}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deezer_canonical_with_locale() {
        let r = parse_canonical("https://www.deezer.com/tr/track/3135556").unwrap();
        assert_eq!(r.platform, SearchPlatform::Deezer);
        assert_eq!(r.kind, SearchType::Track);
        assert_eq!(r.id, "3135556");
    }

    #[test]
    fn deezer_canonical_no_locale() {
        let r = parse_canonical("https://www.deezer.com/album/302127").unwrap();
        assert_eq!(r.kind, SearchType::Album);
        assert_eq!(r.id, "302127");
    }

    #[test]
    fn spotify_open_url() {
        let r = parse_canonical("https://open.spotify.com/track/4iV5W9uYEdYUVa79Axb7Rh").unwrap();
        assert_eq!(r.platform, SearchPlatform::Spotify);
        assert_eq!(r.kind, SearchType::Track);
        assert_eq!(r.id, "4iV5W9uYEdYUVa79Axb7Rh");
    }

    #[test]
    fn apple_album_with_track_query() {
        let r =
            parse_canonical("https://music.apple.com/us/album/album-name/1440857781?i=1440857785")
                .unwrap();
        assert_eq!(r.platform, SearchPlatform::AppleMusic);
        assert_eq!(r.kind, SearchType::Album);
        assert_eq!(r.id, "1440857785");
    }

    #[test]
    fn tidal_browse() {
        let r = parse_canonical("https://tidal.com/browse/track/12345678").unwrap();
        assert_eq!(r.platform, SearchPlatform::Tidal);
        assert_eq!(r.id, "12345678");
    }

    #[test]
    fn youtube_watch() {
        let r = parse_canonical("https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
        assert_eq!(r.platform, SearchPlatform::Youtube);
        assert_eq!(r.kind, SearchType::Track);
        assert_eq!(r.id, "dQw4w9WgXcQ");
    }

    #[test]
    fn a_resolved_link_rebuilds_a_url_the_pipeline_can_detect() {
        let deezer = ResolvedLink {
            platform: SearchPlatform::Deezer,
            kind: SearchType::Track,
            id: "377993071".into(),
        };
        assert_eq!(
            canonical_pipeline_url(&deezer).as_deref(),
            Some("https://www.deezer.com/track/377993071")
        );
        let url = canonical_pipeline_url(&deezer).unwrap();
        assert!(
            crate::services::common::pipeline::orchestrator::detect_platform_and_type(&url)
                .is_some()
        );

        let tidal = ResolvedLink {
            platform: SearchPlatform::Tidal,
            kind: SearchType::Album,
            id: "12345678".into(),
        };
        assert_eq!(
            canonical_pipeline_url(&tidal).as_deref(),
            Some("https://tidal.com/browse/album/12345678")
        );

        let spotify = ResolvedLink {
            platform: SearchPlatform::Spotify,
            kind: SearchType::Track,
            id: "4iV5W9uYEdYUVa79Axb7Rh".into(),
        };
        assert!(canonical_pipeline_url(&spotify).is_none());
    }

    #[test]
    fn deezer_share_hosts_are_recognised_as_short_links() {
        assert!(is_short_link("link.deezer.com"));
        assert!(is_short_link("dzr.page.link"));
        assert!(is_short_link("spotify.link"));
        assert!(is_short_link("youtu.be"));
        assert!(!is_short_link("www.deezer.com"));
        assert!(!is_short_link("play.qobuz.com"));
    }

    #[test]
    fn youtube_playlist() {
        let r = parse_canonical("https://music.youtube.com/watch?v=abc&list=PL123").unwrap();
        assert_eq!(r.kind, SearchType::Playlist);
        assert_eq!(r.id, "PL123");
    }
}

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::errors::{MhError, MhResult};
use crate::http_client;
use crate::services::radio::directory::CustomStations;
use crate::services::radio::icecast;
use crate::services::radio::model::{codec_from_mime, Station, StreamKind};
use crate::services::radio::playlist_parse::{self, PlaylistKind};

/// The content types a playlist arrives as. An extension is the usual signal;
/// these cover the hosts that serve a `.pls` from an extensionless path.
const PLAYLIST_TYPES: &[&str] = &[
    "audio/x-scpls",
    "audio/scpls",
    "audio/x-mpegurl",
    "audio/mpegurl",
    "application/x-mpegurl",
    "application/vnd.apple.mpegurl",
    "application/pls+xml",
];

fn station_from(source_id: &str, name: String, url: String, kind: StreamKind) -> Station {
    Station {
        // The kind is known here from the content type, which is better evidence
        // than the extension `Station::new` guesses from.
        stream_kind: kind,
        ..Station::new(CustomStations::ID, source_id, name, url)
    }
}

/// One probe client for the whole process.
///
/// `http_client::build_probe_client` loads the TLS root store and starts with an
/// empty connection pool every time it is called, and the editor's "test stream"
/// button fires this per interaction.
fn probe_client() -> MhResult<reqwest::Client> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| http_client::build_probe_client().map_err(|e| e.to_string()))
        .clone()
        .map_err(MhError::Other)
}

/// Path segments that name the *file* rather than the station. A URL ending
/// `/abr/playlist.m3u8` is not a station called "playlist".
const GENERIC_SEGMENTS: &[&str] = &[
    "abr",
    "aac",
    "audio",
    "chunklist",
    "chunks",
    "hls",
    "index",
    "listen",
    "live",
    "manifest",
    "master",
    "media",
    "mp3",
    "play",
    "playlist",
    "radio",
    "stream",
    "streams",
    "tunein",
];

fn is_generic(segment: &str) -> bool {
    let base = segment.rsplit_once('.').map_or(segment, |(b, _)| b);
    base.is_empty()
        || base.chars().all(|c| c.is_ascii_digit())
        || GENERIC_SEGMENTS.contains(&base.to_ascii_lowercase().as_str())
}

/// A name to show before the user has typed one.
///
/// The last path segment is usually the file, not the station — the reported
/// `…/powerfmadver/abr/playlist.m3u8` used to import as "playlist" — so generic
/// segments are walked past, and a URL with nothing but generics falls back to
/// the host.
pub(super) fn name_from_url(url: &str) -> String {
    let trimmed = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .trim_end_matches('/');
    let after_scheme = trimmed.split_once("://").map_or(trimmed, |(_, rest)| rest);
    let (host, path) = after_scheme
        .split_once('/')
        .map_or((after_scheme, ""), |(h, p)| (h, p));

    let meaningful = path.split('/').rfind(|s| !s.is_empty() && !is_generic(s));
    match meaningful {
        Some(segment) => segment
            .rsplit_once('.')
            .map_or(segment, |(base, _)| base)
            .to_string(),
        None => host.to_string(),
    }
}

/// Imports whatever a pasted URL turns out to be: a direct stream, an HLS
/// manifest, or a playlist file listing mirrors.
///
/// Built on [`probe`] rather than repeating the request, so what the editor's
/// "test stream" button reports and what actually gets imported cannot drift
/// apart. A playlist is the one case that needs the body too, and only that case
/// pays for a second fetch.
pub async fn from_url(
    url: &str,
    name: Option<&str>,
    headers: &BTreeMap<String, String>,
) -> MhResult<Station> {
    let url = url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(MhError::Parse(format!(
            "{url} is not an http(s) stream URL"
        )));
    }
    let probed = probe(url, headers).await;
    if !probed.ok {
        return Err(MhError::Other(
            probed
                .message
                .unwrap_or_else(|| format!("{url} could not be reached")),
        ));
    }

    if matches!(
        probed.stream_kind,
        StreamKind::Hls | StreamKind::PlaylistFile
    ) {
        let body = fetch_text(url, headers).await?;
        let mut station = from_playlist_body(url, &body, name)?;
        station.bitrate = station.bitrate.or(probed.bitrate);
        station.tags = station.tags.or(probed.genre);
        station.headers = headers.clone();
        return Ok(station);
    }

    let content_type = probed.content_type.unwrap_or_default();
    if !content_type.starts_with("audio/")
        && !content_type.starts_with("application/octet-stream")
        && !content_type.starts_with("video/")
    {
        return Err(MhError::Unsupported(format!(
            "{url} serves {content_type}, which is not audio"
        )));
    }

    let mut station = station_from(
        url,
        name.map(str::to_string)
            .or(probed.icy_name)
            .unwrap_or_else(|| name_from_url(url)),
        url.to_string(),
        StreamKind::Direct,
    );
    station.codec = probed.codec;
    station.bitrate = probed.bitrate;
    station.tags = probed.genre;
    station.homepage = origin_of(url);
    station.headers = headers.clone();
    Ok(station)
}

/// Fetches a playlist body with the station's own headers, which some hosts
/// check before they will serve it.
async fn fetch_text(url: &str, headers: &BTreeMap<String, String>) -> MhResult<String> {
    let mut request = probe_client()?.get(url);
    for (header, value) in headers {
        request = request.header(header.as_str(), value.as_str());
    }
    request
        .send()
        .await
        .map_err(MhError::Network)?
        .text()
        .await
        .map_err(MhError::Network)
}

/// What a stream looked like when we asked it just now.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamProbe {
    pub ok: bool,
    pub status: u16,
    pub content_type: Option<String>,
    pub codec: Option<String>,
    pub bitrate: Option<u32>,
    pub icy_name: Option<String>,
    /// `icy-genre`, which becomes the imported station's tags.
    pub genre: Option<String>,
    pub stream_kind: StreamKind,
    /// The upstream failure in full when `ok` is false.
    pub message: Option<String>,
}

/// Asks a stream what it is, with the station's own headers.
///
/// The editor uses it so a mistyped URL or a missing `Referer` is visible before
/// the station is saved, rather than as silence at play time.
pub async fn probe(url: &str, headers: &BTreeMap<String, String>) -> StreamProbe {
    let url = url.trim();
    let mut out = StreamProbe {
        stream_kind: StreamKind::guess(url),
        ..Default::default()
    };
    if !url.starts_with("http://") && !url.starts_with("https://") {
        out.message = Some(format!("{url} is not an http(s) URL"));
        return out;
    }
    let client = match probe_client() {
        Ok(c) => c,
        Err(e) => {
            out.message = Some(e.to_string());
            return out;
        }
    };
    let mut request = client.get(url).header("Icy-MetaData", "1");
    for (header, value) in headers {
        request = request.header(header.as_str(), value.as_str());
    }
    let resp = match request.send().await {
        Ok(r) => r,
        Err(e) => {
            out.message = Some(e.to_string());
            return out;
        }
    };
    out.status = resp.status().as_u16();
    let head = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    out.content_type = head("content-type").map(|c| {
        c.split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
    });
    out.icy_name = head("icy-name").filter(|n| !n.trim().is_empty());
    out.genre = head("icy-genre").filter(|g| !g.trim().is_empty());
    out.bitrate = head("icy-br")
        .and_then(|v| v.split(',').next().map(str::to_string))
        .and_then(|v| v.trim().parse().ok());
    out.codec = out.content_type.as_deref().and_then(codec_from_mime);
    if !resp.status().is_success() {
        out.message = Some(format!("the station answered HTTP {}", out.status));
        return out;
    }
    if let Some(ct) = out.content_type.as_deref() {
        if PLAYLIST_TYPES.contains(&ct) && out.stream_kind == StreamKind::Direct {
            out.stream_kind = StreamKind::PlaylistFile;
        }
    }
    out.ok = true;
    out
}

/// Imports a playlist the user picked off disk.
pub async fn from_playlist_file(path: &str) -> MhResult<Station> {
    let body = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| MhError::Other(format!("{path}: {e}")))?;
    from_playlist_body(path, &body, None)
}

/// Shared by both playlist entry points. `source` is the thing the user gave us
/// — a URL or a path — and becomes the station's id, so re-importing the same
/// playlist updates the station instead of duplicating it.
fn from_playlist_body(source: &str, body: &str, name: Option<&str>) -> MhResult<Station> {
    match playlist_parse::parse(body)? {
        // An `.m3u8` that is really a manifest: the original URL goes to the
        // remuxer untouched. Parsing it would yield segment URLs.
        PlaylistKind::Hls => Ok(station_from(
            source,
            name.map(str::to_string)
                .unwrap_or_else(|| name_from_url(source)),
            source.to_string(),
            StreamKind::Hls,
        )),
        PlaylistKind::Entries(list) => {
            let mut urls = list.urls.into_iter();
            let first = urls
                .next()
                .ok_or_else(|| MhError::Parse("playlist contained no stream URLs".into()))?;
            let mut station = station_from(
                source,
                name.map(str::to_string)
                    .or(list.title)
                    .unwrap_or_else(|| name_from_url(source)),
                first.clone(),
                StreamKind::guess(&first),
            );
            station.alt_urls = urls.collect();
            station.homepage = origin_of(source);
            Ok(station)
        }
    }
}

/// Every mount an Icecast host is serving, ready to be saved. Nothing is stored
/// yet — the user picks which ones they want.
pub async fn from_icecast(host: &str) -> MhResult<Vec<Station>> {
    icecast::mounts(host).await
}

pub(super) fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?;
    (!host.is_empty()).then(|| format!("{scheme}://{host}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_playlist_body_keeps_its_mirrors_and_its_source_as_the_id() {
        let body = "[playlist]\nFile1=https://a.example/one\nFile2=https://b.example/two\n\
Title1=Test Station\n";
        let station = from_playlist_body("https://x.example/live.pls", body, None).unwrap();
        assert_eq!(station.key, "custom:https://x.example/live.pls");
        assert_eq!(station.source_id, "https://x.example/live.pls");
        assert_eq!(station.stream_url, "https://a.example/one");
        assert_eq!(station.alt_urls, vec!["https://b.example/two"]);
        assert_eq!(station.name, "Test Station");
    }

    #[test]
    fn an_hls_manifest_keeps_the_original_url_and_is_not_split_into_segments() {
        let body = "#EXTM3U\n#EXT-X-VERSION:3\n#EXTINF:10.0,\nseg1.aac\n";
        let station =
            from_playlist_body("https://x.example/live.m3u8", body, Some("Named")).unwrap();
        assert_eq!(station.stream_kind, StreamKind::Hls);
        assert_eq!(station.stream_url, "https://x.example/live.m3u8");
        assert!(station.alt_urls.is_empty());
        assert_eq!(station.name, "Named");
    }
}

use serde::Deserialize;

use crate::errors::{MhError, MhResult};
use crate::http_client;
use crate::services::common::http::read_json;
use crate::services::radio::directory::{xiph::normalize_bitrate, CustomStations};
use crate::services::radio::import::name_from_url;
use crate::services::radio::model::{codec_from_mime, Station};

/// Icecast's standard status document. One reader covers every Icecast and
/// AzuraCast host a user can paste, and it carries the current title — so a
/// station imported this way gets now-playing without ICY byte-parsing.
///
/// It is opportunistic, never required: SomaFM's edge answers HTML here and
/// Radio France answers 403.
pub const STATUS_PATH: &str = "/status-json.xsl";

const USER_AGENT: &str = concat!("MediaHarbor/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Deserialize)]
struct StatusDocument {
    icestats: IceStats,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct IceStats {
    #[serde(default)]
    source: Option<SourceField>,
}

/// A host with exactly one mount serialises `source` as an object rather than a
/// one-element array.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum SourceField {
    One(Box<Mount>),
    Many(Vec<Mount>),
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Mount {
    #[serde(default)]
    listenurl: Option<String>,
    #[serde(default)]
    server_name: Option<String>,
    #[serde(default)]
    server_description: Option<String>,
    #[serde(default)]
    server_type: Option<String>,
    #[serde(default)]
    genre: Option<String>,
    #[serde(default)]
    bitrate: Option<u32>,
    #[serde(default)]
    listeners: Option<i64>,
    #[serde(default)]
    title: Option<String>,
}

impl SourceField {
    fn into_vec(self) -> Vec<Mount> {
        match self {
            SourceField::One(m) => vec![*m],
            SourceField::Many(v) => v,
        }
    }
}

/// Turns a host the user pasted into the URL of its status document. Accepts a
/// bare hostname, an origin, or the status URL itself.
pub fn status_url(host: &str) -> MhResult<String> {
    let host = host.trim().trim_end_matches('/');
    if host.is_empty() {
        return Err(MhError::Parse("no host given".into()));
    }
    if host.ends_with(STATUS_PATH) {
        return Ok(host.to_string());
    }
    let base = if host.starts_with("http://") || host.starts_with("https://") {
        host.to_string()
    } else {
        format!("https://{host}")
    };
    Ok(format!("{base}{STATUS_PATH}"))
}

async fn fetch(url: &str) -> MhResult<Vec<Mount>> {
    let http = http_client::ua_client(USER_AGENT)?;
    let resp = http.get(url).send().await.map_err(MhError::Network)?;
    let parsed: StatusDocument = read_json(url, resp).await?;
    Ok(parsed
        .icestats
        .source
        .map(SourceField::into_vec)
        .unwrap_or_default())
}

/// Every playable mount on `host`, as importable stations.
pub async fn mounts(host: &str) -> MhResult<Vec<Station>> {
    let url = status_url(host)?;
    let mounts = fetch(&url).await?;
    let stations: Vec<Station> = mounts
        .into_iter()
        .filter_map(|m| {
            let listen = m.listenurl.filter(|u| !u.is_empty())?;
            let name = m
                .server_name
                .filter(|n| !n.is_empty() && !n.starts_with("Radio "))
                .or_else(|| {
                    m.server_description
                        .filter(|d| d != "Unspecified description")
                })
                .unwrap_or_else(|| name_from_url(&listen));
            Some(Station {
                homepage: Some(url.trim_end_matches(STATUS_PATH).to_string()),
                tags: m.genre.filter(|g| !g.is_empty() && g != "various"),
                codec: m.server_type.as_deref().and_then(codec_from_mime),
                bitrate: m.bitrate.and_then(normalize_bitrate),
                clickcount: m.listeners,
                ..Station::new(CustomStations::ID, &listen, name, listen.clone())
            })
        })
        .collect();
    if stations.is_empty() {
        return Err(MhError::NotFound(format!("{url} lists no playable mounts")));
    }
    Ok(stations)
}

/// The title the host reports for `listen_url` right now.
///
/// `status_host` is separate on purpose: an Icecast admin can point `listenurl`
/// at a relay or CDN on another host entirely — Nightride publishes its status
/// on `stream.nightride.fm` and streams from `lissen.to:8000` — so deriving the
/// status address from the stream address reaches a host that is not serving one.
pub async fn now_playing(status_host: &str, listen_url: &str) -> MhResult<Option<String>> {
    let mounts = fetch(&status_url(status_host)?).await?;
    Ok(mounts
        .into_iter()
        .find(|m| m.listenurl.as_deref() == Some(listen_url))
        .and_then(|m| m.title)
        .filter(|t| !t.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_bare_host_an_origin_or_the_status_url() {
        assert_eq!(
            status_url("stream.nightride.fm").unwrap(),
            "https://stream.nightride.fm/status-json.xsl"
        );
        assert_eq!(
            status_url("http://example.org:8000/").unwrap(),
            "http://example.org:8000/status-json.xsl"
        );
        assert_eq!(
            status_url("https://a.example/status-json.xsl").unwrap(),
            "https://a.example/status-json.xsl"
        );
    }

    #[test]
    fn reads_a_single_mount_served_as_an_object() {
        let body = r#"{"icestats":{"source":{"listenurl":"http://a.example:8000/live",
            "server_name":"Test","bitrate":128,"server_type":"audio/mpeg","title":"A - B"}}}"#;
        let doc: StatusDocument = serde_json::from_str(body).unwrap();
        let mounts = doc.icestats.source.unwrap().into_vec();
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].title.as_deref(), Some("A - B"));
    }

    /// The status document and the stream can live on different hosts, so a
    /// status URL is built from the host the station was imported from, port and
    /// all — never from the stream URL.
    #[test]
    fn a_status_url_keeps_the_port_it_was_given() {
        assert_eq!(
            status_url("http://lissen.to:8000").unwrap(),
            "http://lissen.to:8000/status-json.xsl"
        );
    }
}

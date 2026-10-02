use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How the URL in [`Station::stream_url`] has to be handled before a WebView
/// `<audio>` element will take it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamKind {
    #[default]
    Direct,
    Hls,
    PlaylistFile,
}

impl StreamKind {
    /// Every variant, so the test below can pin `as_str` against the serde name
    /// and neither mapping can drift from the other.
    pub const ALL: [StreamKind; 3] = [
        StreamKind::Direct,
        StreamKind::Hls,
        StreamKind::PlaylistFile,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            StreamKind::Direct => "direct",
            StreamKind::Hls => "hls",
            StreamKind::PlaylistFile => "playlistFile",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "hls" => StreamKind::Hls,
            "playlistFile" => StreamKind::PlaylistFile,
            _ => StreamKind::Direct,
        }
    }

    /// Classifies a bare URL. The directories' own HLS flags are unreliable —
    /// radio-browser stations serving `index.m3u8` report `hls: 0` — so the URL
    /// is what gets believed.
    pub fn guess(url: &str) -> Self {
        let path = url.split(['?', '#']).next().unwrap_or_default();
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".m3u8") {
            StreamKind::Hls
        } else if lower.ends_with(".pls") || lower.ends_with(".m3u") {
            StreamKind::PlaylistFile
        } else {
            StreamKind::Direct
        }
    }
}

/// The one station shape that crosses IPC. Every directory adapter maps its own
/// wire format onto this before it leaves the module, so the frontend sees a
/// single camelCase type instead of one per source.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Station {
    /// `"{source}:{source_id}"`. The primary key everywhere — the store, the
    /// list tables, and the `url` field of a play request.
    pub key: String,
    pub source: String,
    pub source_id: String,
    pub name: String,
    pub stream_url: String,
    pub stream_kind: StreamKind,
    /// Mirrors of the same stream, in preference order. SomaFM publishes three.
    #[serde(default)]
    pub alt_urls: Vec<String>,
    pub homepage: Option<String>,
    pub favicon: Option<String>,
    pub tags: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub bitrate: Option<u32>,
    pub votes: Option<i64>,
    pub clickcount: Option<i64>,
    /// A cover the user supplied, held in the shared cover cache. No directory
    /// provides one, so this never has an upstream value to diverge from — it is
    /// a plain column rather than an override.
    pub cover_id: Option<String>,
    /// Request headers this station needs. Some broadcasters check `Referer` or
    /// `Origin` and answer 403 without them.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

impl Station {
    pub fn new(source: &str, source_id: &str, name: String, stream_url: String) -> Self {
        Self {
            key: station_key(source, source_id),
            source: source.to_string(),
            source_id: source_id.to_string(),
            stream_kind: StreamKind::guess(&stream_url),
            name,
            stream_url,
            ..Default::default()
        }
    }

    /// Every URL worth trying, best first.
    pub fn candidate_urls(&self) -> Vec<&str> {
        std::iter::once(self.stream_url.as_str())
            .chain(self.alt_urls.iter().map(String::as_str))
            .filter(|u| !u.is_empty())
            .collect()
    }
}

/// The codec a stream's MIME type names, in the form the UI shows.
///
/// `None` where the type says nothing useful: a mount answering
/// `application/octet-stream` has not told us its codec, and printing
/// "OCTET-STREAM" in the quality slot is worse than printing nothing.
pub fn codec_from_mime(mime: &str) -> Option<String> {
    let subtype = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    Some(match subtype.as_str() {
        "mpeg" | "mp3" => "MP3",
        "aac" | "aacp" | "mp4" => "AAC",
        "ogg" | "opus" | "vorbis" => "OGG",
        "flac" | "x-flac" => "FLAC",
        _ => return None,
    }
    .to_string())
}

pub fn station_key(source: &str, source_id: &str) -> String {
    format!("{source}:{source_id}")
}

/// Splits on the **first** colon only, so a `source_id` that itself contains one
/// — an Icecast mount pasted as `host:8000/mount` — survives the round trip.
pub fn split_key(key: &str) -> Option<(&str, &str)> {
    key.split_once(':')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FacetKind {
    Tags,
    Countries,
    Languages,
    Codecs,
}

impl FacetKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "tags" => Some(FacetKind::Tags),
            "countries" => Some(FacetKind::Countries),
            "languages" => Some(FacetKind::Languages),
            "codecs" => Some(FacetKind::Codecs),
            _ => None,
        }
    }
}

/// One browsable value. `name` is what a query filters on — an ISO code for a
/// country — and `label` is what a human reads, which is why the country facet
/// no longer shows `DE` where it means Germany.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facet {
    pub name: String,
    pub label: String,
    pub station_count: i64,
}

impl Facet {
    pub fn new(name: impl Into<String>, count: i64) -> Self {
        let name = name.into();
        Self {
            label: name.clone(),
            name,
            station_count: count,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationQuery {
    pub name: Option<String>,
    pub tag: Option<String>,
    pub country_code: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub bitrate_min: Option<u32>,
    /// `clickcount` | `votes` | `clicktrend` | `random` | `name`.
    pub order: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    /// Directory ids to ask. `None` means whatever the settings enable.
    pub sources: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryCapabilities {
    /// Which browse lists this directory can fill. The radio page hides a facet
    /// no enabled directory offers rather than showing an empty one.
    pub facets: Vec<FacetKind>,
    /// Whether asking for a second page returns anything new. The local
    /// directories answer from a fixed collection, so they do not.
    pub paginates: bool,
}

/// Whether a directory can answer yet.
///
/// The Icecast directory is one 9.5 MB document; it mirrors itself in the
/// background rather than making a query wait fourteen seconds, so there is a
/// window where it is enabled and has nothing to say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DirectoryStatus {
    #[default]
    Ready,
    Loading,
}

/// One directory as the frontend sees it: what it can do, and whether the user
/// has it switched on.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectorySource {
    pub id: String,
    pub label: String,
    pub capabilities: DirectoryCapabilities,
    pub enabled: bool,
    /// False for the user's own stations, which are data rather than a feed and
    /// so have nothing to switch off.
    pub toggleable: bool,
    #[serde(default)]
    pub status: DirectoryStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// There is no codegen between these types and their TypeScript mirror in
    /// `src/tauri-bridge.ts`, so the wire shape is pinned here. The old station
    /// DTO carried per-field renames that made `rename_all` a no-op and shipped
    /// `stationuuid` / `url_resolved` / `countrycode` alongside a second,
    /// snake_case shape from the store — the adapter between them is what this
    /// type replaced.
    #[test]
    fn a_station_crosses_ipc_as_one_camel_case_shape() {
        let mut station = Station::new(
            "radiobrowser",
            "abc-123",
            "Test".into(),
            "https://s.example/live".into(),
        );
        station.country_code = Some("DE".into());
        station.alt_urls = vec!["https://s2.example/live".into()];

        let json = serde_json::to_value(&station).unwrap();
        // Compared as a sorted set: `serde_json::Value` is a `BTreeMap`, and the
        // names are the contract — their order on the wire is not.
        let keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "altUrls",
                "bitrate",
                "clickcount",
                "codec",
                "country",
                "countryCode",
                "coverId",
                "favicon",
                "headers",
                "homepage",
                "key",
                "language",
                "name",
                "source",
                "sourceId",
                "streamKind",
                "streamUrl",
                "tags",
                "votes",
            ]
        );
        assert_eq!(json["key"], "radiobrowser:abc-123");
        assert_eq!(json["streamKind"], "direct");
    }

    #[test]
    fn the_facet_and_source_shapes_are_camel_case_too() {
        let facet = serde_json::to_value(Facet::new("jazz", 1233)).unwrap();
        assert_eq!(
            facet.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["label", "name", "stationCount"]
        );

        let source = serde_json::to_value(DirectorySource {
            id: "somafm".into(),
            label: "SomaFM".into(),
            capabilities: DirectoryCapabilities {
                facets: vec![FacetKind::Tags],
                paginates: false,
            },
            enabled: true,
            toggleable: true,
            status: DirectoryStatus::Ready,
        })
        .unwrap();
        assert_eq!(source["status"], "ready");
        assert_eq!(source["capabilities"]["paginates"], false);
        assert_eq!(source["capabilities"]["facets"][0], "tags");
    }

    /// The `.m3u8` extension is shared by a playlist and an HLS manifest, and the
    /// directories' own `hls` flag lies — stations serving `index.m3u8` report
    /// `hls: 0`.
    #[test]
    fn stream_kind_is_guessed_from_the_url_and_survives_a_query_string() {
        assert_eq!(
            StreamKind::guess("https://x.example/live.m3u8?token=1"),
            StreamKind::Hls
        );
        assert_eq!(
            StreamKind::guess("https://x.example/a.pls"),
            StreamKind::PlaylistFile
        );
        assert_eq!(
            StreamKind::guess("https://x.example/stream"),
            StreamKind::Direct
        );
        assert_eq!(StreamKind::parse(StreamKind::Hls.as_str()), StreamKind::Hls);
    }

    /// `as_str`/`parse` back the SQLite text column and the serde derive backs
    /// the wire. They are two mappings of the same three strings, so this pins
    /// them together rather than leaving the agreement to inspection.
    #[test]
    fn the_stored_stream_kind_and_the_wire_one_are_the_same_string() {
        for kind in StreamKind::ALL {
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
            assert_eq!(StreamKind::parse(kind.as_str()), kind);
        }
    }

    #[test]
    fn a_mime_type_names_a_codec_only_when_it_actually_does() {
        assert_eq!(codec_from_mime("audio/mpeg"), Some("MP3".into()));
        assert_eq!(codec_from_mime("audio/x-flac"), Some("FLAC".into()));
        // Charset parameters and casing are both live in the wild.
        assert_eq!(
            codec_from_mime("audio/AACP; charset=utf-8"),
            Some("AAC".into())
        );
        assert_eq!(codec_from_mime("application/octet-stream"), None);
        assert_eq!(codec_from_mime(""), None);
    }

    #[test]
    fn a_key_splits_on_the_first_colon_only() {
        let url = "http://lissen.to:8000/chillsynth.mp3";
        let key = station_key("custom", url);
        assert_eq!(split_key(&key), Some(("custom", url)));
    }
}

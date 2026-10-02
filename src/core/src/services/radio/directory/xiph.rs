use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::errors::{MhError, MhResult};
use crate::http_client;
use crate::media::library::Library;
use crate::services::common::ids::now_secs;
use crate::services::radio::directory::{
    facets_from_counts, page, DirectoryStatus, RadioDirectory, TextFilter,
};
use crate::services::radio::import::name_from_url;
use crate::services::radio::model::{
    codec_from_mime, DirectoryCapabilities, Facet, FacetKind, Station, StationQuery,
};

const USER_AGENT: &str = concat!("MediaHarbor/", env!("CARGO_PKG_VERSION"));
const YP_URL: &str = "https://dir.xiph.org/yp.xml";

/// The document is 9.5 MB and takes about fourteen seconds to fetch, so it is
/// mirrored to disk and only refetched when a listener would notice it is stale.
const TTL: Duration = Duration::from_secs(12 * 60 * 60);

/// One mount, kept small on purpose: 13.7k of these are held in memory and
/// written to disk as JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Entry {
    #[serde(rename = "n")]
    name: String,
    #[serde(rename = "u")]
    listen_url: String,
    #[serde(rename = "t", skip_serializing_if = "Option::is_none", default)]
    server_type: Option<String>,
    #[serde(rename = "b", skip_serializing_if = "Option::is_none", default)]
    bitrate: Option<u32>,
    #[serde(rename = "g", skip_serializing_if = "Option::is_none", default)]
    genre: Option<String>,
    #[serde(rename = "s", skip_serializing_if = "Option::is_none", default)]
    song: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Mirror {
    fetched_at: u64,
    entries: Vec<Entry>,
}

impl Mirror {
    fn is_fresh(&self) -> bool {
        now_secs().saturating_sub(self.fetched_at) < TTL.as_secs()
    }
}

fn cache() -> &'static RwLock<Option<Arc<Mirror>>> {
    static CACHE: OnceLock<RwLock<Option<Arc<Mirror>>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

/// Guards against a dozen concurrent queries each starting their own 9.5 MB
/// download the first time the page is opened.
fn refreshing() -> &'static AtomicBool {
    static REFRESHING: AtomicBool = AtomicBool::new(false);
    &REFRESHING
}

impl Entry {
    /// Borrowed rather than consuming, so a search can walk the shared mirror
    /// and only build the stations that survive its filter.
    fn to_station(&self) -> Station {
        Station {
            tags: self
                .genre
                .clone()
                .filter(|g| !g.is_empty() && g != "various"),
            codec: self.server_type.as_deref().and_then(codec_from_mime),
            bitrate: self.bitrate.and_then(normalize_bitrate),
            ..Station::new(
                XiphDirectory::ID,
                &self.listen_url,
                self.name.clone(),
                self.listen_url.clone(),
            )
        }
    }
}

/// Icecast servers disagree about the unit.
///
/// Most report kbps. A handful report bits per second — `320000`, `192192`,
/// `160009` — and at least one host uses `99999` to mean "no idea". Four-figure
/// values are *not* automatically bps: CD-rate FLAC is genuinely 1411 kbps and
/// 24-bit FLAC reaches 6144, so the split has to be higher than that.
pub(crate) fn normalize_bitrate(raw: u32) -> Option<u32> {
    match raw {
        0 => None,
        1..=8_000 => Some(raw),
        // Recognisable sentinels rather than a 100 kbps stream.
        9_999 | 99_999 | 999_999 => None,
        _ => {
            let kbps = (raw + 500) / 1_000;
            (8..=640).contains(&kbps).then_some(kbps)
        }
    }
}

/// The Icecast public directory: every server that opted into being listed.
///
/// Two things make it usable that were not obvious. Its entries carry no id, but
/// `listen_url` is unique across all of them and is exactly the shape a
/// `source:id` key wants. And `current_song` is in the document, so 13.7k
/// stations get now-playing for free — off the mirror, with no extra request.
pub struct XiphDirectory {
    cache_path: PathBuf,
}

impl XiphDirectory {
    pub const ID: &'static str = "xiph";

    /// Construction has no side effect on purpose. `describe` builds every
    /// directory just to read its label, and warming here meant that merely
    /// rendering the settings source list could start a 9.5 MB download.
    pub fn new(library: &Arc<Library>) -> Self {
        Self {
            cache_path: library.data_dir().join("radio").join("xiph-yp.json"),
        }
    }

    /// Loads the mirror if it is not loaded, and refreshes it if it is stale —
    /// always in the background. A query never waits fourteen seconds for it:
    /// an empty answer now beats a page that hangs.
    fn warm(&self) {
        if refreshing().swap(true, Ordering::SeqCst) {
            return;
        }
        let path = self.cache_path.clone();
        tokio::spawn(async move {
            let result = refresh(&path).await;
            refreshing().store(false, Ordering::SeqCst);
            if let Err(e) = result {
                tracing_warn(&format!("Icecast directory refresh failed: {e}"));
            }
        });
    }

    /// The shared mirror, by reference count. The cache holds an `Arc` precisely
    /// so a query does not have to copy 13.7k entries to read them.
    async fn mirror(&self) -> Option<Arc<Mirror>> {
        cache().read().await.clone()
    }
}

/// The title the directory last saw on this mount.
///
/// Free: `current_song` came down with the document, so a station here has
/// now-playing without a single extra request. It only moves when the mirror
/// refreshes, which is the honest limit of reading it this way.
pub async fn cached_now_playing(listen_url: &str) -> Option<String> {
    cache()
        .read()
        .await
        .as_ref()?
        .entries
        .iter()
        .find(|e| e.listen_url == listen_url)
        .and_then(|e| e.song.clone())
        .filter(|s| !s.trim().is_empty())
}

fn tracing_warn(message: &str) {
    eprintln!("[radio] {message}");
}

/// Reads the mirror from memory, then disk, then the network, in that order.
async fn refresh(path: &PathBuf) -> MhResult<()> {
    if cache().read().await.as_ref().is_some_and(|m| m.is_fresh()) {
        return Ok(());
    }
    if let Some(disk) = read_cache(path).await {
        let fresh = disk.is_fresh();
        *cache().write().await = Some(Arc::new(disk));
        if fresh {
            return Ok(());
        }
    }

    let http = http_client::build_stream_client()?;
    let body = http
        .get(YP_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(MhError::Network)?
        .text()
        .await
        .map_err(MhError::Network)?;

    let entries = parse_yp(&body)?;
    let mirror = Mirror {
        fetched_at: now_secs(),
        entries,
    };
    write_cache(path, &mirror).await;
    *cache().write().await = Some(Arc::new(mirror));
    Ok(())
}

async fn read_cache(path: &PathBuf) -> Option<Mirror> {
    let body = tokio::fs::read(path).await.ok()?;
    serde_json::from_slice(&body).ok()
}

async fn write_cache(path: &PathBuf, mirror: &Mirror) {
    let Ok(body) = serde_json::to_vec(mirror) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(path, body).await;
}

/// Streams the document rather than building a tree for it: 33,828 entries as a
/// DOM is tens of megabytes for a list we immediately flatten.
fn parse_yp(xml: &str) -> MhResult<Vec<Entry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut entries = Vec::with_capacity(16_000);
    let mut current: Option<Entry> = None;
    let mut field = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref tag)) => {
                let name = tag.name().into_inner();
                if name == "entry" {
                    current = Some(Entry::default());
                }
                field = name.to_string();
            }
            Ok(Event::Text(ref text)) => {
                let Some(entry) = current.as_mut() else {
                    continue;
                };
                let value = text.xml10_content().trim().to_string();
                if value.is_empty() {
                    continue;
                }
                match field.as_str() {
                    "server_name" => entry.name = value,
                    "listen_url" => entry.listen_url = value,
                    "server_type" => entry.server_type = Some(value),
                    "bitrate" => entry.bitrate = value.parse().ok(),
                    "genre" => entry.genre = Some(value),
                    "current_song" => entry.song = Some(value),
                    _ => {}
                }
            }
            Ok(Event::End(ref tag)) => {
                if tag.name().into_inner() == "entry" {
                    if let Some(mut entry) = current.take() {
                        // Two thirds of the document is servers that list
                        // themselves without saying where to listen.
                        if !entry.listen_url.is_empty() {
                            if entry.name.is_empty() || entry.name == "Unspecified name" {
                                entry.name = name_from_url(&entry.listen_url);
                            }
                            entries.push(entry);
                        }
                    }
                }
                field.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(MhError::Parse(format!("yp.xml: {e}"))),
            _ => {}
        }
    }
    Ok(entries)
}

#[async_trait]
impl RadioDirectory for XiphDirectory {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn label(&self) -> &'static str {
        "Icecast directory"
    }

    fn capabilities(&self) -> DirectoryCapabilities {
        DirectoryCapabilities {
            // `yp.xml` carries no country and no language, so offering those
            // filters here would return nothing and look broken.
            facets: vec![FacetKind::Tags, FacetKind::Codecs],
            paginates: false,
        }
    }

    /// Only "loading" when there is nothing to show yet; a refresh behind a
    /// populated mirror is not something to report.
    fn status(&self) -> DirectoryStatus {
        let warming = refreshing().load(Ordering::SeqCst)
            && cache().try_read().is_ok_and(|c| c.is_none());
        if warming {
            DirectoryStatus::Loading
        } else {
            DirectoryStatus::Ready
        }
    }

    async fn search(&self, query: &StationQuery) -> MhResult<Vec<Station>> {
        // Anything that reads the mirror is also what should be loading it.
        self.warm();
        // Filters this directory has no data for would silently match
        // everything, which is worse than matching nothing.
        if query.country_code.is_some() || query.language.is_some() {
            return Ok(Vec::new());
        }
        let Some(mirror) = self.mirror().await else {
            return Ok(Vec::new());
        };
        let filter = TextFilter::new(query);
        let stations = mirror
            .entries
            .iter()
            .filter(|e| {
                filter.matches(
                    &[e.name.as_str()],
                    e.genre.as_deref().unwrap_or_default(),
                )
            })
            .map(Entry::to_station)
            .collect();
        // Nothing here has a play count, so "popular" is read as "the
        // best-sounding", which at least orders by something real.
        Ok(page(stations, query, |s| s.bitrate.unwrap_or(0) as i64))
    }

    async fn browse(&self, kind: FacetKind, limit: u32) -> MhResult<Vec<Facet>> {
        // Anything that reads the mirror is also what should be loading it.
        self.warm();
        let Some(mirror) = self.mirror().await else {
            return Ok(Vec::new());
        };
        let mut counts: HashMap<String, i64> = HashMap::new();
        for entry in &mirror.entries {
            match kind {
                FacetKind::Tags => {
                    for genre in entry
                        .genre
                        .as_deref()
                        .unwrap_or_default()
                        .split([',', ' ', '|'])
                    {
                        let genre = genre.trim().to_lowercase();
                        if genre.len() > 2 && genre != "various" {
                            *counts.entry(genre).or_default() += 1;
                        }
                    }
                }
                FacetKind::Codecs => {
                    if let Some(codec) = entry.server_type.as_deref().and_then(codec_from_mime) {
                        *counts.entry(codec).or_default() += 1;
                    }
                }
                _ => return Ok(Vec::new()),
            }
        }
        Ok(facets_from_counts(counts, limit))
    }

    async fn resolve(&self, source_id: &str) -> MhResult<Option<Station>> {
        // Anything that reads the mirror is also what should be loading it.
        self.warm();
        Ok(self
            .mirror()
            .await
            .and_then(|mirror| {
                mirror
                    .entries
                    .iter()
                    .find(|e| e.listen_url == source_id)
                    .map(Entry::to_station)
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<directory>
  <entry>
    <server_name>Radio Nowhere</server_name>
    <server_type>audio/mpeg</server_type>
    <bitrate>128</bitrate>
    <samplerate>44100</samplerate>
    <channels>2</channels>
    <listen_url>http://listen.example:8000/stream</listen_url>
    <current_song>Someone - A Song</current_song>
    <genre>jazz blues</genre>
  </entry>
  <entry>
    <server_name>Unspecified name</server_name>
    <server_type>application/ogg</server_type>
    <bitrate>0</bitrate>
    <listen_url>http://other.example:8000/chillsynth.ogg</listen_url>
    <current_song></current_song>
    <genre>various</genre>
  </entry>
  <entry>
    <server_name>Listed But Silent</server_name>
    <server_type>audio/mpeg</server_type>
    <bitrate>96</bitrate>
    <listen_url></listen_url>
    <genre>talk</genre>
  </entry>
</directory>"#;

    #[test]
    fn parses_entries_and_drops_the_ones_with_nowhere_to_listen() {
        let entries = parse_yp(SAMPLE).unwrap();
        assert_eq!(entries.len(), 2, "the entry with no listen_url must go");
        assert_eq!(entries[0].name, "Radio Nowhere");
        assert_eq!(entries[0].bitrate, Some(128));
        assert_eq!(entries[0].song.as_deref(), Some("Someone - A Song"));
    }

    /// Thousands of servers publish no name; the mount is what tells them apart.
    #[test]
    fn an_unnamed_server_is_named_after_its_mount() {
        let entries = parse_yp(SAMPLE).unwrap();
        assert_eq!(entries[1].name, "chillsynth");
    }

    #[test]
    fn a_station_keeps_its_listen_url_as_its_id() {
        let station = parse_yp(SAMPLE).unwrap().remove(0).to_station();
        assert_eq!(station.key, "xiph:http://listen.example:8000/stream");
        assert_eq!(station.source_id, "http://listen.example:8000/stream");
        assert_eq!(station.codec.as_deref(), Some("MP3"));
        assert_eq!(station.tags.as_deref(), Some("jazz blues"));
    }

    /// Every one of these came out of the live document.
    #[test]
    fn bitrates_are_read_in_whichever_unit_the_server_chose() {
        // Plain kbps, including lossless, which is genuinely four figures.
        assert_eq!(normalize_bitrate(128), Some(128));
        assert_eq!(normalize_bitrate(1411), Some(1411), "CD-rate FLAC");
        assert_eq!(normalize_bitrate(6144), Some(6144), "24-bit FLAC");
        // Bits per second.
        assert_eq!(normalize_bitrate(320_000), Some(320));
        assert_eq!(normalize_bitrate(192_192), Some(192));
        assert_eq!(normalize_bitrate(160_009), Some(160));
        assert_eq!(normalize_bitrate(112_000), Some(112));
        // Absent, and the sentinel one host uses for "unknown".
        assert_eq!(normalize_bitrate(0), None);
        assert_eq!(normalize_bitrate(99_999), None);
    }

    #[test]
    fn a_zero_bitrate_is_absent_rather_than_zero() {
        let station = parse_yp(SAMPLE).unwrap().remove(1).to_station();
        assert_eq!(station.bitrate, None);
        assert_eq!(station.tags, None, "\"various\" is not a genre");
    }
}

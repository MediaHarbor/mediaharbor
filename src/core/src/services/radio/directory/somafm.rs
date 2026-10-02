use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::errors::{MhError, MhResult};
use crate::http_client;
use crate::services::common::http::{read_body, read_json};
use crate::services::radio::directory::{facets_from_counts, page, RadioDirectory, TextFilter};
use crate::services::radio::model::{
    DirectoryCapabilities, Facet, FacetKind, Station, StationQuery, StreamKind,
};
use crate::services::radio::playlist_parse::{self, PlaylistKind};

const USER_AGENT: &str = concat!("MediaHarbor/", env!("CARGO_PKG_VERSION"));
const CHANNELS_URL: &str = "https://somafm.com/channels.json";
const SERVICE: &str = "SomaFM";

/// The whole directory is one 50 KB document, so it is fetched once and shared.
/// A per-instance cache would never hit: an adapter is built per request.
const CHANNELS_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Deserialize)]
struct ChannelsResponse {
    channels: Vec<Channel>,
}

#[derive(Debug, Clone, Deserialize)]
struct Channel {
    id: String,
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    genre: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    largeimage: Option<String>,
    #[serde(default)]
    listeners: Option<String>,
    #[serde(default, rename = "lastPlaying")]
    last_playing: Option<String>,
    #[serde(default)]
    playlists: Vec<ChannelPlaylist>,
}

#[derive(Debug, Clone, Deserialize)]
struct ChannelPlaylist {
    url: String,
    #[serde(default)]
    format: String,
    #[serde(default)]
    quality: String,
}

impl Channel {
    fn into_station(self) -> Station {
        let (bitrate, codec) = {
            let best = self.playlists.iter().find(|p| p.quality == "highest");
            (
                best.and_then(|p| bitrate_from_url(&p.url)),
                best.map(|p| p.format.to_ascii_uppercase())
                    .filter(|c| !c.is_empty()),
            )
        };
        Station {
            homepage: Some("https://somafm.com".to_string()),
            favicon: self.largeimage.or(self.image),
            tags: self.genre.filter(|g| !g.is_empty()),
            country: Some("United States".to_string()),
            country_code: Some("US".to_string()),
            language: Some("english".to_string()),
            codec,
            bitrate,
            clickcount: self.listeners.and_then(|l| l.parse().ok()),
            // The stream URL is left empty on purpose: it lives behind a `.pls`,
            // and fetching 46 of them to build one browse page would cost 46
            // requests. `resolve` fills it in for the one station being played.
            ..Station::new(SomaFm::ID, &self.id, self.title, String::new())
        }
    }

    /// The stream to actually play: the best-quality MP3 where there is one —
    /// every WebView takes it — then the best of whatever else is offered.
    fn best_playlist(&self) -> Option<&ChannelPlaylist> {
        self.playlists
            .iter()
            .find(|p| p.quality == "highest" && p.format == "mp3")
            .or_else(|| self.playlists.iter().find(|p| p.quality == "highest"))
            .or_else(|| self.playlists.first())
    }
}

/// SomaFM names its streams `<channel>-<kbps>-<codec>` and its playlists
/// `<channel><kbps>.pls`; the channel document carries no bitrate field of its
/// own. Both forms end a token with the digits, so the trailing digit run is
/// what gets read.
fn bitrate_from_url(url: &str) -> Option<u32> {
    url.rsplit('/')
        .next()?
        .split(['-', '.'])
        .filter_map(|part| {
            let digits = part.trim_end_matches(|c: char| !c.is_ascii_digit());
            let start = digits
                .rfind(|c: char| !c.is_ascii_digit())
                .map_or(0, |i| i + 1);
            digits[start..].parse::<u32>().ok()
        })
        .find(|b| (16..=800).contains(b))
}

struct ChannelCache {
    fetched_at: Instant,
    channels: Vec<Channel>,
}

fn cache() -> &'static RwLock<Option<ChannelCache>> {
    static CACHE: OnceLock<RwLock<Option<ChannelCache>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub struct SomaFm {
    http: reqwest::Client,
}

impl SomaFm {
    pub const ID: &'static str = "somafm";

    pub fn new() -> MhResult<Self> {
        Ok(Self {
            http: http_client::ua_client(USER_AGENT)?,
        })
    }

    async fn get_json<T: DeserializeOwned>(&self, url: &str) -> MhResult<T> {
        let resp = self.http.get(url).send().await.map_err(MhError::Network)?;
        read_json(SERVICE, resp).await
    }

    async fn channels(&self) -> MhResult<Vec<Channel>> {
        if let Some(hit) = cache().read().await.as_ref() {
            if hit.fetched_at.elapsed() < CHANNELS_TTL {
                return Ok(hit.channels.clone());
            }
        }
        let parsed: ChannelsResponse = self.get_json(CHANNELS_URL).await?;
        *cache().write().await = Some(ChannelCache {
            fetched_at: Instant::now(),
            channels: parsed.channels.clone(),
        });
        Ok(parsed.channels)
    }

    /// The title SomaFM reports for a channel right now. HLS carries no in-band
    /// ICY, and a curated network publishes better titles than one anyway.
    pub async fn now_playing(&self, source_id: &str) -> MhResult<Option<String>> {
        // Deliberately bypasses the cache: a cached `lastPlaying` is a cached
        // *song*, which is the one thing here that must not be stale.
        let parsed: ChannelsResponse = self.get_json(CHANNELS_URL).await?;
        Ok(parsed
            .channels
            .into_iter()
            .find(|c| c.id == source_id)
            .and_then(|c| c.last_playing)
            .filter(|t| !t.trim().is_empty()))
    }
}

#[async_trait]
impl RadioDirectory for SomaFm {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn label(&self) -> &'static str {
        "SomaFM"
    }

    fn capabilities(&self) -> DirectoryCapabilities {
        DirectoryCapabilities {
            // 46 curated channels, all American and all English: a country or
            // language chip over them would be one chip.
            facets: vec![FacetKind::Tags],
            paginates: false,
        }
    }

    async fn search(&self, query: &StationQuery) -> MhResult<Vec<Station>> {
        // A country or language filter is a filter this directory cannot honour,
        // and answering it with everything would look like a bug.
        if query
            .country_code
            .as_deref()
            .is_some_and(|c| !c.eq_ignore_ascii_case("US"))
            || query
                .language
                .as_deref()
                .is_some_and(|l| !l.eq_ignore_ascii_case("english"))
        {
            return Ok(Vec::new());
        }
        let filter = TextFilter::new(query);
        let stations = self
            .channels()
            .await?
            .into_iter()
            .filter(|c| {
                let genre = c.genre.as_deref().unwrap_or_default();
                filter.matches(
                    &[
                        c.title.as_str(),
                        genre,
                        c.description.as_deref().unwrap_or_default(),
                    ],
                    genre,
                )
            })
            .map(Channel::into_station)
            .collect();
        Ok(page(stations, query, |s| s.clickcount.unwrap_or(0)))
    }

    async fn browse(&self, kind: FacetKind, limit: u32) -> MhResult<Vec<Facet>> {
        if kind != FacetKind::Tags {
            return Ok(Vec::new());
        }
        let mut counts: HashMap<String, i64> = HashMap::new();
        for channel in self.channels().await? {
            for genre in channel.genre.unwrap_or_default().split('|') {
                let genre = genre.trim();
                if !genre.is_empty() {
                    *counts.entry(genre.to_string()).or_default() += 1;
                }
            }
        }
        Ok(facets_from_counts(counts, limit))
    }

    async fn resolve(&self, source_id: &str) -> MhResult<Option<Station>> {
        let Some(channel) = self
            .channels()
            .await?
            .into_iter()
            .find(|c| c.id == source_id)
        else {
            return Ok(None);
        };
        let playlist_url = channel.best_playlist().map(|p| p.url.clone());
        let mut station = channel.into_station();
        let Some(playlist_url) = playlist_url else {
            return Err(MhError::NotFound(format!(
                "SomaFM channel {source_id} lists no streams"
            )));
        };
        let resp = self
            .http
            .get(&playlist_url)
            .send()
            .await
            .map_err(MhError::Network)?;
        match playlist_parse::parse(&read_body(SERVICE, resp).await?)? {
            PlaylistKind::Hls => {
                station.stream_url = playlist_url;
                station.stream_kind = StreamKind::Hls;
            }
            PlaylistKind::Entries(list) => {
                let mut urls = list.urls.into_iter();
                station.stream_url = urls.next().unwrap_or_default();
                station.stream_kind = StreamKind::guess(&station.stream_url);
                station.alt_urls = urls.collect();
            }
        }
        Ok(Some(station))
    }
}

#[cfg(test)]
mod tests {
    use super::bitrate_from_url;

    #[test]
    fn reads_the_bitrate_out_of_a_soma_stream_name() {
        assert_eq!(
            bitrate_from_url("https://ice6.somafm.com/7soul-128-mp3"),
            Some(128)
        );
        assert_eq!(
            bitrate_from_url("https://api.somafm.com/7soul32.pls"),
            Some(32)
        );
        assert_eq!(bitrate_from_url("https://api.somafm.com/7soul.pls"), None);
    }
}

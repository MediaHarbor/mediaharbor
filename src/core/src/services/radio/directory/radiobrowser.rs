use async_trait::async_trait;
use serde::Deserialize;

use crate::errors::{MhError, MhResult};
use crate::http_client;
use crate::services::common::http::{pct_encode, read_json};
use crate::services::common::limits;
use crate::services::radio::directory::{ranked, RadioDirectory};
use crate::services::radio::model::{
    DirectoryCapabilities, Facet, FacetKind, Station, StationQuery,
};

/// radio-browser asks every client to identify itself and rejects the default reqwest
/// agent, so the UA is not optional decoration.
const USER_AGENT: &str = concat!("MediaHarbor/", env!("CARGO_PKG_VERSION"));
const SERVICE: &str = "radio-browser";

/// How many tag completions the search box is offered.
const SUGGEST_LIMIT: usize = 12;

/// The mirrors are independent hosts behind one DNS name. `all.` round-robins across
/// whichever are healthy, so it is the fallback rather than the first choice: a fixed
/// host keeps a user's paging consistent while it is up.
const MIRRORS: &[&str] = &[
    "https://de1.api.radio-browser.info",
    "https://all.api.radio-browser.info",
];

#[derive(Debug, Clone, Default, Deserialize)]
struct ApiStation {
    #[serde(rename = "stationuuid")]
    uuid: String,
    name: String,
    #[serde(default)]
    url_resolved: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    favicon: Option<String>,
    #[serde(default)]
    tags: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    countrycode: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    codec: Option<String>,
    #[serde(default)]
    bitrate: Option<u32>,
    #[serde(default)]
    votes: Option<i64>,
    #[serde(default)]
    clickcount: Option<i64>,
}

impl ApiStation {
    /// `url_resolved` is the station's stream after redirects; `url` is what the
    /// submitter typed and is sometimes a playlist file.
    fn stream_url(&self) -> &str {
        if self.url_resolved.is_empty() {
            &self.url
        } else {
            &self.url_resolved
        }
    }

    fn into_station(self) -> Station {
        let url = self.stream_url().to_string();
        Station {
            homepage: self.homepage.filter(|s| !s.is_empty()),
            favicon: self.favicon.filter(|s| !s.is_empty()),
            tags: self.tags.filter(|s| !s.is_empty()),
            country: self.country.filter(|s| !s.is_empty()),
            country_code: self.countrycode.filter(|s| !s.is_empty()),
            language: self.language.filter(|s| !s.is_empty()),
            codec: self.codec.filter(|s| !s.is_empty()),
            bitrate: self.bitrate.filter(|b| *b > 0),
            votes: self.votes,
            clickcount: self.clickcount,
            ..Station::new(RadioBrowser::ID, &self.uuid, self.name, url)
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ApiFacet {
    name: String,
    #[serde(default)]
    stationcount: i64,
}

/// The country endpoint is the one facet that carries a display name beside the
/// value a query filters on, which is why `facets("countries")` used to show
/// `DE` where it meant Germany.
#[derive(Debug, Clone, Default, Deserialize)]
struct ApiCountry {
    name: String,
    #[serde(default)]
    iso_3166_1: String,
    #[serde(default)]
    stationcount: i64,
}

pub struct RadioBrowser {
    http: reqwest::Client,
}

impl RadioBrowser {
    pub const ID: &'static str = "radiobrowser";

    pub fn new() -> MhResult<Self> {
        Ok(Self {
            http: http_client::ua_client(USER_AGENT)?,
        })
    }

    /// Tries each mirror in turn and reports the last failure. Every mirror
    /// serves the same data, so a host that is down is worth stepping past —
    /// which is failover, not a retry, and so has no backoff.
    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> MhResult<T> {
        let mut last: Option<MhError> = None;
        for base in MIRRORS {
            let sent = self
                .http
                .get(format!("{base}{path}"))
                .query(query)
                .send()
                .await;
            match sent {
                Ok(resp) => match read_json(SERVICE, resp).await {
                    Ok(parsed) => return Ok(parsed),
                    Err(e) => last = Some(e),
                },
                Err(e) => last = Some(MhError::Network(e)),
            }
        }
        Err(last.unwrap_or_else(|| MhError::Other("no radio-browser mirror answered".into())))
    }
}

#[async_trait]
impl RadioDirectory for RadioBrowser {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn label(&self) -> &'static str {
        "radio-browser"
    }

    fn capabilities(&self) -> DirectoryCapabilities {
        DirectoryCapabilities {
            facets: vec![
                FacetKind::Tags,
                FacetKind::Countries,
                FacetKind::Languages,
                FacetKind::Codecs,
            ],
            paginates: true,
        }
    }

    async fn search(&self, req: &StationQuery) -> MhResult<Vec<Station>> {
        let limit = req
            .limit
            .unwrap_or(limits::RADIO_PAGE_DEFAULT)
            .min(limits::RADIO_BROWSER_PAGE_MAX)
            .to_string();
        let offset = req.offset.unwrap_or(0).to_string();
        let order = req.order.clone().unwrap_or_else(|| "clickcount".into());
        // `random` has no meaningful direction, and reversing `name` would list
        // the alphabet backwards; everything else wants the biggest first.
        let reverse = !matches!(order.as_str(), "random" | "name");
        let mut query: Vec<(&str, String)> = vec![
            ("limit", limit),
            ("offset", offset),
            ("order", order),
            ("reverse", reverse.to_string()),
            ("hidebroken", "true".into()),
        ];
        for (key, value) in [
            ("name", &req.name),
            ("tag", &req.tag),
            ("countrycode", &req.country_code),
            ("language", &req.language),
            ("codec", &req.codec),
        ] {
            if let Some(v) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                query.push((key, v.to_string()));
            }
        }
        if let Some(min) = req.bitrate_min.filter(|b| *b > 0) {
            query.push(("bitrateMin", min.to_string()));
        }
        let raw: Vec<ApiStation> = self.get("/json/stations/search", &query).await?;
        Ok(raw.into_iter().map(ApiStation::into_station).collect())
    }

    async fn browse(&self, kind: FacetKind, limit: u32) -> MhResult<Vec<Facet>> {
        let query = vec![
            ("limit", limit.to_string()),
            ("order", "stationcount".to_string()),
            ("reverse", "true".to_string()),
            ("hidebroken", "true".to_string()),
        ];
        if kind == FacetKind::Countries {
            let raw: Vec<ApiCountry> = self.get("/json/countries", &query).await?;
            return Ok(raw
                .into_iter()
                .filter(|c| !c.iso_3166_1.is_empty())
                .map(|c| Facet {
                    name: c.iso_3166_1,
                    label: c.name,
                    station_count: c.stationcount,
                })
                .collect());
        }
        let path = match kind {
            FacetKind::Tags => "/json/tags",
            FacetKind::Languages => "/json/languages",
            FacetKind::Codecs => "/json/codecs",
            FacetKind::Countries => unreachable!("handled above"),
        };
        let raw: Vec<ApiFacet> = self.get(path, &query).await?;
        Ok(raw
            .into_iter()
            .filter(|f| !f.name.is_empty())
            .map(|f| Facet::new(f.name, f.stationcount))
            .collect())
    }

    async fn resolve(&self, source_id: &str) -> MhResult<Option<Station>> {
        let found: Vec<ApiStation> = self
            .get("/json/stations/byuuid", &[("uuids", source_id.to_string())])
            .await?;
        Ok(found.into_iter().next().map(ApiStation::into_station))
    }

    /// The documented `?filter=` query parameter is silently ignored — it returns
    /// the global top tags whatever the prefix — so the prefix goes in the path.
    async fn suggest(&self, prefix: &str) -> MhResult<Vec<Facet>> {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            return Ok(Vec::new());
        }
        let raw: Vec<ApiFacet> = self
            .get(
                &format!("/json/tags/{}", pct_encode(prefix)),
                &[("hidebroken", "true".to_string())],
            )
            .await?;
        // The endpoint answers alphabetically; a suggestion list wants the tags
        // that actually have stations behind them at the top.
        Ok(ranked(
            raw.into_iter()
                .filter(|f| !f.name.is_empty())
                .map(|f| Facet::new(f.name, f.stationcount))
                .collect(),
            SUGGEST_LIMIT,
        ))
    }

    /// Registers a play with the directory. It is what feeds the popularity ordering
    /// the browse lists are sorted by, so a client that reads that ordering should
    /// contribute to it.
    async fn report_play(&self, source_id: &str) {
        for base in MIRRORS {
            let url = format!("{base}/json/url/{source_id}");
            if self.http.get(&url).send().await.is_ok() {
                return;
            }
        }
    }
}

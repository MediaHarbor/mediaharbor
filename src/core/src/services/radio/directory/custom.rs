use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::errors::MhResult;
use crate::media::library::Library;
use crate::services::radio::directory::{facets_from_counts, page, RadioDirectory, TextFilter};
use crate::services::radio::model::{
    station_key, DirectoryCapabilities, Facet, FacetKind, Station, StationQuery,
};
use crate::services::radio::store::RadioStore;

/// How much of the store one request will scan. The user's own stations are a
/// hand-curated list, so this is a guard against a pathological import rather
/// than a page size.
const SCAN_LIMIT: i64 = 2000;

/// The user's own streams. It is backed by the store rather than a feed, which
/// is why it can never be switched off — but it is still a `RadioDirectory`, so
/// nothing above has to special-case where a station came from.
pub struct CustomStations {
    library: Arc<Library>,
}

impl CustomStations {
    pub const ID: &'static str = "custom";

    pub fn new(library: Arc<Library>) -> Self {
        Self { library }
    }

    fn all(&self) -> MhResult<Vec<Station>> {
        RadioStore::by_source(self.library.db(), Self::ID, SCAN_LIMIT)
    }
}

#[async_trait]
impl RadioDirectory for CustomStations {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn label(&self) -> &'static str {
        "My stations"
    }

    fn toggleable(&self) -> bool {
        false
    }

    fn capabilities(&self) -> DirectoryCapabilities {
        DirectoryCapabilities {
            facets: vec![FacetKind::Tags],
            paginates: false,
        }
    }

    async fn search(&self, query: &StationQuery) -> MhResult<Vec<Station>> {
        let filter = TextFilter::new(query);
        let stations = self
            .all()?
            .into_iter()
            .filter(|s| {
                let tags = s.tags.as_deref().unwrap_or_default();
                filter.matches(&[s.name.as_str(), tags], tags)
            })
            .collect();
        // The store hands these back name-ordered and a station the user typed in
        // has no play count, so the incoming order is already the ranking.
        Ok(page(stations, query, |_| 0))
    }

    async fn browse(&self, kind: FacetKind, limit: u32) -> MhResult<Vec<Facet>> {
        if kind != FacetKind::Tags {
            return Ok(Vec::new());
        }
        let mut counts: HashMap<String, i64> = HashMap::new();
        for station in self.all()? {
            for tag in station.tags.unwrap_or_default().split(',') {
                let tag = tag.trim();
                if !tag.is_empty() {
                    *counts.entry(tag.to_lowercase()).or_default() += 1;
                }
            }
        }
        Ok(facets_from_counts(counts, limit))
    }

    async fn resolve(&self, source_id: &str) -> MhResult<Option<Station>> {
        RadioStore::get(self.library.db(), &station_key(Self::ID, source_id))
    }
}

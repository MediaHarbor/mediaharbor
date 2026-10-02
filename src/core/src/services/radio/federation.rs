//! One question, several directories, one answer.
//!
//! Everything here is cross-directory policy: which directories a request
//! reaches, what happens when one of them fails, and how their answers are
//! combined. It sits beside the directories rather than on `BackendState`
//! because none of it needs any other part of the backend — which is also what
//! makes it testable without one.

use std::sync::Arc;

use futures_util::future::join_all;

use crate::errors::MhResult;
use crate::media::library::Library;
use crate::services::common::limits;
use crate::services::radio::directory::{self, RadioDirectory};
use crate::services::radio::model::{
    split_key, DirectorySource, Facet, FacetKind, Station, StationQuery,
};
use crate::services::radio::store::{RadioStationDetail, RadioStore};

/// How many values a browse list carries. Enough to fill the facet rail without
/// paging it, which the directories could not do anyway.
const FACET_PAGE: u32 = 80;

/// How many completions the search box is offered while typing.
const SUGGEST_PAGE: u32 = 12;

/// The directories a user currently has switched on, ready to be asked.
pub struct Federation {
    library: Arc<Library>,
    enabled: Vec<String>,
}

impl Federation {
    pub fn new(library: Arc<Library>, enabled: Vec<String>) -> Self {
        Self { library, enabled }
    }

    /// The directories a query should actually reach: the ones the request named
    /// where it named any, and otherwise everything the user has switched on.
    fn directories(&self, requested: Option<&[String]>) -> Vec<Box<dyn RadioDirectory>> {
        match requested {
            Some(ids) if !ids.is_empty() => ids
                .iter()
                .filter_map(|id| directory::build(id, &self.library))
                .collect(),
            _ => directory::active(&self.enabled, &self.library),
        }
    }

    /// The directory behind a key, and the id to ask it for.
    fn directory_for<'k>(&self, key: &'k str) -> Option<(Box<dyn RadioDirectory>, &'k str)> {
        let (source, source_id) = split_key(key)?;
        Some((directory::build(source, &self.library)?, source_id))
    }

    pub fn sources(&self) -> Vec<DirectorySource> {
        directory::describe(&self.enabled, &self.library)
    }

    pub async fn search(&self, query: &StationQuery) -> MhResult<Vec<Station>> {
        let directories = self.directories(query.sources.as_deref());
        let answers = join_all(directories.iter().map(|d| d.search(query))).await;
        let pages = directory::collect(answers, "no station directory is enabled")?;
        let limit = query.limit.unwrap_or(limits::RADIO_PAGE_DEFAULT) as usize;
        Ok(directory::interleave(pages, limit))
    }

    pub async fn facets(
        &self,
        kind: FacetKind,
        source: Option<String>,
        limit: Option<u32>,
    ) -> MhResult<Vec<Facet>> {
        let limit = limit.unwrap_or(FACET_PAGE);
        let sources = source.map(|s| vec![s]);
        let directories = self.directories(sources.as_deref());
        let answers = join_all(
            directories
                .iter()
                .filter(|d| d.capabilities().facets.contains(&kind))
                .map(|d| d.browse(kind, limit)),
        )
        .await;
        let pages = directory::collect(answers, "no directory offers that facet")?;
        Ok(directory::merge_facets(pages, limit as usize))
    }

    /// Prefix completions, merged across directories. Anything that fails is
    /// dropped rather than surfaced: a suggestion list is not worth an error
    /// banner while the user is still typing.
    pub async fn suggest(&self, prefix: &str, limit: Option<u32>) -> Vec<Facet> {
        let limit = limit.unwrap_or(SUGGEST_PAGE);
        let directories = self.directories(None);
        let answers = join_all(directories.iter().map(|d| d.suggest(prefix))).await;
        let pages = answers.into_iter().filter_map(Result::ok).collect();
        directory::merge_facets(pages, limit as usize)
    }

    /// The station as its directory publishes it right now, remembered on the
    /// way past. A source with no directory behind it has only the store.
    pub async fn station(&self, key: &str) -> MhResult<Option<Station>> {
        let db = self.library.db();
        let Some((dir, source_id)) = self.directory_for(key) else {
            return RadioStore::get(db, key);
        };
        let found = dir.resolve(source_id).await?;
        if let Some(station) = &found {
            RadioStore::remember(db, station)?;
        }
        Ok(found)
    }

    /// Guarantees there is a stored row for `key`, resolving it from its
    /// directory if there is not.
    ///
    /// Favouriting, adding to a list and opening the editor all reach stations
    /// the user has so far only seen in a search result, and all three need a row
    /// to point at — so all three come through here.
    pub async fn ensure_station(&self, key: &str) -> MhResult<Option<Station>> {
        let db = self.library.db();
        if let Some(stored) = RadioStore::get(db, key)? {
            return Ok(Some(stored));
        }
        let Some((dir, source_id)) = self.directory_for(key) else {
            return Ok(None);
        };
        let Some(station) = dir.resolve(source_id).await? else {
            return Ok(None);
        };
        RadioStore::remember(db, &station)?;
        Ok(Some(station))
    }

    /// The same for a batch. The directories are asked concurrently — adding
    /// twenty search results to a playlist is one round of requests, not twenty
    /// in a row.
    pub async fn ensure_stations(&self, keys: &[String]) -> MhResult<()> {
        let db = self.library.db();
        let known = RadioStore::existing_keys(db, keys)?;
        let missing: Vec<&String> = keys.iter().filter(|k| !known.contains(*k)).collect();
        if missing.is_empty() {
            return Ok(());
        }
        let resolved = join_all(missing.into_iter().map(|key| async move {
            match self.directory_for(key) {
                Some((dir, source_id)) => dir.resolve(source_id).await.ok().flatten(),
                None => None,
            }
        }))
        .await;
        for station in resolved.into_iter().flatten() {
            RadioStore::remember(db, &station)?;
        }
        Ok(())
    }

    /// The station as the directory published it, alongside the user's changes.
    /// The editor needs both so it can offer "reset to the directory's value".
    pub async fn station_detail(&self, key: &str) -> MhResult<Option<RadioStationDetail>> {
        let db = self.library.db();
        if let Some(detail) = RadioStore::detail(db, key)? {
            return Ok(Some(detail));
        }
        // A station being edited straight out of a search result has no row yet.
        if self.ensure_station(key).await?.is_none() {
            return Ok(None);
        }
        RadioStore::detail(db, key)
    }
}

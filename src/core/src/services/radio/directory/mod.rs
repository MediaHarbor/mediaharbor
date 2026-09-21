pub mod custom;
pub mod radiobrowser;
pub mod somafm;
pub mod xiph;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::errors::MhResult;
use crate::media::library::Library;
use crate::services::common::limits;
use crate::services::radio::model::{
    DirectoryCapabilities, DirectorySource, DirectoryStatus, Facet, FacetKind, Station,
    StationQuery,
};

pub use crate::services::radio::model::DirectoryStatus as Status;
pub use custom::CustomStations;
pub use radiobrowser::RadioBrowser;
pub use somafm::SomaFm;
pub use xiph::XiphDirectory;

/// One station directory. Adapters keep their own wire DTOs private and hand out
/// [`Station`], so nothing above this trait knows which service answered.
#[async_trait]
pub trait RadioDirectory: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn capabilities(&self) -> DirectoryCapabilities;

    /// Whether the user can switch this directory off. Only the user's own
    /// stations cannot: they are data rather than a feed, so there is nothing
    /// to disable.
    fn toggleable(&self) -> bool {
        true
    }

    /// Whether the directory can answer yet. Only one has a warm-up worth
    /// reporting, so the rest inherit "ready".
    fn status(&self) -> DirectoryStatus {
        DirectoryStatus::Ready
    }

    async fn search(&self, query: &StationQuery) -> MhResult<Vec<Station>>;
    async fn browse(&self, kind: FacetKind, limit: u32) -> MhResult<Vec<Facet>>;
    async fn resolve(&self, source_id: &str) -> MhResult<Option<Station>>;

    /// Prefix completions for the search box. Empty is a valid answer — a
    /// directory of 46 channels has nothing to suggest.
    async fn suggest(&self, _prefix: &str) -> MhResult<Vec<Facet>> {
        Ok(Vec::new())
    }

    /// Best-effort popularity reporting. A directory whose browse lists are
    /// ordered by play count should contribute to that ordering.
    async fn report_play(&self, _source_id: &str) {}
}

/// Every directory there is, in display order: the feeds first, then the user's
/// own stations, which are always present.
pub const ALL: &[&str] = &[
    RadioBrowser::ID,
    SomaFm::ID,
    XiphDirectory::ID,
    CustomStations::ID,
];

/// What a fresh install enables. Every toggleable entry of [`ALL`] — an
/// invariant the tests below pin rather than a second list to keep in step.
pub const DEFAULT_SOURCES: &[&str] = &[RadioBrowser::ID, SomaFm::ID, XiphDirectory::ID];

/// `None` for an id no directory answers to. Construction itself cannot
/// meaningfully fail per directory: the only error underneath is the shared HTTP
/// client failing to load the TLS root store, which is a process-wide condition.
pub fn build(id: &str, library: &Arc<Library>) -> Option<Box<dyn RadioDirectory>> {
    Some(match id {
        RadioBrowser::ID => Box::new(RadioBrowser::new().ok()?) as Box<dyn RadioDirectory>,
        SomaFm::ID => Box::new(SomaFm::new().ok()?),
        XiphDirectory::ID => Box::new(XiphDirectory::new(library)),
        CustomStations::ID => Box::new(CustomStations::new(library.clone())),
        _ => return None,
    })
}

/// Every directory that should answer a query: the enabled feeds, plus anything
/// that has no switch to be disabled by.
pub fn active(enabled: &[String], library: &Arc<Library>) -> Vec<Box<dyn RadioDirectory>> {
    ALL.iter()
        .filter_map(|id| build(id, library))
        .filter(|dir| !dir.toggleable() || enabled.iter().any(|e| e == dir.id()))
        .collect()
}

/// The full roster with each directory's on/off state, for the settings UI and
/// the source chips on the radio page.
pub fn describe(enabled: &[String], library: &Arc<Library>) -> Vec<DirectorySource> {
    ALL.iter()
        .filter_map(|id| build(id, library))
        .map(|dir| {
            let toggleable = dir.toggleable();
            DirectorySource {
                id: dir.id().to_string(),
                label: dir.label().to_string(),
                capabilities: dir.capabilities(),
                enabled: !toggleable || enabled.iter().any(|e| e == dir.id()),
                toggleable,
                status: dir.status(),
            }
        })
        .collect()
}

/// The name and tag halves of a query, prepared once instead of per station.
///
/// Both are substring matches folded to lowercase, which is what a directory
/// with no search index behind it can honestly promise. An absent half matches
/// everything and costs nothing — the haystack is never even lowercased.
pub(super) struct TextFilter {
    name: String,
    tag: String,
}

impl TextFilter {
    pub(super) fn new(query: &StationQuery) -> Self {
        Self {
            name: fold(query.name.as_deref()),
            tag: fold(query.tag.as_deref()),
        }
    }

    /// `haystack` is every field the directory is willing to match a name
    /// against; `tags` is the one a tag filter has to hit on its own.
    pub(super) fn matches(&self, haystack: &[&str], tags: &str) -> bool {
        if !self.name.is_empty() && !haystack.iter().any(|h| contains_folded(h, &self.name)) {
            return false;
        }
        if !self.tag.is_empty() && !contains_folded(tags, &self.tag) {
            return false;
        }
        true
    }
}

fn fold(value: Option<&str>) -> String {
    value.unwrap_or_default().trim().to_lowercase()
}

fn contains_folded(haystack: &str, folded_needle: &str) -> bool {
    haystack.to_lowercase().contains(folded_needle)
}

/// The tail every local directory shares: drop what the query excludes, order,
/// and cut the requested window.
///
/// `rank` is the only part that genuinely differs — what "popular" means to a
/// directory that publishes no play counts. Sorting is stable, so a directory
/// whose rank is constant keeps whatever order it handed in.
pub(super) fn page(
    mut stations: Vec<Station>,
    query: &StationQuery,
    rank: impl Fn(&Station) -> i64,
) -> Vec<Station> {
    if let Some(codec) = query.codec.as_deref().filter(|c| !c.is_empty()) {
        stations.retain(|s| {
            s.codec
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(codec))
        });
    }
    if let Some(min) = query.bitrate_min.filter(|b| *b > 0) {
        stations.retain(|s| s.bitrate.unwrap_or(0) >= min);
    }
    match query.order.as_deref() {
        Some("name") => stations.sort_by_key(|s| s.name.to_lowercase()),
        _ => stations.sort_by_key(|s| std::cmp::Reverse(rank(s))),
    }
    let offset = query.offset.unwrap_or(0) as usize;
    let limit = query.limit.unwrap_or(limits::RADIO_PAGE_DEFAULT) as usize;
    stations.into_iter().skip(offset).take(limit).collect()
}

/// Turns a directory's own tally into the page it publishes.
pub(super) fn facets_from_counts(counts: HashMap<String, i64>, limit: u32) -> Vec<Facet> {
    ranked(
        counts
            .into_iter()
            .map(|(name, count)| Facet::new(name, count))
            .collect(),
        limit as usize,
    )
}

/// Sums the counts for a facet several directories both carry, keeping the first
/// human-readable label offered for it.
pub fn merge_facets(pages: Vec<Vec<Facet>>, limit: usize) -> Vec<Facet> {
    let mut merged: HashMap<String, Facet> = HashMap::new();
    for facet in pages.into_iter().flatten() {
        match merged.get_mut(&facet.name) {
            Some(existing) => existing.station_count += facet.station_count,
            None => {
                merged.insert(facet.name.clone(), facet);
            }
        }
    }
    ranked(merged.into_values().collect(), limit)
}

/// The one order facets are ever published in: biggest first, ties by name so
/// the list does not reshuffle between two identical requests.
pub(super) fn ranked(mut facets: Vec<Facet>, limit: usize) -> Vec<Facet> {
    facets.sort_by(|a, b| {
        b.station_count
            .cmp(&a.station_count)
            .then_with(|| a.name.cmp(&b.name))
    });
    facets.truncate(limit);
    facets
}

/// Round-robins the sources instead of concatenating them.
///
/// A directory of 46 curated channels and one of 46 000 crowd-sourced ones have
/// no comparable score to sort by, and concatenating would bury the small one
/// under a full page of the large one every time.
pub fn interleave<T>(pages: Vec<Vec<T>>, limit: usize) -> Vec<T> {
    let mut iters: Vec<_> = pages.into_iter().map(Vec::into_iter).collect();
    let mut out = Vec::new();
    while out.len() < limit {
        let before = out.len();
        for iter in &mut iters {
            if let Some(item) = iter.next() {
                out.push(item);
                if out.len() >= limit {
                    return out;
                }
            }
        }
        if out.len() == before {
            break;
        }
    }
    out
}

/// Keeps whatever answered and only fails when nothing did, so one dead mirror
/// does not blank a page the other directories could have filled. The error that
/// does surface is the upstream one, not a summary of it.
pub fn collect<T>(answers: Vec<MhResult<Vec<T>>>, empty: &str) -> MhResult<Vec<Vec<T>>> {
    let mut pages = Vec::new();
    let mut failure = None;
    for answer in answers {
        match answer {
            Ok(page) => pages.push(page),
            Err(e) => failure = Some(e),
        }
    }
    if pages.is_empty() {
        return Err(failure
            .unwrap_or_else(|| crate::errors::MhError::Other(empty.to_string())));
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station(name: &str, bitrate: Option<u32>, codec: Option<&str>) -> Station {
        Station {
            name: name.to_string(),
            bitrate,
            codec: codec.map(str::to_string),
            ..Default::default()
        }
    }

    fn query() -> StationQuery {
        StationQuery::default()
    }

    /// The two tables are separate because `DEFAULT_SOURCES` has to be a const
    /// for the settings defaults, and `toggleable()` lives on the instance. This
    /// pins the relationship so they cannot drift.
    #[test]
    fn default_sources_is_all_minus_the_directory_with_no_switch() {
        let expected: Vec<&str> = ALL
            .iter()
            .copied()
            .filter(|id| *id != CustomStations::ID)
            .collect();
        assert_eq!(DEFAULT_SOURCES, expected.as_slice());
    }

    #[test]
    fn a_constant_rank_leaves_the_incoming_order_alone() {
        let stations = vec![station("Zed", None, None), station("Alpha", None, None)];
        let paged = page(stations, &query(), |_| 0);
        assert_eq!(
            paged.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["Zed", "Alpha"]
        );
    }

    #[test]
    fn ordering_by_name_is_case_insensitive() {
        let stations = vec![station("zed", None, None), station("Alpha", None, None)];
        let mut q = query();
        q.order = Some("name".into());
        let paged = page(stations, &q, |_| 0);
        assert_eq!(
            paged.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["Alpha", "zed"]
        );
    }

    #[test]
    fn codec_and_bitrate_filters_are_applied_before_the_window() {
        let stations = vec![
            station("a", Some(64), Some("mp3")),
            station("b", Some(320), Some("MP3")),
            station("c", Some(320), Some("aac")),
        ];
        let mut q = query();
        q.codec = Some("mp3".into());
        q.bitrate_min = Some(128);
        let paged = page(stations, &q, |_| 0);
        assert_eq!(
            paged.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["b"]
        );
    }

    #[test]
    fn offset_and_limit_cut_a_window_out_of_the_ordered_list() {
        let stations = (0..10)
            .map(|i| station(&format!("s{i}"), None, None))
            .collect();
        let mut q = query();
        q.offset = Some(3);
        q.limit = Some(2);
        let paged = page(stations, &q, |_| 0);
        assert_eq!(
            paged.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["s3", "s4"]
        );
    }

    #[test]
    fn an_absent_filter_half_matches_everything() {
        let filter = TextFilter::new(&query());
        assert!(filter.matches(&["anything"], ""));
    }

    #[test]
    fn name_matches_any_haystack_field_and_tag_only_the_tag_field() {
        let mut q = query();
        q.name = Some("  AMBIENT ".into());
        q.tag = Some("Jazz".into());
        let filter = TextFilter::new(&q);
        assert!(filter.matches(&["Deep Ambient Sounds", ""], "jazz, blues"));
        // The name hit may come from any field...
        assert!(filter.matches(&["Untitled", "an ambient mix"], "jazz"));
        // ...but the tag has to hit the tag field, not the description.
        assert!(!filter.matches(&["ambient", "jazz in the description"], "blues"));
    }

    #[test]
    fn facets_are_ordered_by_count_then_name() {
        let counts = HashMap::from([
            ("rock".to_string(), 5),
            ("ambient".to_string(), 9),
            ("blues".to_string(), 5),
        ]);
        let facets = facets_from_counts(counts, 10);
        assert_eq!(
            facets.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["ambient", "blues", "rock"]
        );
    }

    #[test]
    fn merging_sums_a_facet_two_directories_both_carry() {
        let merged = merge_facets(
            vec![
                vec![Facet::new("jazz", 3), Facet::new("rock", 1)],
                vec![Facet::new("jazz", 4)],
            ],
            10,
        );
        assert_eq!(merged[0].name, "jazz");
        assert_eq!(merged[0].station_count, 7);
    }

    #[test]
    fn interleave_takes_from_every_page_before_exhausting_one() {
        let out = interleave(vec![vec![1, 2, 3, 4], vec![10, 20]], 4);
        assert_eq!(out, [1, 10, 2, 20]);
    }

    #[test]
    fn collect_keeps_the_pages_that_answered_and_reports_the_real_error() {
        let answers: Vec<MhResult<Vec<u8>>> = vec![
            Err(crate::errors::MhError::Other("mirror down".into())),
            Ok(vec![1, 2]),
        ];
        assert_eq!(collect(answers, "nothing").unwrap(), vec![vec![1, 2]]);

        let all_failed: Vec<MhResult<Vec<u8>>> =
            vec![Err(crate::errors::MhError::Other("mirror down".into()))];
        assert!(collect(all_failed, "nothing")
            .unwrap_err()
            .to_string()
            .contains("mirror down"));
    }
}

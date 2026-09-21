//! One pass over Spotify's web player bundles.
//!
//! Several things exist nowhere except the shipped JavaScript: the GraphQL
//! persisted-query hashes, and the TOTP secret `/api/token` checks. Both rotate
//! on a Spotify deploy, and both used to be fetched by separate code paths that
//! downloaded the same megabyte of script twice and cached it under two
//! different policies. This module does the walk once and hands out the result.
//!
//! Anything Spotify publishes from a real endpoint does NOT belong here — the
//! regional `spclient` host comes from `apresolve`, see `super::endpoints`.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::totp_secrets::{self, TotpSecret};
use crate::errors::{MhError, MhResult};
use crate::services::common::ids::now_secs;

const HOMEPAGE: &str = "https://open.spotify.com/";
/// How many of the page's bundles to walk before giving up.
const MAX_BUNDLES: usize = 8;

static BUNDLE_URL_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r#"https://[a-z0-9.-]*spotifycdn\.com/[a-zA-Z0-9_./-]+\.js"#).unwrap()
});

static QUERY_PAIR_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(
        r#"new\s+[\w$.]+\("([A-Za-z][A-Za-z0-9_]+)","(?:query|mutation)","([0-9a-f]{64})""#,
    )
    .unwrap()
});

static ROUTE_CHUNK_RES: std::sync::LazyLock<Vec<(&'static str, regex::Regex)>> =
    std::sync::LazyLock::new(|| {
        ["xpui-routes-search", "browse-v2"]
            .into_iter()
            .map(|chunk| {
                (
                    chunk,
                    regex::Regex::new(&format!(r#"(\d+):"({chunk})""#)).unwrap(),
                )
            })
            .collect()
    });

/// Everything one sweep of the bundles yields. Persisted as-is by the library,
/// so a restart does not re-download to answer either consumer.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct WebPlayerAssets {
    pub scraped_at: u64,
    pub hashes: HashMap<String, String>,
    /// Absent on caches written before the TOTP secret joined this sweep.
    #[serde(default)]
    pub totp: Option<TotpSecret>,
}

/// Shared across the library and the session so one scrape serves both.
static MEMO: std::sync::LazyLock<RwLock<Option<WebPlayerAssets>>> =
    std::sync::LazyLock::new(|| RwLock::new(None));

/// Primes the in-process copy from whatever was persisted last run, so a cold
/// start answers both consumers without touching the network.
pub async fn seed(assets: WebPlayerAssets) {
    if assets.hashes.is_empty() && assets.totp.is_none() {
        return;
    }
    *MEMO.write().await = Some(assets);
}

/// What was already scraped this run, if anything.
pub async fn cached() -> Option<WebPlayerAssets> {
    MEMO.read().await.clone()
}

/// The cached sweep, scraping once if nothing is loaded yet.
pub async fn cached_or_scrape(client: &reqwest::Client) -> MhResult<WebPlayerAssets> {
    if let Some(hit) = cached().await {
        return Ok(hit);
    }
    scrape(client).await
}

/// A fresh sweep. Used when Spotify has rotated something and the cached copy is
/// known stale; the result replaces the shared copy.
pub async fn scrape(client: &reqwest::Client) -> MhResult<WebPlayerAssets> {
    let html = client
        .get(HOMEPAGE)
        .send()
        .await
        .map_err(MhError::Network)?
        .text()
        .await
        .map_err(MhError::Network)?;

    let mut urls: Vec<String> = BUNDLE_URL_RE
        .find_iter(&html)
        .map(|m| m.as_str().to_string())
        .collect();
    urls.sort();
    urls.dedup();

    let mut hashes: HashMap<String, String> = HashMap::new();
    let mut totp: Option<TotpSecret> = None;
    let mut queue: Vec<String> = urls.into_iter().take(MAX_BUNDLES).collect();
    let mut visited: HashSet<String> = HashSet::new();

    while let Some(url) = queue.pop() {
        if !visited.insert(url.clone()) {
            continue;
        }
        let body = match client.get(&url).send().await {
            Ok(r) => r.text().await.unwrap_or_default(),
            Err(_) => continue,
        };

        for cap in QUERY_PAIR_RE.captures_iter(&body) {
            hashes
                .entry(cap[1].to_string())
                .or_insert_with(|| cap[2].to_string());
        }
        if totp.is_none() {
            totp = totp_secrets::best_in(&body);
        }

        for (chunk, name_re) in ROUTE_CHUNK_RES.iter() {
            if let Some(cap) = name_re.captures(&body) {
                let chunk_id = &cap[1];
                let Ok(hash_re) = regex::Regex::new(&format!(r#"{chunk_id}:"([0-9a-f]{{6,12}})""#))
                else {
                    continue;
                };
                if let Some(hc) = hash_re.captures(&body) {
                    let full = format!(
                        "https://open.spotifycdn.com/cdn/build/web-player/{chunk}.{}.js",
                        &hc[1]
                    );
                    if !visited.contains(&full) {
                        queue.push(full);
                    }
                }
            }
        }

        if hashes.contains_key("searchTracks")
            && hashes.contains_key("browsePage")
            && hashes.len() >= 110
            && totp.is_some()
        {
            break;
        }
    }

    if hashes.is_empty() && totp.is_none() {
        return Err(MhError::Other(
            "Spotify web player scrape found neither pathfinder hashes nor a TOTP secret — \
             the bundle format may have changed"
                .into(),
        ));
    }

    let assets = WebPlayerAssets {
        scraped_at: now_secs(),
        hashes,
        totp,
    };
    *MEMO.write().await = Some(assets.clone());
    Ok(assets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_yields_only_its_spotify_cdn_scripts() {
        let html = r#"<script src="https://open.spotifycdn.com/cdn/build/web-player/web-player.0d1ded8b.js"></script>
            <script src="https://example.com/other.js"></script>
            <script src="https://open.spotifycdn.com/cdn/js/gtm.96d60fd6.js"></script>"#;
        let urls: Vec<&str> = BUNDLE_URL_RE.find_iter(html).map(|m| m.as_str()).collect();
        assert_eq!(urls.len(), 2);
        assert!(urls.iter().all(|u| u.contains("spotifycdn.com")));
    }

    /// The hash and the secret come out of the same bundle text, which is the
    /// whole reason this is one pass instead of two.
    #[test]
    fn one_body_yields_both_a_hash_and_the_secret() {
        let body = concat!(
            r#"new e.T("searchTracks","query","aaaaaaaabbbbbbbbccccccccddddddddeeeeeeeeffffffff0000000011111111"),"#,
            r#"let eD=[{secret:',7/*F("rLJ2oxaKL^f+E1xvP@N',version:61}].map(e=>{"#
        );
        let hashes: Vec<String> = QUERY_PAIR_RE
            .captures_iter(body)
            .map(|c| c[1].to_string())
            .collect();
        assert_eq!(hashes, vec!["searchTracks".to_string()]);
        assert_eq!(totp_secrets::best_in(body).map(|s| s.version), Some(61));
    }

    #[test]
    fn a_cache_written_before_totp_joined_still_loads() {
        let old = r#"{"scraped_at":1700000000,"hashes":{"searchTracks":"abc"}}"#;
        let parsed: WebPlayerAssets = serde_json::from_str(old).expect("older cache still parses");
        assert!(parsed.totp.is_none());
        assert_eq!(parsed.hashes.len(), 1);
    }
}

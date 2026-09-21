//! Where Spotify's per-region API actually lives.
//!
//! The regional prefix on `spclient` (`gue1-`, `gew4-`, `guc3-`, `gae2-`…) is
//! assigned per account and location, so pinning one in a constant works only for
//! whoever happened to be sitting next to that datacentre when the code was
//! written. Spotify publishes the live list from `apresolve`, ordered nearest
//! first, which is what the desktop and mobile clients use.

use std::time::{Duration, Instant};

use tokio::sync::RwLock;

const APRESOLVE_URL: &str = "https://apresolve.spotify.com/?type=spclient";
/// The unregionalised alias. It answers everywhere, so it is both the fallback
/// and the reason a stale regional constant fails quietly rather than loudly.
pub const SPCLIENT_FALLBACK: &str = "spclient.wg.spotify.com";
const CACHE_TTL: Duration = Duration::from_secs(60 * 60);

static CACHE: std::sync::LazyLock<RwLock<Option<(String, Instant)>>> =
    std::sync::LazyLock::new(|| RwLock::new(None));

/// The nearest `spclient` host, memoised for an hour. Never fails: an unreachable
/// resolver falls back to the generic alias rather than blocking a download.
pub async fn spclient_host(client: &reqwest::Client) -> String {
    if let Some((host, fetched)) = CACHE.read().await.as_ref() {
        if fetched.elapsed() < CACHE_TTL {
            return host.clone();
        }
    }

    let resolved = fetch_spclient_host(client)
        .await
        .unwrap_or_else(|| SPCLIENT_FALLBACK.to_string());
    *CACHE.write().await = Some((resolved.clone(), Instant::now()));
    resolved
}

/// `https://{nearest spclient}` — the form most call sites want.
pub async fn spclient_base(client: &reqwest::Client) -> String {
    format!("https://{}", spclient_host(client).await)
}

async fn fetch_spclient_host(client: &reqwest::Client) -> Option<String> {
    let resp = client
        .get(APRESOLVE_URL)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    body["spclient"]
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str())
        .map(|entry| entry.split(':').next().unwrap_or(entry).to_string())
        .find(|host| !host.is_empty())
}

#[cfg(test)]
mod tests {
    fn parse(body: serde_json::Value) -> Option<String> {
        body["spclient"]
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str())
            .map(|entry| entry.split(':').next().unwrap_or(entry).to_string())
            .find(|host| !host.is_empty())
    }

    /// The captured `apresolve` payload: nearest region first, `host:port` form.
    #[test]
    fn the_nearest_host_is_taken_and_the_port_dropped() {
        let body = serde_json::json!({
            "spclient": [
                "gew4-spclient.spotify.com:443",
                "guc3-spclient.spotify.com:443",
                "gue1-spclient.spotify.com:443",
                "gae2-spclient.spotify.com:443"
            ]
        });
        assert_eq!(parse(body), Some("gew4-spclient.spotify.com".to_string()));
    }

    #[test]
    fn an_unusable_payload_yields_nothing_so_the_caller_falls_back() {
        assert_eq!(parse(serde_json::json!({})), None);
        assert_eq!(parse(serde_json::json!({ "spclient": [] })), None);
    }
}

//! Deezer anti-ban telemetry (Phase F).
//!
//! MediaHarbor normally makes only the API calls it needs. The real Deezer web
//! client continuously emits Braze analytics beacons (`ss` session-start /
//! `se` session-end) to `sdk.iad-01.braze.com/api/v3/data/`. A fetch-only client
//! that never sends these is a ban fingerprint. When the per-service opt-in
//! (`deezer_telemetry_enabled`, default OFF) is on, we replicate that traffic:
//! after a genuine recommendations/explore load we fire a Braze `ss` event, tying
//! the telemetry to real user activity as a real client does.
//!
//! Body shape reverse-engineered from a real web-client capture (HAR). Braze is a
//! public, documented SDK; we mimic the **web** integration (`sdk_metadata:["npm"]`,
//! `sdk_version 6.7.1`), consistent with MediaHarbor's web-API Deezer path.

use std::sync::OnceLock;

use crate::services::common::ids::{now_millis, rand_uuid};
use serde_json::{json, Value};
use tokio::sync::OnceCell;

const BRAZE_URL: &str = "https://sdk.iad-01.braze.com/api/v3/data/";
const API_KEY: &str = "5ba97124-1b79-4acc-86b7-9547bc58cb18";
const SDK_VERSION: &str = "6.7.1";
const FALLBACK_CHROME_MAJOR: &str = "150";
const CHROME_VERSION_URL: &str =
    "https://versionhistory.googleapis.com/v1/chrome/platforms/linux/channels/stable/versions";

/// The current stable Chrome major version, fetched once from Google's public
/// version-history API. A frozen browser version ages into a ban fingerprint, so
/// we track the real one. Falls back to [`FALLBACK_CHROME_MAJOR`].
async fn chrome_major() -> &'static str {
    static V: OnceCell<String> = OnceCell::const_new();
    V.get_or_init(|| async {
        let fetched = async {
            let client = crate::http_client::shared_client().ok()?;
            let v: Value = client
                .get(CHROME_VERSION_URL)
                .send()
                .await
                .ok()?
                .json()
                .await
                .ok()?;
            v.pointer("/versions/0/version")
                .and_then(|x| x.as_str())
                .and_then(|s| s.split('.').next())
                .filter(|s| s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty())
                .map(str::to_string)
        }
        .await;
        fetched.unwrap_or_else(|| FALLBACK_CHROME_MAJOR.to_string())
    })
    .await
}

fn user_agent(chrome_major: &str) -> String {
    format!(
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{chrome_major}.0.0.0 Safari/537.36"
    )
}

/// A stable per-process Braze identity (generated once, reused this run), mirroring
/// the real client's persistent `device_id` and per-run `session_id`.
struct Session {
    device_id: String,
    session_id: String,
}

fn session() -> &'static Session {
    static S: OnceLock<Session> = OnceLock::new();
    S.get_or_init(|| Session {
        device_id: rand_uuid(),
        session_id: rand_uuid(),
    })
}

/// Build the Braze `/api/v3/data/` request body for one event (`ss` or `se`).
fn data_body(name: &str, user_id: &str, duration_secs: Option<u64>, chrome_major: &str) -> Value {
    let s = session();
    let ts = (now_millis() / 1000) as u64;
    let mut event = json!({
        "name": name,
        "time": ts,
        "session_id": s.session_id,
        "user_id": user_id,
    });
    if let Some(d) = duration_secs {
        event["data"] = json!({ "d": d });
    }
    json!({
        "respond_with": { "user_id": user_id },
        "events": [event],
        "device": {
            "browser": "Chrome",
            "browser_version": format!("{chrome_major}.0.0.0"),
            "os_version": "Linux",
            "locale": "en",
            "time_zone": "UTC",
            "user_agent": user_agent(chrome_major),
        },
        "api_key": API_KEY,
        "time": ts,
        "sdk_metadata": ["npm"],
        "sdk_version": SDK_VERSION,
        "device_id": s.device_id,
    })
}

async fn send(body: Value, chrome_major: &str) {
    let client = match crate::http_client::shared_client() {
        Ok(c) => c,
        Err(_) => return,
    };
    let _ = client
        .post(BRAZE_URL)
        .header("Content-Type", "application/json")
        .header("Origin", "https://www.deezer.com")
        .header("User-Agent", user_agent(chrome_major))
        .json(&body)
        .send()
        .await;
}

async fn session_start(user_id: &str) {
    let cm = chrome_major().await;
    send(data_body("ss", user_id, None, cm), cm).await;
}

async fn session_end(user_id: &str, duration_secs: u64) {
    let cm = chrome_major().await;
    send(data_body("se", user_id, Some(duration_secs), cm), cm).await;
}

/// Sessions idle longer than this are considered ended; the next activity closes
/// the stale session (`se`) and opens a fresh one (`ss`). Mirrors Braze web, which
/// rolls a session after ~30 min of inactivity.
const SESSION_GAP_SECS: u64 = 30 * 60;

struct SessionClock {
    started_at: u64,
    last_activity: u64,
}

fn clock() -> &'static std::sync::Mutex<Option<SessionClock>> {
    static C: OnceLock<std::sync::Mutex<Option<SessionClock>>> = OnceLock::new();
    C.get_or_init(|| std::sync::Mutex::new(None))
}

/// Drive the Braze session lifecycle from real user activity. The FIRST activity
/// opens a session (`ss`). Activity after an idle gap closes the stale session
/// (`se` with its real duration) and opens a new one — so every `ss` is eventually
/// paired with an `se`, as a genuine client does. Best-effort; failures swallowed.
pub async fn on_activity(user_id: &str) {
    if user_id.is_empty() {
        return;
    }
    let now = (now_millis() / 1000) as u64;
    let action = {
        let mut g = match clock().lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match g.as_mut() {
            None => {
                *g = Some(SessionClock {
                    started_at: now,
                    last_activity: now,
                });
                Some(None)
            }
            Some(c) if now.saturating_sub(c.last_activity) > SESSION_GAP_SECS => {
                let ended_duration = c.last_activity.saturating_sub(c.started_at);
                c.started_at = now;
                c.last_activity = now;
                Some(Some(ended_duration))
            }
            Some(c) => {
                c.last_activity = now;
                None
            }
        }
    };
    match action {
        Some(Some(prev_duration)) => {
            session_end(user_id, prev_duration).await;
            session_start(user_id).await;
        }
        Some(None) => session_start(user_id).await,
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ss_body_has_expected_shape() {
        let b = data_body("ss", "123", None, "150");
        assert_eq!(b["api_key"], API_KEY);
        assert_eq!(b["sdk_version"], SDK_VERSION);
        assert_eq!(b["sdk_metadata"][0], "npm");
        assert_eq!(b["events"][0]["name"], "ss");
        assert_eq!(b["events"][0]["user_id"], "123");
        assert!(b["events"][0].get("data").is_none());
        assert_eq!(b["device"]["browser_version"], "150.0.0.0");
        assert!(b["device"]["user_agent"]
            .as_str()
            .unwrap()
            .contains("Chrome/150"));
        assert!(b["device_id"].as_str().unwrap().len() == 36);
    }

    #[test]
    fn se_body_carries_duration() {
        let b = data_body("se", "123", Some(42), "150");
        assert_eq!(b["events"][0]["name"], "se");
        assert_eq!(b["events"][0]["data"]["d"], 42);
    }

    #[test]
    fn session_is_stable() {
        let a = session().device_id.clone();
        let b = session().device_id.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn uuid_format() {
        let u = rand_uuid();
        assert_eq!(u.len(), 36);
        assert_eq!(u.as_bytes()[14], b'4');
    }
}

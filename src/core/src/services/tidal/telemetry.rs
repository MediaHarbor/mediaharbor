//! Tidal anti-ban telemetry (Phase F.2).
//!
//! MediaHarbor normally makes only the API calls it needs. The real Tidal client
//! continuously emits `display_module` impression beacons (one per UI module shown)
//! to `tidal.com/api/event-batch` as an AWS SQS `SendMessageBatch`. A fetch-only
//! client that never sends these is a ban fingerprint. When the per-service opt-in
//! (`tidal_telemetry_enabled`, default OFF) is on, we replicate that traffic: after a
//! genuine feed/explore load, we fire a `display_module` batch for the modules that
//! were returned — tying the telemetry to real user activity, as a real client does.
//!
//! Event shape reverse-engineered from a real web-client capture (HAR). We mimic the
//! **web** client (`platform:"web"`), consistent with MediaHarbor's web-API Tidal path.

use std::sync::OnceLock;

use crate::services::common::ids::{now_millis, rand_hex, rand_uuid};
use serde_json::json;
use tokio::sync::OnceCell;

const EVENT_BATCH_URL: &str = "https://ec.tidal.com/api/event-batch";
const FALLBACK_APP_VERSION: &str = "2.200.0";
const CLIENT_BUILD_SUFFIX: &str = "-10004.648";
const CLIENT_TOKEN: &str = "13108";
const CLIENT_ID_HEADER: &str = "lw3vR6GE1vtNBsjv";
const PLAY_STORE_URL: &str =
    "https://play.google.com/store/apps/details?id=com.aspiro.tidal&hl=en&gl=US";

/// The current Tidal **Android** app version, scraped once from the Play Store
/// listing. Reporting a stale version is a ban fingerprint, so we track theirs.
/// Constrained to the `2.<minor>.<patch>` family (the Play page also contains
/// unrelated version-like strings). Falls back to [`FALLBACK_APP_VERSION`].
async fn app_version() -> &'static str {
    static VERSION: OnceCell<String> = OnceCell::const_new();
    VERSION
        .get_or_init(|| async {
            let fetched = async {
                let client = crate::http_client::shared_client().ok()?;
                let html = client
                    .get(PLAY_STORE_URL)
                    .header("User-Agent", "Mozilla/5.0")
                    .send()
                    .await
                    .ok()?
                    .text()
                    .await
                    .ok()?;
                let re = regex::Regex::new(r"2\.(\d{1,3})\.(\d{1,3})").ok()?;
                re.captures_iter(&html)
                    .filter_map(|c| {
                        let minor: u32 = c.get(1)?.as_str().parse().ok()?;
                        let patch: u32 = c.get(2)?.as_str().parse().ok()?;
                        Some((minor, patch, c.get(0)?.as_str().to_string()))
                    })
                    .max_by_key(|(minor, patch, _)| (*minor, *patch))
                    .map(|(_, _, v)| v)
            }
            .await;
            fetched.unwrap_or_else(|| FALLBACK_APP_VERSION.to_string())
        })
        .await
}

/// A stable per-process session identity mirroring the real Android client's
/// numeric `clientId` and `sessionId` (generated once, reused every event this run).
struct Session {
    client_id: u64,
    session_id: String,
    trace_uuid: String,
}

fn session() -> &'static Session {
    static S: OnceLock<Session> = OnceLock::new();
    S.get_or_init(|| Session {
        client_id: 100_000_000 + (rand_u64() % 900_000_000),
        session_id: rand_uuid(),
        trace_uuid: rand_uuid(),
    })
}

fn rand_u64() -> u64 {
    u64::from_str_radix(&rand_hex(16), 16).unwrap_or(597_577_154)
}

/// The device fingerprint reported in telemetry. Every field is user-configurable
/// (Settings → Tidal) so the whole set stays internally consistent — no hidden
/// lookup table that could pair a Pixel model with a Samsung vendor. Callers pass
/// a fully-specified [`Device`]; `app_version` is resolved from the Play Store.
pub struct Device {
    pub model: String,
    pub vendor: String,
    pub device_type: String,
    pub os_version: String,
    pub screen_width: u32,
    pub screen_height: u32,
}

/// Per-batch identity: the configured device + the live Play Store app version.
struct Ctx {
    version: String,
    device: Device,
}

fn android_client(ctx: &Ctx) -> serde_json::Value {
    json!({
        "deviceType": ctx.device.device_type,
        "platform": "android",
        "token": CLIENT_TOKEN,
        "version": format!("{}{CLIENT_BUILD_SUFFIX}", ctx.version),
    })
}

fn android_user(user_id: &str) -> serde_json::Value {
    let s = session();
    let uid: u64 = user_id.parse().unwrap_or(0);
    json!({
        "clientId": s.client_id,
        "id": uid,
        "sessionId": s.session_id,
    })
}

fn android_extras() -> serde_json::Value {
    let s = session();
    json!({ "navigation": { "trace": { "origin": "SEARCH", "chainSize": 1, "uuid": s.trace_uuid } } })
}

/// Build one event's JSON `MessageBody` in the real Android-client shape (captured
/// from `tidal_mitm.flow`). Note: no `name` field in the body — the event name
/// lives only in the `Name` MessageAttribute — and no `accessToken` in `user`.
fn android_event(group: &str, payload: serde_json::Value, user_id: &str, ctx: &Ctx) -> String {
    json!({
        "client": android_client(ctx),
        "extras": android_extras(),
        "payload": payload,
        "ts": now_millis(),
        "user": android_user(user_id),
        "uuid": rand_uuid(),
        "group": group,
        "version": 2,
    })
    .to_string()
}

/// The Android client's `Headers` MessageAttribute — carries the bearer token and
/// device identity alongside every SQS entry.
fn headers_attr(access_token: &str, ctx: &Ctx) -> String {
    json!({
        "app-version": ctx.version,
        "os-version": ctx.device.os_version,
        "os-name": "Android",
        "requested-sent-timestamp": now_millis().to_string(),
        "device-vendor": ctx.device.vendor,
        "device-model": ctx.device.model,
        "consent-category": "NECESSARY",
        "authorization": access_token,
        "client-id": CLIENT_ID_HEADER,
    })
    .to_string()
}

/// Append one SQS `SendMessageBatchRequestEntry.N.*` with both the `Name` and
/// `Headers` MessageAttributes the real web client sends.
fn push_entry(
    form: &mut Vec<(String, String)>,
    n: usize,
    name: &str,
    body: String,
    access_token: &str,
    ctx: &Ctx,
) {
    form.push((format!("SendMessageBatchRequestEntry.{n}.Id"), rand_uuid()));
    form.push((
        format!("SendMessageBatchRequestEntry.{n}.MessageBody"),
        body,
    ));
    form.push((
        format!("SendMessageBatchRequestEntry.{n}.MessageAttribute.1.Name"),
        "Name".into(),
    ));
    form.push((
        format!("SendMessageBatchRequestEntry.{n}.MessageAttribute.1.Value.StringValue"),
        name.into(),
    ));
    form.push((
        format!("SendMessageBatchRequestEntry.{n}.MessageAttribute.1.Value.DataType"),
        "String".into(),
    ));
    form.push((
        format!("SendMessageBatchRequestEntry.{n}.MessageAttribute.2.Name"),
        "Headers".into(),
    ));
    form.push((
        format!("SendMessageBatchRequestEntry.{n}.MessageAttribute.2.Value.DataType"),
        "String".into(),
    ));
    form.push((
        format!("SendMessageBatchRequestEntry.{n}.MessageAttribute.2.Value.StringValue"),
        headers_attr(access_token, ctx),
    ));
}

async fn post_batch(access_token: &str, form: Vec<(String, String)>, ctx: &Ctx) {
    let client = match crate::http_client::shared_client() {
        Ok(c) => c,
        Err(_) => return,
    };
    let _ = client
        .post(EVENT_BATCH_URL)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header(
            "User-Agent",
            format!("TIDAL_ANDROID/{} okhttp/5.3.2", ctx.version),
        )
        .form(&form)
        .send()
        .await;
}

/// Fire a `display_module` impression batch for the given (moduleId, pageId) pairs.
/// Best-effort: failures are swallowed (telemetry must never break a real request).
pub async fn display_modules(
    access_token: &str,
    user_id: &str,
    device: Device,
    modules: &[(String, String)],
) {
    if modules.is_empty() || access_token.is_empty() {
        return;
    }
    let ctx = Ctx {
        version: app_version().await.to_string(),
        device,
    };
    let mut form: Vec<(String, String)> = Vec::new();
    for (i, (module_id, page_id)) in modules.iter().enumerate().take(10) {
        let body = android_event(
            "analytics",
            json!({ "moduleId": module_id, "pageId": page_id, "placement": i }),
            user_id,
            &ctx,
        );
        push_entry(&mut form, i + 1, "display_module", body, access_token, &ctx);
    }
    post_batch(access_token, form, &ctx).await;
}

/// What a real client reports for each tier: the quality name, the codec, and the
/// bandwidth the DASH manifest advertises. Device-probed — the lossy tiers are 97k
/// and 322k, not the round numbers their names imply. Claiming hi-res for a session
/// that actually streamed AAC is precisely the inconsistency this mimicry exists to
/// avoid, and it is what a hardcoded `HI_RES_LOSSLESS` used to do for every user.
fn tier_report(quality: u8) -> (&'static str, &'static str, &'static str, u32) {
    match quality {
        0 => ("LOW", "mp4a.40.5", "audio/mp4", 97_000),
        1 => ("HIGH", "mp4a.40.2", "audio/mp4", 322_000),
        2 => ("LOSSLESS", "flac", "audio/flac", 763_000),
        _ => ("HI_RES_LOSSLESS", "flac", "audio/flac", 1_461_000),
    }
}

/// Report a completed play to Tidal's listening history. Replays the exact
/// Android-client lifecycle captured from `tidal_mitm.flow` (verified: the
/// captured "Another Star" play landed in the account's Recently Played):
/// `streaming_session_start` → `playback_statistics` (the history record,
/// `endReason:COMPLETE`) → `streaming_session_end`, plus `play_log/playback_session`,
/// all sharing one `streamingSessionId`. Best-effort; failures swallowed.
pub async fn report_playback(
    access_token: &str,
    user_id: &str,
    device: Device,
    product_id: &str,
    duration_secs: u64,
    quality: u8,
) {
    if access_token.is_empty() || product_id.is_empty() {
        return;
    }
    let ctx = Ctx {
        version: app_version().await.to_string(),
        device,
    };
    let ssid = rand_uuid();
    let end = now_millis();
    let start = end.saturating_sub(u128::from(duration_secs) * 1000);
    let end_pos = duration_secs as f64;
    let (actual_quality, codecs, mime_type, bandwidth) = tier_report(quality);

    let events = [
        (
            "streaming_metrics",
            "streaming_session_start",
            json!({
                "hardwarePlatform": ctx.device.model,
                "isOfflineModeStart": false,
                "mobileNetworkType": "",
                "networkType": "WIFI",
                "operatingSystem": "Android",
                "operatingSystemVersion": ctx.device.os_version,
                "screenHeight": ctx.device.screen_height, "screenWidth": ctx.device.screen_width,
                "sessionProductId": product_id, "sessionProductType": "TRACK",
                "sessionType": "PLAYBACK", "startReason": "EXPLICIT",
                "streamingSessionId": ssid, "timestamp": start,
            }),
        ),
        (
            "streaming_metrics",
            "playback_statistics",
            json!({
                "actualAssetPresentation": "FULL", "actualAudioMode": "STEREO",
                "actualProductId": product_id, "actualQuality": actual_quality,
                "actualStartTimestamp": start,
                "adaptations": [{
                    "assetPosition": 0.0, "bandwidth": bandwidth, "codecs": codecs,
                    "mimeType": mime_type, "timestamp": start,
                    "videoHeight": -1, "videoWidth": -1,
                }],
                "cdm": "NONE", "endReason": "COMPLETE", "endTimestamp": end,
                "hasAds": false, "idealStartTimestamp": start,
                "mediaStorage": "INTERNET", "productType": "TRACK",
                "stalls": [], "streamingSessionId": ssid, "tags": ["ADAPTIVE_PLAYBACK"],
            }),
        ),
        (
            "play_log",
            "playback_session",
            json!({
                "actions": [{ "actionType": "PLAYBACK_STOP", "assetPosition": end_pos, "timestamp": end }],
                "actualAssetPresentation": "FULL", "actualAudioMode": "STEREO",
                "actualProductId": product_id, "actualQuality": actual_quality,
                "endAssetPosition": end_pos, "endTimestamp": end, "isPostPaywall": true,
                "playbackSessionId": ssid, "productType": "TRACK",
                "requestedProductId": product_id, "sourceId": product_id, "sourceType": "ITEM",
                "startAssetPosition": 0.0, "startTimestamp": start,
            }),
        ),
        (
            "streaming_metrics",
            "streaming_session_end",
            json!({ "streamingSessionId": ssid, "timestamp": end }),
        ),
    ];

    let mut form: Vec<(String, String)> = Vec::new();
    for (i, (group, name, payload)) in events.iter().enumerate() {
        let body = android_event(group, payload.clone(), user_id, &ctx);
        push_entry(&mut form, i + 1, name, body, access_token, &ctx);
    }
    post_batch(access_token, form, &ctx).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> Ctx {
        Ctx {
            version: "2.200.0".to_string(),
            device: Device {
                model: "SM-A166B".to_string(),
                vendor: "samsung".to_string(),
                device_type: "phone".to_string(),
                os_version: "14".to_string(),
                screen_width: 1080,
                screen_height: 2340,
            },
        }
    }

    #[test]
    fn device_fields_flow_through() {
        let h = headers_attr("tok", &test_ctx());
        let v: serde_json::Value = serde_json::from_str(&h).unwrap();
        assert_eq!(v["device-model"], "SM-A166B");
        assert_eq!(v["device-vendor"], "samsung");
        assert_eq!(v["os-version"], "14");
    }

    #[test]
    fn android_event_has_expected_shape() {
        let b = android_event(
            "analytics",
            json!({"moduleId":"m","pageId":"p","placement":0}),
            "208701978",
            &test_ctx(),
        );
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert_eq!(v["group"], "analytics");
        assert!(v.get("name").is_none());
        assert_eq!(v["client"]["platform"], "android");
        assert_eq!(v["client"]["token"], "13108");
        assert_eq!(v["client"]["version"], "2.200.0-10004.648");
        assert!(v["user"].get("accessToken").is_none());
        assert!(v["user"]["clientId"].is_u64());
        assert_eq!(v["user"]["id"], 208701978u64);
        assert!(v["user"]["sessionId"].is_string());
        assert!(v["extras"]["navigation"]["trace"].is_object());
        assert!(v["ts"].as_u64().is_some());
        assert!(v["uuid"].as_str().unwrap().len() >= 32);
    }

    #[test]
    fn session_is_stable() {
        let a = session().session_id.clone();
        let b = session().session_id.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn uuid_format() {
        let u = rand_uuid();
        assert_eq!(u.len(), 36);
        assert_eq!(u.as_bytes()[14], b'4');
    }

    #[test]
    fn streaming_event_has_history_shape() {
        let b = android_event(
            "streaming_metrics",
            json!({ "actualProductId": "510132015", "endReason": "COMPLETE", "cdm": "NONE" }),
            "208701978",
            &test_ctx(),
        );
        let v: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert_eq!(v["group"], "streaming_metrics");
        assert_eq!(v["client"]["platform"], "android");
        assert_eq!(v["user"]["id"], 208701978u64);
        assert_eq!(v["payload"]["actualProductId"], "510132015");
        assert_eq!(v["payload"]["endReason"], "COMPLETE");
        assert_eq!(v["payload"]["cdm"], "NONE");
    }

    #[test]
    fn headers_attr_carries_bearer() {
        let h = headers_attr("mytoken", &test_ctx());
        let v: serde_json::Value = serde_json::from_str(&h).unwrap();
        assert_eq!(v["authorization"], "mytoken");
        assert_eq!(v["os-name"], "Android");
        assert_eq!(v["client-id"], "lw3vR6GE1vtNBsjv");
        assert_eq!(v["device-model"], "SM-A166B");
        assert_eq!(v["device-vendor"], "samsung");
        assert_eq!(v["app-version"], "2.200.0");
    }
}

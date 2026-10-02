//! Qobuz playback-history + anti-ban telemetry (Phase F.7).
//!
//! MediaHarbor normally makes only the API calls it needs. The real Qobuz client
//! reports every play through a three-call chain that also writes listen history:
//! `track/reportStreamingStart` (form), `track/reportStreamingEndJson` (json, the
//! history record), and `event/reportTrackContext` (signed, where it was played
//! from). A fetch-only client that never sends these is a ban fingerprint and never
//! appears in the account's Qobuz listen history. When the per-service opt-in
//! (`qobuz_sync_playback_history`, default OFF) is on, we replicate the full chain.
//!
//! The history write depends on a server-issued `blob` play-proof token that cannot
//! be fabricated; it is returned by `track/getFileUrl`. So this module resolves the
//! stream (getFileUrl) to mint a fresh blob and read the actually-served `format_id`,
//! then reports Start/End with that blob and the real format, exactly as the app does.
//!
//! Shapes reverse-engineered from a real Android-client capture. MediaHarbor uses the
//! web `app_id`, but the endpoint shapes are identical; the blob dependency was
//! confirmed live on the web path.

use crate::services::common::ids::{now_secs, rand_uuid};
use reqwest::Client;
use serde_json::{json, Value};

const BASE: &str = "https://www.qobuz.com/api.json/0.2";
const SOFTWARE_VERSION: &str = "9.11.1.3";

pub struct QobuzContext {
    pub content_group_type: String,
    pub content_group_id: String,
    pub played_from_name: String,
    pub played_from_category: String,
    pub played_from_level: String,
}

impl Default for QobuzContext {
    fn default() -> Self {
        Self {
            content_group_type: "Library".into(),
            content_group_id: String::new(),
            played_from_name: "None".into(),
            played_from_category: "Library".into(),
            played_from_level: "Music Page".into(),
        }
    }
}

fn iso8601(secs: u64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(secs as i64, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%:z").to_string())
        .unwrap_or_default()
}

fn md5_hex(s: &str) -> String {
    use md5::{Digest, Md5};
    hex::encode(Md5::digest(s.as_bytes()))
}

/// The `blob` (server-issued play-proof) + the actually-served `format_id` for a
/// track, from `track/getFileUrl`. Both are required for a valid history write.
struct StreamProof {
    blob: String,
    format_id: u32,
}

async fn resolve_proof(
    client: &Client,
    app_id: &str,
    token: &str,
    secret: &str,
    track_id: &str,
    requested_format_id: u32,
) -> Option<StreamProof> {
    let ts = now_secs();
    let sig = crate::services::qobuz::app_credentials::file_url_signature(
        track_id,
        &requested_format_id.to_string(),
        &ts.to_string(),
        secret,
    );
    let resp = client
        .get(format!("{BASE}/track/getFileUrl"))
        .header("X-App-Id", app_id)
        .header("X-User-Auth-Token", token)
        .header("accept", "application/json")
        .query(&[
            ("request_ts", ts.to_string().as_str()),
            ("request_sig", sig.as_str()),
            ("track_id", track_id),
            ("format_id", requested_format_id.to_string().as_str()),
            ("intent", "stream"),
            ("app_id", app_id),
        ])
        .send()
        .await
        .ok()?;
    let v: Value = resp.json().await.ok()?;
    let blob = v.get("blob")?.as_str()?.to_string();
    let format_id = v
        .get("format_id")
        .and_then(|x| x.as_u64())
        .unwrap_or(requested_format_id as u64) as u32;
    Some(StreamProof { blob, format_id })
}

/// Fire the full Qobuz play → history chain. Best-effort: any network error is
/// swallowed so telemetry can never break playback (consistent with the Tidal and
/// Deezer telemetry paths). The history write depends on the server-issued `blob`,
/// which we mint by resolving the stream first.
pub async fn report_playback(
    client: Client,
    app_id: String,
    token: String,
    secret: String,
    track_id: String,
    requested_format_id: u32,
    duration_secs: u64,
    context: QobuzContext,
) {
    let proof = match resolve_proof(
        &client,
        &app_id,
        &token,
        &secret,
        &track_id,
        requested_format_id,
    )
    .await
    {
        Some(p) => p,
        None => return,
    };

    let started = now_secs();
    let track_context_uuid = rand_uuid();

    let start_events = json!([{
        "track_id": track_id,
        "date": started,
        "format_id": proof.format_id,
    }])
    .to_string();
    let start_body = format!(
        "events={}",
        url::form_urlencoded::byte_serialize(start_events.as_bytes()).collect::<String>()
    );
    let _ = client
        .post(format!("{BASE}/track/reportStreamingStart?app_id={app_id}"))
        .header("X-App-Id", &app_id)
        .header("X-User-Auth-Token", &token)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(start_body)
        .send()
        .await;

    let end_body = json!({
        "renderer_context": { "software_version": SOFTWARE_VERSION },
        "events": [{
            "duration": duration_secs,
            "start_stream": iso8601(started),
            "blob": proof.blob,
            "track_context_uuid": track_context_uuid,
            "online": 1,
            "local": 0,
        }],
    });
    let _ = client
        .post(format!(
            "{BASE}/track/reportStreamingEndJson?app_id={app_id}"
        ))
        .header("X-App-Id", &app_id)
        .header("X-User-Auth-Token", &token)
        .header("Content-Type", "application/json")
        .json(&end_body)
        .send()
        .await;

    let cts = now_secs();
    let csig = md5_hex(&format!("eventreportTrackContext{cts}{secret}"));
    let ctx_body = json!({
        "version": "01.00",
        "events": [{
            "track_context_uuid": track_context_uuid,
            "data": {
                "contentGroupType": context.content_group_type,
                "contentGroupId": context.content_group_id,
                "playedFromName": context.played_from_name,
                "playedFromCategory": context.played_from_category,
                "playedFromLevel": context.played_from_level,
            },
        }],
    });
    let _ = client
        .post(format!("{BASE}/event/reportTrackContext"))
        .header("X-App-Id", &app_id)
        .header("X-User-Auth-Token", &token)
        .header("Content-Type", "application/json")
        .query(&[
            ("request_ts", cts.to_string().as_str()),
            ("request_sig", csig.as_str()),
            ("app_id", app_id.as_str()),
        ])
        .json(&ctx_body)
        .send()
        .await;
}

/// Signed POST with the same `<object><method><ts><secret>` md5 the play report
/// uses.
async fn signed_post(
    client: &Client,
    app_id: &str,
    token: &str,
    secret: &str,
    object: &str,
    method: &str,
    form: Vec<(&'static str, String)>,
) {
    let ts = now_secs();
    let sig = md5_hex(&format!("{object}{method}{ts}{secret}"));
    let req = client
        .post(format!("{BASE}/{object}/{method}"))
        .header("X-App-Id", app_id)
        .header("X-User-Auth-Token", token)
        .query(&[
            ("request_ts", ts.to_string().as_str()),
            ("request_sig", sig.as_str()),
            ("app_id", app_id),
        ]);
    let _ = req.form(&form).send().await;
}

/// `POST session/start` — the handshake the app makes once when it comes up.
pub async fn report_session_start(client: Client, app_id: String, token: String, secret: String) {
    signed_post(
        &client,
        &app_id,
        &token,
        &secret,
        "session",
        "start",
        vec![("profile", "qbz-1".to_string())],
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_is_v4_shaped() {
        let u = rand_uuid();
        assert_eq!(u.len(), 36);
        assert_eq!(u.as_bytes()[14], b'4');
        assert!(matches!(u.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
    }

    #[test]
    fn iso8601_has_offset() {
        let s = iso8601(1_784_565_686);
        assert!(s.contains('T'));
        assert!(s.contains('+') || s.contains('-'));
    }

    #[test]
    fn context_default_is_library() {
        let c = QobuzContext::default();
        assert_eq!(c.content_group_type, "Library");
        assert_eq!(c.played_from_level, "Music Page");
    }

    #[test]
    fn sig_is_stable_md5() {
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
    }
}

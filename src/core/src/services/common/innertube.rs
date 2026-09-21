use std::path::{Path, PathBuf};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};

use crate::errors::{MhError, MhResult};
use crate::services::common::ids::now_secs;
use crate::services::common::library::{collect_set_cookies, read_cookies};

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0 Safari/537.36";

/// The parts of an InnerTube identity that differ between YouTube and YT Music.
/// Everything else about the transport — SAPISIDHASH signing, the header set,
/// the context envelope, cookie rotation — is identical.
#[derive(Clone, Copy)]
pub struct InnertubeCfg {
    pub client_name: &'static str,
    pub client_version: &'static str,
    pub client_name_id: &'static str,
    pub origin: &'static str,
    pub referer: &'static str,
    /// Used to prefix errors, e.g. "YT Music".
    pub label: &'static str,
}

pub const YOUTUBE: InnertubeCfg = InnertubeCfg {
    client_name: "WEB",
    client_version: "2.20250507.01.00",
    client_name_id: "1",
    origin: "https://www.youtube.com",
    referer: "https://www.youtube.com/",
    label: "YouTube",
};

pub const YTMUSIC: InnertubeCfg = InnertubeCfg {
    client_name: "WEB_REMIX",
    client_version: "1.20260526.04.00",
    client_name_id: "67",
    origin: "https://music.youtube.com",
    referer: "https://music.youtube.com/",
    label: "YT Music",
};

pub struct Innertube {
    client: Client,
    sapisid: String,
    cookie_header: String,
    cookies_path: PathBuf,
    cfg: InnertubeCfg,
}

impl Innertube {
    /// `missing_sapisid` is the message shown when the cookie jar has no
    /// SAPISID, which is what "not signed in" looks like on this API.
    pub fn from_cookies(
        cookies_path: &str,
        cfg: InnertubeCfg,
        missing_sapisid: &str,
    ) -> MhResult<Self> {
        let jar = read_cookies(cookies_path)?;
        let sapisid = jar
            .get("SAPISID")
            .or_else(|| jar.get("__Secure-3PAPISID"))
            .ok_or_else(|| MhError::Auth(missing_sapisid.to_string()))?
            .clone();
        let cookie_header = jar
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        let client = crate::http_client::build_ua_client(UA)?;
        Ok(Self {
            client,
            sapisid,
            cookie_header,
            cookies_path: PathBuf::from(cookies_path),
            cfg,
        })
    }

    pub fn cookies_path(&self) -> &Path {
        &self.cookies_path
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn cookie_header(&self) -> &str {
        &self.cookie_header
    }

    /// A freshly signed `SAPISIDHASH` value, for callers that build their own
    /// request rather than going through [`Innertube::post`].
    pub fn authorization(&self) -> String {
        self.sapisidhash()
    }

    pub fn context(&self) -> Value {
        json!({
            "client": {
                "clientName": self.cfg.client_name,
                "clientVersion": self.cfg.client_version,
                "hl": "en", "gl": "US",
                "platform": "DESKTOP",
                "userAgent": UA,
            },
            "user": { "lockedSafetyMode": false },
            "request": { "useSsl": true },
        })
    }

    fn sapisidhash(&self) -> String {
        let ts = now_secs();
        let input = format!("{ts} {} {}", self.sapisid, self.cfg.origin);
        format!(
            "SAPISIDHASH {ts}_{}",
            hex::encode(Sha1::digest(input.as_bytes()))
        )
    }

    pub fn headers(&self) -> MhResult<HeaderMap> {
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_str(&self.sapisidhash())?);
        h.insert("cookie", HeaderValue::from_str(&self.cookie_header)?);
        h.insert("origin", HeaderValue::from_static(self.cfg.origin));
        h.insert("referer", HeaderValue::from_static(self.cfg.referer));
        h.insert("x-origin", HeaderValue::from_static(self.cfg.origin));
        h.insert("content-type", HeaderValue::from_static("application/json"));
        h.insert("x-goog-authuser", HeaderValue::from_static("0"));
        h.insert(
            HeaderName::from_static("x-youtube-client-name"),
            HeaderValue::from_static(self.cfg.client_name_id),
        );
        h.insert(
            HeaderName::from_static("x-youtube-client-version"),
            HeaderValue::from_static(self.cfg.client_version),
        );
        Ok(h)
    }

    /// Google rotates session cookies on the response; writing them back keeps
    /// the jar usable for the next run instead of silently ageing out.
    fn persist_rotation(&self, set_cookies: Vec<String>) {
        if set_cookies.is_empty() {
            return;
        }
        let path = self.cookies_path.clone();
        tokio::spawn(async move {
            let _ = crate::auth::netscape::apply_set_cookies(&path, &set_cookies).await;
        });
    }

    /// POST without status interpretation — for callers that treat a 401 as data
    /// rather than an error (session probes).
    pub async fn post_status(&self, url: &str, body: Value) -> MhResult<(StatusCode, Value)> {
        let resp = self
            .client
            .post(url)
            .headers(self.headers()?)
            .json(&body)
            .send()
            .await
            .map_err(MhError::Network)?;
        let status = resp.status();
        let set_cookies = collect_set_cookies(resp.headers());
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        self.persist_rotation(set_cookies);
        Ok((status, v))
    }

    /// POST expecting success. `what` names the operation in errors, e.g.
    /// "browse FEmusic_home".
    pub async fn post(&self, url: &str, body: Value, what: &str) -> MhResult<Value> {
        let (status, v) = self.post_status(url, body).await?;
        if matches!(status.as_u16(), 401 | 403) {
            return Err(MhError::Auth(format!(
                "{} {what} unauthorized (HTTP {}). Cookies may be expired.",
                self.cfg.label,
                status.as_u16()
            )));
        }
        if !status.is_success() {
            return Err(crate::services::common::http::api_error(
                &format!("{} {what}", self.cfg.label),
                status,
                &v.to_string(),
            ));
        }
        Ok(v)
    }

    /// `browseId` + optional `params`, the shape almost every InnerTube read uses.
    pub async fn browse(
        &self,
        url: &str,
        browse_id: &str,
        params: Option<&str>,
    ) -> MhResult<Value> {
        let mut body = json!({ "browseId": browse_id, "context": self.context() });
        if let Some(p) = params {
            body["params"] = Value::String(p.to_string());
        }
        self.post(url, body, &format!("browse {browse_id}")).await
    }

    pub async fn continuation(&self, url: &str, token: &str) -> MhResult<Value> {
        let body = json!({ "continuation": token, "context": self.context() });
        self.post(url, body, "continuation").await
    }
}

/// `{ "runs": [...] }` / `{ "simpleText": ... }` — InnerTube's two text shapes.
/// `join_runs` controls whether every run is concatenated (YT Music titles) or
/// only the first is taken (YouTube bylines).
pub fn text_of(v: &Value, join_runs: bool) -> Option<String> {
    if let Some(t) = v.get("simpleText").and_then(|x| x.as_str()) {
        return (!t.is_empty()).then(|| t.to_string());
    }
    let runs = v.get("runs")?.as_array()?;
    let joined: String = if join_runs {
        runs.iter()
            .filter_map(|r| r.get("text").and_then(|x| x.as_str()))
            .collect()
    } else {
        runs.first()
            .and_then(|r| r.get("text").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_string()
    };
    (!joined.is_empty()).then_some(joined)
}

/// Depth-first collect of every `{ <key>: {...} }` node for the named renderer
/// keys, in document order.
pub fn collect_renderers(root: &Value, keys: &[&str]) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    walk(root, keys, &mut out);
    out
}

fn walk(v: &Value, keys: &[&str], out: &mut Vec<(String, Value)>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if keys.contains(&k.as_str()) {
                    out.push((k.clone(), val.clone()));
                }
                walk(val, keys, out);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                walk(item, keys, out);
            }
        }
        _ => {}
    }
}

/// First continuation token anywhere in an InnerTube response — the field sits
/// at a different depth per surface, so the whole tree is walked.
pub fn continuation_token(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(t) = map
                .get("nextContinuationData")
                .and_then(|n| n.get("continuation"))
                .and_then(|c| c.as_str())
            {
                return Some(t.to_string());
            }
            if let Some(t) = map
                .get("continuationCommand")
                .and_then(|n| n.get("token"))
                .and_then(|c| c.as_str())
            {
                return Some(t.to_string());
            }
            map.values().find_map(continuation_token)
        }
        Value::Array(arr) => arr.iter().find_map(continuation_token),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_client_identities_stay_distinct() {
        assert_ne!(YOUTUBE.client_name, YTMUSIC.client_name);
        assert_ne!(YOUTUBE.client_name_id, YTMUSIC.client_name_id);
        assert_ne!(YOUTUBE.origin, YTMUSIC.origin);
    }

    #[test]
    fn simple_text_and_runs_both_read_as_text() {
        assert_eq!(
            text_of(&json!({ "simpleText": "Hello" }), true).as_deref(),
            Some("Hello")
        );
        let runs = json!({ "runs": [{ "text": "A" }, { "text": "B" }] });
        assert_eq!(text_of(&runs, true).as_deref(), Some("AB"), "joined");
        assert_eq!(
            text_of(&runs, false).as_deref(),
            Some("A"),
            "first run only"
        );
    }

    #[test]
    fn empty_text_is_absent_not_an_empty_string() {
        assert_eq!(text_of(&json!({ "simpleText": "" }), true), None);
        assert_eq!(text_of(&json!({ "runs": [] }), true), None);
        assert_eq!(text_of(&json!({ "other": 1 }), true), None);
    }

    #[test]
    fn renderers_are_collected_at_any_depth_in_document_order() {
        let tree = json!({
            "a": { "musicResponsiveListItemRenderer": { "n": 1 } },
            "b": [{ "deep": { "gridRenderer": { "n": 2 } } }],
            "c": { "ignored": { "n": 3 } }
        });
        let found = collect_renderers(&tree, &["musicResponsiveListItemRenderer", "gridRenderer"]);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, "musicResponsiveListItemRenderer");
        assert_eq!(found[1].0, "gridRenderer");
    }
}

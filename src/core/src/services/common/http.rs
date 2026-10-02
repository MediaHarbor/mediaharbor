use reqwest::{Response, StatusCode};
use serde::de::DeserializeOwned;

use crate::errors::{MhError, MhResult};

pub fn api_error(service: &str, status: StatusCode, body: &str) -> MhError {
    let code = status.as_u16();
    let body = body.trim();
    match code {
        401 | 403 => MhError::Auth(format!(
            "{service} session expired or rejected (HTTP {code}): {body}"
        )),
        429 => MhError::RateLimited(format!("{service} rate limited (HTTP {code}): {body}")),
        _ => MhError::Other(format!("{service} API error (HTTP {code}): {body}")),
    }
}

/// Consume a failed auth response into `MhError::Auth`, keeping the whole body.
/// Kept separate from `api_error` because callers on auth endpoints want the
/// `Auth` variant regardless of status — several call sites branch on it to
/// decide whether a credential needs refreshing.
pub async fn auth_error(service: &str, resp: Response) -> MhError {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    MhError::Auth(format!(
        "{service} authentication failed (HTTP {}): {}",
        status.as_u16(),
        body.trim()
    ))
}

pub async fn read_body(service: &str, resp: Response) -> MhResult<String> {
    let status = resp.status();
    let body = resp.text().await.map_err(MhError::Network)?;
    if status.is_success() {
        Ok(body)
    } else {
        Err(api_error(service, status, &body))
    }
}

pub async fn read_json<T: DeserializeOwned>(service: &str, resp: Response) -> MhResult<T> {
    let body = read_body(service, resp).await?;
    serde_json::from_str(&body).map_err(|e| {
        MhError::Parse(format!(
            "{service}: unexpected response: {e} — body: {body}"
        ))
    })
}

/// Percent-encode everything outside RFC 3986's unreserved set.
///
/// Deliberately not `url::form_urlencoded::byte_serialize`, which is also used in this
/// crate: that encodes a space as `+`, which these callers must not send.
pub fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_whole_upstream_body() {
        let body = "x".repeat(5000);
        let e = api_error("Qobuz", StatusCode::BAD_REQUEST, &body);
        assert!(e.to_string().contains(&body), "body must not be truncated");
    }

    #[test]
    fn maps_auth_and_rate_limit_codes() {
        assert!(matches!(
            api_error("S", StatusCode::UNAUTHORIZED, "no"),
            MhError::Auth(_)
        ));
        assert!(matches!(
            api_error("S", StatusCode::FORBIDDEN, "no"),
            MhError::Auth(_)
        ));
        assert!(matches!(
            api_error("S", StatusCode::TOO_MANY_REQUESTS, "slow"),
            MhError::RateLimited(_)
        ));
        assert!(matches!(
            api_error("S", StatusCode::INTERNAL_SERVER_ERROR, "boom"),
            MhError::Other(_)
        ));
    }
}

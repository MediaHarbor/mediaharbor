//! Client for a `glomatico/wrapper-v2` daemon.
//!
//! The daemon runs Apple Music's Android libraries in a container and is the only
//! way to obtain FairPlay-protected renditions such as ALAC. MediaHarbor never
//! starts or bundles it — the user runs it themselves and points MediaHarbor at it.

use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};

const DECRYPT_MAGIC: &[u8; 4] = b"WV2D";
const DECRYPT_VERSION: u16 = 1;
const DECRYPT_KIND_BATCH: u16 = 1;
const DECRYPT_KIND_OK: u16 = 2;
const DECRYPT_KIND_ERROR: u16 = 3;
const DECRYPT_KIND_CLOSE: u16 = 9;

/// Guards against a desynchronised stream making us allocate wildly.
const MAX_PAYLOAD_BYTES: u32 = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct WrapperConfig {
    pub base_url: String,
    pub decrypt_host: String,
    pub decrypt_port: u16,
}

impl WrapperConfig {
    /// `None` when the user has not enabled the wrapper.
    pub fn from_settings(settings: &Settings) -> Option<Self> {
        if !settings.apple_use_wrapper {
            return None;
        }
        let base = settings.apple_wrapper_url.trim();
        let base = if base.is_empty() {
            "http://127.0.0.1".to_string()
        } else {
            base.trim_end_matches('/').to_string()
        };
        let host = settings.apple_wrapper_decrypt_host.trim();
        let host = if host.is_empty() {
            "127.0.0.1".to_string()
        } else {
            host.to_string()
        };
        let port = settings
            .apple_wrapper_decrypt_port
            .trim()
            .parse::<u16>()
            .unwrap_or(10020);
        Some(Self {
            base_url: base,
            decrypt_host: host,
            decrypt_port: port,
        })
    }
}

/// What the daemon made of a sign-in attempt. The status codes are the daemon's
/// own `http_status_for(LoginState)` mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginOutcome {
    Authenticated,
    Needs2fa,
    Failed(String),
}

impl LoginOutcome {
    pub fn from_response(status: u16, body: &Value) -> Self {
        let detail = body["detail"]
            .as_str()
            .or_else(|| body["error"].as_str())
            .or_else(|| body["last_error"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        match status {
            200 => LoginOutcome::Authenticated,
            202 => LoginOutcome::Needs2fa,
            504 => LoginOutcome::Failed(
                "wrapper-v2 sign-in timed out while Apple was still deciding".into(),
            ),
            _ => LoginOutcome::Failed(if detail.is_empty() {
                format!("wrapper-v2 sign-in failed (HTTP {status})")
            } else {
                format!("wrapper-v2 sign-in failed (HTTP {status}): {detail}")
            }),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WrapperStatus {
    pub version: String,
    pub runtime: String,
    /// The daemon's own `LoginState` string: `logged_out`, `in_progress`,
    /// `awaiting_2fa`, `authenticated` or `failed`.
    pub state: String,
    pub authenticated: bool,
    pub playback_ready: bool,
    pub apple_id: Option<String>,
}

/// `/me` reports a `LoginState` string, not a boolean — reading it as a flag is
/// how a logged-out daemon can look signed in.
pub fn parse_status(health: &Value, me: &Value) -> WrapperStatus {
    let auth = &me["auth"];
    let state = auth["state"].as_str().unwrap_or_default().to_string();
    let runtime = &me["runtime"];
    WrapperStatus {
        version: health["version"]
            .as_str()
            .or_else(|| me["version"].as_str())
            .unwrap_or_default()
            .to_string(),
        runtime: health["mode"].as_str().unwrap_or_default().to_string(),
        authenticated: state == "authenticated",
        playback_ready: runtime["playback_ready"].as_bool().unwrap_or(false),
        state,
        apple_id: auth["apple_id"]
            .as_str()
            .or_else(|| auth["username"].as_str())
            .map(str::to_string),
    }
}

pub struct WrapperClient {
    config: WrapperConfig,
    http: reqwest::Client,
}

impl WrapperClient {
    pub fn new(config: WrapperConfig) -> MhResult<Self> {
        Ok(Self {
            config,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .map_err(MhError::Network)?,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.config.base_url, path)
    }

    async fn get_json(&self, path: &str) -> MhResult<Value> {
        let resp = self.http.get(self.url(path)).send().await.map_err(|e| {
            MhError::Other(format!(
                "wrapper-v2 is not reachable at {} ({e}). Start the daemon, or turn the \
                 wrapper off in Settings → Apple Music.",
                self.config.base_url
            ))
        })?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(MhError::Other(format!(
                "wrapper-v2 {path} returned HTTP {}: {}",
                status.as_u16(),
                body.trim()
            )));
        }
        serde_json::from_str(&body)
            .map_err(|e| MhError::Parse(format!("wrapper-v2 {path} returned invalid JSON: {e}")))
    }

    /// Liveness plus sign-in state, used to fail fast before a download starts.
    pub async fn status(&self) -> MhResult<WrapperStatus> {
        let health = self.get_json("/health").await?;
        let me = self.get_json("/me").await.unwrap_or(Value::Null);
        Ok(parse_status(&health, &me))
    }

    async fn post_json(&self, path: &str, body: Value) -> MhResult<(u16, Value)> {
        let resp = self
            .http
            .post(self.url(path))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                MhError::Other(format!(
                    "wrapper-v2 is not reachable at {} ({e})",
                    self.config.base_url
                ))
            })?;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((status, json))
    }

    /// Signs the daemon in. Apple's own flow runs inside the wrapper, so the
    /// credential is an Apple ID plus an **app-specific password** — a real
    /// account password will usually be bounced into the 2FA branch.
    pub async fn login(&self, email: &str, password: &str) -> MhResult<LoginOutcome> {
        if email.trim().is_empty() || password.is_empty() {
            return Err(MhError::Auth(
                "wrapper-v2 sign-in needs both an Apple ID and an app-specific password".into(),
            ));
        }
        let (status, body) = self
            .post_json(
                "/login",
                serde_json::json!({ "username": email.trim(), "password": password }),
            )
            .await?;
        Ok(LoginOutcome::from_response(status, &body))
    }

    /// Continues a sign-in that came back awaiting a verification code.
    pub async fn submit_2fa(&self, code: &str) -> MhResult<LoginOutcome> {
        let (status, body) = self
            .post_json("/login/2fa", serde_json::json!({ "code": code.trim() }))
            .await?;
        Ok(LoginOutcome::from_response(status, &body))
    }

    /// Drops the daemon's session. The daemon holds exactly one, so this is the
    /// only way to sign a different Apple ID in.
    pub async fn logout(&self) -> MhResult<()> {
        let resp = self
            .http
            .delete(self.url("/login"))
            .send()
            .await
            .map_err(|e| {
                MhError::Other(format!(
                    "wrapper-v2 is not reachable at {} ({e})",
                    self.config.base_url
                ))
            })?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        Err(MhError::Other(format!(
            "wrapper-v2 DELETE /login returned HTTP {status}: {body}"
        )))
    }

    /// The full MZ playback dispatch for a store id: every variant, key URI and
    /// asset location Apple offers this account.
    pub async fn playback(&self, adam_id: &str) -> MhResult<Value> {
        if !adam_id.chars().all(|c| c.is_ascii_digit()) {
            return Err(MhError::Other(format!(
                "wrapper-v2 needs a numeric Apple store id, got {adam_id}"
            )));
        }
        self.get_json(&format!("/playback?adam_id={adam_id}")).await
    }

    /// Opens a decrypt session. gamdl keeps one connection for a whole track and
    /// numbers its batches, so the worker sees a single continuous conversation
    /// rather than a reconnect per batch.
    pub async fn decrypt_session(&self) -> MhResult<DecryptSession> {
        let addr = format!("{}:{}", self.config.decrypt_host, self.config.decrypt_port);
        let stream = TcpStream::connect(&addr).await.map_err(|e| {
            MhError::Other(format!(
                "wrapper-v2 decrypt port {addr} is not reachable ({e}). Check that the \
                 daemon is running and that its decrypt port is published."
            ))
        })?;
        Ok(DecryptSession {
            stream: Some(stream),
            next_request_id: 1,
        })
    }
}

/// One decrypt conversation with the daemon. Dropping it closes the socket; call
/// [`DecryptSession::close`] to send the protocol's CLOSE frame first.
pub struct DecryptSession {
    stream: Option<TcpStream>,
    next_request_id: u32,
}

impl DecryptSession {
    /// Decrypts one batch. `key_uri` is the FairPlay `skd://` URI, not a Widevine
    /// PSSH, and every sample must be the already-gathered *encrypted* bytes.
    pub async fn decrypt_samples(
        &mut self,
        adam_id: &str,
        key_uri: &str,
        samples: &[Vec<u8>],
    ) -> MhResult<Vec<Vec<u8>>> {
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        validate_label("adam_id", adam_id)?;
        validate_label("skd_uri", key_uri)?;
        if samples.iter().any(|s| s.is_empty()) {
            return Err(MhError::Other(
                "wrapper-v2 rejects empty samples in a decrypt batch".into(),
            ));
        }

        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| MhError::Other("wrapper-v2 decrypt session is already closed".into()))?;

        let payload = encode_batch_payload(adam_id, key_uri, samples);
        write_frame(stream, DECRYPT_KIND_BATCH, request_id, &payload).await?;

        let (kind, response_id, body) = read_frame(stream).await?;
        if response_id != request_id {
            return Err(MhError::Other(format!(
                "wrapper-v2 answered decrypt request {request_id} with id {response_id}"
            )));
        }
        match kind {
            DECRYPT_KIND_OK => {
                let plains = decode_ok_payload(&body)?;
                if plains.len() != samples.len() {
                    return Err(MhError::Other(format!(
                        "wrapper-v2 returned {} plaintexts for {} samples",
                        plains.len(),
                        samples.len()
                    )));
                }
                Ok(plains)
            }
            DECRYPT_KIND_ERROR => Err(MhError::Other(format!(
                "wrapper-v2 refused to decrypt: {}",
                String::from_utf8_lossy(&body).trim()
            ))),
            other => Err(MhError::Other(format!(
                "wrapper-v2 replied with unexpected frame kind {other}"
            ))),
        }
    }

    pub async fn close(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            let _ = write_frame(&mut stream, DECRYPT_KIND_CLOSE, 0, &[]).await;
        }
    }
}

fn validate_label(name: &str, value: &str) -> MhResult<()> {
    if value.is_empty() {
        return Err(MhError::Other(format!(
            "wrapper-v2: {name} must not be empty"
        )));
    }
    if value.len() > u16::MAX as usize {
        return Err(MhError::Other(format!(
            "wrapper-v2: {name} is too long for the decrypt protocol"
        )));
    }
    Ok(())
}

/// `u16 adam_id_len | u16 uri_len | u32 sample_count | u32 sample_len[..] | adam_id |
/// uri | samples[..]`, all big-endian. The two identifier lengths are 16-bit — the
/// README leaves the widths unstated and they are not u32.
fn encode_batch_payload(adam_id: &str, key_uri: &str, samples: &[Vec<u8>]) -> Vec<u8> {
    let total: usize = samples.iter().map(|s| s.len()).sum();
    let mut out = Vec::with_capacity(8 + samples.len() * 4 + adam_id.len() + key_uri.len() + total);
    out.extend_from_slice(&(adam_id.len() as u16).to_be_bytes());
    out.extend_from_slice(&(key_uri.len() as u16).to_be_bytes());
    out.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for s in samples {
        out.extend_from_slice(&(s.len() as u32).to_be_bytes());
    }
    out.extend_from_slice(adam_id.as_bytes());
    out.extend_from_slice(key_uri.as_bytes());
    for s in samples {
        out.extend_from_slice(s);
    }
    out
}

fn decode_ok_payload(body: &[u8]) -> MhResult<Vec<Vec<u8>>> {
    let mut cur = 0usize;
    let count = read_u32(body, &mut cur)? as usize;
    let mut lens = Vec::with_capacity(count);
    for _ in 0..count {
        lens.push(read_u32(body, &mut cur)? as usize);
    }
    let mut out = Vec::with_capacity(count);
    for len in lens {
        let end = cur
            .checked_add(len)
            .filter(|e| *e <= body.len())
            .ok_or_else(|| {
                MhError::Parse("wrapper-v2 decrypt reply is shorter than its lengths".into())
            })?;
        out.push(body[cur..end].to_vec());
        cur = end;
    }
    Ok(out)
}

fn read_u32(buf: &[u8], cur: &mut usize) -> MhResult<u32> {
    let end = *cur + 4;
    if end > buf.len() {
        return Err(MhError::Parse(
            "wrapper-v2 frame ended mid-integer".to_string(),
        ));
    }
    let v = u32::from_be_bytes([buf[*cur], buf[*cur + 1], buf[*cur + 2], buf[*cur + 3]]);
    *cur = end;
    Ok(v)
}

fn encode_frame(kind: u16, request_id: u32, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(16 + payload.len());
    frame.extend_from_slice(DECRYPT_MAGIC);
    frame.extend_from_slice(&DECRYPT_VERSION.to_be_bytes());
    frame.extend_from_slice(&kind.to_be_bytes());
    frame.extend_from_slice(&request_id.to_be_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

async fn write_frame(
    stream: &mut TcpStream,
    kind: u16,
    request_id: u32,
    payload: &[u8],
) -> MhResult<()> {
    stream
        .write_all(&encode_frame(kind, request_id, payload))
        .await
        .map_err(MhError::Io)?;
    stream.flush().await.map_err(MhError::Io)?;
    Ok(())
}

async fn read_frame(stream: &mut TcpStream) -> MhResult<(u16, u32, Vec<u8>)> {
    let mut header = [0u8; 16];
    stream.read_exact(&mut header).await.map_err(|e| {
        MhError::Other(format!(
            "wrapper-v2 closed the connection before replying ({e}) — the worker most \
             likely crashed on this track"
        ))
    })?;
    if &header[0..4] != DECRYPT_MAGIC {
        return Err(MhError::Parse(
            "wrapper-v2 reply did not start with the WV2D magic".into(),
        ));
    }
    let version = u16::from_be_bytes([header[4], header[5]]);
    if version != DECRYPT_VERSION {
        return Err(MhError::Unsupported(format!(
            "wrapper-v2 speaks decrypt protocol version {version}, MediaHarbor implements \
             {DECRYPT_VERSION}"
        )));
    }
    let kind = u16::from_be_bytes([header[6], header[7]]);
    let request_id = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
    let len = u32::from_be_bytes([header[12], header[13], header[14], header[15]]);
    if len > MAX_PAYLOAD_BYTES {
        return Err(MhError::Parse(format!(
            "wrapper-v2 announced an implausible {len}-byte payload"
        )));
    }
    let mut payload = vec![0u8; len as usize];
    stream.read_exact(&mut payload).await.map_err(MhError::Io)?;
    Ok((kind, request_id, payload))
}

/// Apple's per-account master URLs carry a variant-set token (`P123_something.m3u8`).
/// Swapping it for `_default` is what exposes the ALAC renditions — the token in the
/// URL the API hands out selects a restricted variant set that has none.
pub fn switch_master_url_to_default(url: &str) -> String {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(P\d+)_[^/]+(\.m3u8)").expect("static regex")
    });
    RE.replace(url, "${1}_default${2}").into_owned()
}

/// The master playlist URL for a song, preferring the wrapper's own playback
/// dispatch over anything the web token can reach.
pub fn master_url_from_playback(playback: &Value) -> Option<String> {
    playback
        .pointer("/songList/0/hls-playlist-url")
        .and_then(|v| v.as_str())
        .filter(|u| !u.is_empty())
        .map(switch_master_url_to_default)
}

/// One sample split into the parts the daemon needs and the parts it must not see.
///
/// CBCS encrypts only whole 16-byte blocks, and a sample's subsample map marks
/// which byte ranges are encrypted at all. Only `encrypted` travels to the
/// wrapper; `clear_tail` and the clear subsample ranges stay local and are woven
/// back in afterwards.
#[derive(Debug, Clone, Default)]
pub struct SamplePlan {
    pub original_len: usize,
    pub encrypted: Vec<u8>,
    pub clear_tail: Vec<u8>,
    /// `(clear_bytes, encrypted_bytes)` pairs, in order. Empty means the whole
    /// 16-byte-aligned prefix is encrypted.
    pub subsamples: Vec<(usize, usize)>,
}

/// Splits a sample for transmission. With no subsample map the encrypted region is
/// the 16-byte-aligned prefix and the `len % 16` remainder is already clear.
pub fn plan_sample(data: &[u8], subsamples: &[(usize, usize)]) -> SamplePlan {
    if subsamples.is_empty() {
        let aligned = data.len() & !0xF;
        return SamplePlan {
            original_len: data.len(),
            encrypted: data[..aligned].to_vec(),
            clear_tail: data[aligned..].to_vec(),
            subsamples: Vec::new(),
        };
    }

    let mut encrypted = Vec::new();
    let mut offset = 0usize;
    for (clear, enc) in subsamples {
        offset = offset.saturating_add(*clear);
        let end = offset.saturating_add(*enc).min(data.len());
        if offset < end {
            encrypted.extend_from_slice(&data[offset..end]);
        }
        offset = end;
    }
    SamplePlan {
        original_len: data.len(),
        encrypted,
        clear_tail: Vec::new(),
        subsamples: subsamples.to_vec(),
    }
}

/// Weaves decrypted bytes back into their original sample, restoring the clear
/// ranges the wrapper never saw.
pub fn reassemble_sample(data: &[u8], plan: &SamplePlan, plaintext: &[u8]) -> MhResult<Vec<u8>> {
    let mut decrypted = Vec::with_capacity(plaintext.len() + plan.clear_tail.len());
    decrypted.extend_from_slice(plaintext);
    decrypted.extend_from_slice(&plan.clear_tail);

    if plan.subsamples.is_empty() {
        if decrypted.len() != data.len() {
            return Err(MhError::Other(format!(
                "decrypted sample length mismatch: expected {}, got {}",
                data.len(),
                decrypted.len()
            )));
        }
        return Ok(decrypted);
    }

    let encrypted_total: usize = plan.subsamples.iter().map(|(_, e)| *e).sum();
    if decrypted.len() != encrypted_total {
        return Err(MhError::Other(format!(
            "decrypted subsample length mismatch: expected {encrypted_total}, got {}",
            decrypted.len()
        )));
    }

    let mut out = Vec::with_capacity(data.len());
    let mut dec_off = 0usize;
    let mut offset = 0usize;
    for (clear, enc) in &plan.subsamples {
        let clear_end = offset
            .checked_add(*clear)
            .filter(|e| *e <= data.len())
            .ok_or_else(|| MhError::Other("subsample clear range exceeds sample".into()))?;
        out.extend_from_slice(&data[offset..clear_end]);
        offset = clear_end;

        let dec_end = dec_off
            .checked_add(*enc)
            .filter(|e| *e <= decrypted.len())
            .ok_or_else(|| MhError::Other("subsample decrypt range exceeds plaintext".into()))?;
        let enc_end = offset
            .checked_add(*enc)
            .filter(|e| *e <= data.len())
            .ok_or_else(|| MhError::Other("subsample encrypted range exceeds sample".into()))?;
        out.extend_from_slice(&decrypted[dec_off..dec_end]);
        dec_off = dec_end;
        offset = enc_end;
    }
    if offset < data.len() {
        out.extend_from_slice(&data[offset..]);
    }
    if out.len() != data.len() {
        return Err(MhError::Other(format!(
            "reassembled sample length mismatch: expected {}, got {}",
            data.len(),
            out.len()
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors wrapper-v2's own `decrypt_batch_payload_layout` test: the two
    /// identifier lengths are u16, the counts and sample lengths are u32.
    #[test]
    fn a_logged_out_daemon_is_not_reported_as_signed_in() {
        let health =
            serde_json::json!({"status": "ok", "version": "0.0.2", "mode": "rust-supervisor"});
        let me = serde_json::json!({"auth": {"state": "logged_out"}});
        let st = parse_status(&health, &me);
        assert!(!st.authenticated);
        assert_eq!(st.state, "logged_out");
        assert_eq!(st.version, "0.0.2");
    }

    #[test]
    fn an_authenticated_snapshot_is_recognised() {
        let me = serde_json::json!({
            "auth": {"state": "authenticated", "apple_id": "a@b.c", "storefront": "tr"},
            "runtime": {"playback_ready": true}
        });
        let st = parse_status(&serde_json::json!({}), &me);
        assert!(st.authenticated);
        assert!(st.playback_ready);
        assert_eq!(st.apple_id.as_deref(), Some("a@b.c"));
    }

    #[test]
    fn mid_flow_states_are_not_authenticated() {
        for state in ["in_progress", "awaiting_2fa", "failed", "unknown"] {
            let me = serde_json::json!({"auth": {"state": state}});
            assert!(
                !parse_status(&serde_json::json!({}), &me).authenticated,
                "{state} must not count as signed in"
            );
        }
    }

    #[test]
    fn login_status_codes_map_to_the_daemons_states() {
        let empty = serde_json::json!({});
        assert_eq!(
            LoginOutcome::from_response(200, &empty),
            LoginOutcome::Authenticated
        );
        assert_eq!(
            LoginOutcome::from_response(202, &empty),
            LoginOutcome::Needs2fa
        );
        assert!(matches!(
            LoginOutcome::from_response(504, &empty),
            LoginOutcome::Failed(m) if m.contains("timed out")
        ));
    }

    #[test]
    fn a_failed_login_carries_the_daemons_own_reason() {
        let body = serde_json::json!({"error": "bad_credentials", "detail": "Apple said no"});
        match LoginOutcome::from_response(401, &body) {
            LoginOutcome::Failed(m) => {
                assert!(m.contains("401"), "{m}");
                assert!(m.contains("Apple said no"), "{m}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn the_variant_token_is_swapped_for_default() {
        assert_eq!(
            switch_master_url_to_default(
                "https://aod.itunes.apple.com/x/P123456_a1b2c3.m3u8?aec=HD"
            ),
            "https://aod.itunes.apple.com/x/P123456_default.m3u8?aec=HD"
        );
    }

    #[test]
    fn a_url_without_a_variant_token_is_left_alone() {
        let url = "https://aod.itunes.apple.com/x/master.m3u8";
        assert_eq!(switch_master_url_to_default(url), url);
    }

    #[test]
    fn the_playback_dispatch_yields_a_default_master() {
        let playback = serde_json::json!({
            "songList": [{"hls-playlist-url": "https://x/P9_abc.m3u8"}]
        });
        assert_eq!(
            master_url_from_playback(&playback).unwrap(),
            "https://x/P9_default.m3u8"
        );
        assert!(master_url_from_playback(&serde_json::json!({"songList": []})).is_none());
    }

    #[test]
    fn a_sample_without_subsamples_splits_on_the_block_boundary() {
        let data: Vec<u8> = (0u8..37).collect();
        let plan = plan_sample(&data, &[]);
        assert_eq!(plan.encrypted.len(), 32);
        assert_eq!(plan.clear_tail.len(), 5);
        assert_eq!(plan.clear_tail, data[32..].to_vec());
    }

    #[test]
    fn an_unencrypted_remainder_round_trips_unchanged() {
        let data: Vec<u8> = (0u8..37).collect();
        let plan = plan_sample(&data, &[]);
        let out = reassemble_sample(&data, &plan, &plan.encrypted).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn subsample_maps_keep_their_clear_ranges_local() {
        let data: Vec<u8> = (0u8..40).collect();
        let subs = vec![(8usize, 16usize), (4usize, 12usize)];
        let plan = plan_sample(&data, &subs);
        assert_eq!(plan.encrypted.len(), 28);
        assert_eq!(&plan.encrypted[..16], &data[8..24]);
        assert_eq!(&plan.encrypted[16..], &data[28..40]);

        let out = reassemble_sample(&data, &plan, &plan.encrypted).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn a_short_plaintext_is_rejected_rather_than_silently_truncating() {
        let data: Vec<u8> = (0u8..32).collect();
        let plan = plan_sample(&data, &[]);
        assert!(reassemble_sample(&data, &plan, &plan.encrypted[..16]).is_err());
    }

    #[test]
    fn batch_payload_matches_the_daemons_own_layout_test() {
        let payload = encode_batch_payload("123", "skd://x", &[vec![1, 2, 3, 4]]);
        assert_eq!(&payload[0..2], &3u16.to_be_bytes());
        assert_eq!(&payload[2..4], &7u16.to_be_bytes());
        assert_eq!(&payload[4..8], &1u32.to_be_bytes());
        assert_eq!(&payload[8..12], &4u32.to_be_bytes());
        assert_eq!(&payload[12..15], b"123");
        assert_eq!(&payload[15..22], b"skd://x");
        assert_eq!(&payload[22..], &[1, 2, 3, 4]);
    }

    #[test]
    fn the_decrypt_frame_header_is_sixteen_bytes() {
        let frame = encode_frame(DECRYPT_KIND_BATCH, 7, b"xy");
        assert_eq!(&frame[0..4], b"WV2D");
        assert_eq!(u16::from_be_bytes([frame[4], frame[5]]), 1);
        assert_eq!(u16::from_be_bytes([frame[6], frame[7]]), DECRYPT_KIND_BATCH);
        assert_eq!(
            u32::from_be_bytes([frame[8], frame[9], frame[10], frame[11]]),
            7
        );
        assert_eq!(
            u32::from_be_bytes([frame[12], frame[13], frame[14], frame[15]]),
            2
        );
        assert_eq!(&frame[16..], b"xy");
    }

    #[test]
    fn ok_payload_round_trips() {
        let samples = vec![vec![9u8, 8, 7], vec![], vec![1]];
        let mut body = Vec::new();
        body.extend_from_slice(&(samples.len() as u32).to_be_bytes());
        for s in &samples {
            body.extend_from_slice(&(s.len() as u32).to_be_bytes());
        }
        for s in &samples {
            body.extend_from_slice(s);
        }
        assert_eq!(decode_ok_payload(&body).unwrap(), samples);
    }

    #[test]
    fn a_truncated_reply_is_rejected_rather_than_panicking() {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&99u32.to_be_bytes());
        body.extend_from_slice(&[1, 2, 3]);
        assert!(decode_ok_payload(&body).is_err());
    }

    #[test]
    fn config_is_absent_until_the_user_enables_it() {
        let mut s = Settings::default();
        assert!(WrapperConfig::from_settings(&s).is_none());
        s.apple_use_wrapper = true;
        let cfg = WrapperConfig::from_settings(&s).expect("enabled");
        assert_eq!(cfg.base_url, "http://127.0.0.1");
        assert_eq!(cfg.decrypt_port, 10020);
    }

    #[test]
    fn a_trailing_slash_in_the_url_does_not_double_up() {
        let s = Settings {
            apple_use_wrapper: true,
            apple_wrapper_url: "http://box.local:8080/".into(),
            ..Default::default()
        };
        let cfg = WrapperConfig::from_settings(&s).expect("enabled");
        assert_eq!(cfg.base_url, "http://box.local:8080");
    }
}

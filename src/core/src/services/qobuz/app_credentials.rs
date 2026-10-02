use base64::Engine;
use futures_util::StreamExt;
use std::collections::HashMap;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::services::common::ids::now_secs;

const BASE_URL: &str = "https://www.qobuz.com/api.json/0.2";
const PLAY_HOST: &str = "https://play.qobuz.com";

/// Web player builds whose bundle still ships the app credentials of a past era.
/// These are public asset paths, not credentials: the app_id and the signing
/// secret are parsed out of whatever Qobuz serves at the path. Ordered newest
/// first; each entry is the earliest mint time it covers. The current era needs
/// no entry — it is whatever `/login` links to right now.
const ERA_BUNDLES: &[(i64, &str)] = &[(1723420800, "7.2.0-b100"), (0, "7.1.4-b008")];

/// Newest era boundary; a token minted at or after this came from the live bundle.
const LIVE_ERA_FROM: i64 = 1746489600;

/// Qobuz tokens decode to this many bytes, opening with the mint time.
const TOKEN_BYTES: usize = 64;
const TOKEN_EPOCH_FLOOR: i64 = 1420070400;

/// A track that exists in every storefront, used to make a signed request whose
/// signature Qobuz will judge before it looks at any token.
const PROBE_TRACK_ID: &str = "19512574";
const PROBE_FORMAT_ID: &str = "27";

/// A ceiling on how much of a bundle is held in memory. Not an optimisation:
/// where the credentials sit varies by build (3.4 MB in 8.2.0, 6.4 MB in 7.1.4),
/// so the whole file is read and this only stops a pathological response.
const BUNDLE_BYTE_CEILING: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPair {
    pub app_id: String,
    pub secret: String,
}

#[derive(Debug, Clone, Default)]
pub struct BundleCreds {
    pub production_app_id: Option<String>,
    pub app_ids: Vec<String>,
    pub secrets: Vec<String>,
}

impl BundleCreds {
    /// App ids best-first: the one the player actually ships with, then the rest.
    fn ordered_app_ids(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(ref p) = self.production_app_id {
            out.push(p.clone());
        }
        for id in &self.app_ids {
            if !out.contains(id) {
                out.push(id.clone());
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppVerdict {
    OwnsToken,
    NotThisApp,
    DeadAppId,
    Unreachable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretVerdict {
    Signs,
    BadSignature,
    ClockSkew,
    Unreachable(String),
}

/// When the token was minted, read out of the token itself.
///
/// Qobuz packs the mint time into the first four bytes; the remaining sixty are
/// opaque. A value outside a sane window means this token does not carry one, so
/// callers fall back to searching rather than trusting a bogus date.
pub fn minted_at(token: &str) -> Option<i64> {
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.trim().trim_end_matches('='))
        .ok()?;
    if raw.len() != TOKEN_BYTES {
        return None;
    }
    let ts = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as i64;
    (TOKEN_EPOCH_FLOOR..=now_secs() as i64)
        .contains(&ts)
        .then_some(ts)
}

/// The bundle version that shipped the credentials a token of this age needs.
/// `None` means the live bundle covers it.
fn era_bundle_for(minted: Option<i64>) -> Option<&'static str> {
    let ts = minted?;
    if ts >= LIVE_ERA_FROM {
        return None;
    }
    ERA_BUNDLES
        .iter()
        .find(|(from, _)| ts >= *from)
        .map(|(_, version)| *version)
}

/// The pair the user configured, if both halves are present. `qobuz_app_secret`
/// is the older field name and still what OrpheusDL is handed, so either counts.
pub fn configured_pair(s: &Settings) -> Option<AppPair> {
    let app_id = s.qobuz_app_id.trim();
    if app_id.is_empty() {
        return None;
    }
    let secret = s
        .qobuz_secrets
        .split(',')
        .map(str::trim)
        .find(|x| !x.is_empty())
        .unwrap_or_else(|| s.qobuz_app_secret.trim());
    (!secret.is_empty()).then(|| AppPair {
        app_id: app_id.to_string(),
        secret: secret.to_string(),
    })
}

/// The app id the settings point at, whatever the state of the secret.
pub fn configured_app_id(s: &Settings) -> Option<String> {
    let id = s.qobuz_app_id.trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// The `request_sig` Qobuz demands on `track/getFileUrl`: the call's parameters in one
/// fixed order, salted with the app secret and MD5-hexed. Qobuz rejects the call
/// outright if any byte of the payload is wrong, and the same signature is needed to
/// stream a track, to probe whether an app id works, and to obtain the play-proof the
/// listening-history write requires — so the formula is spelled out exactly once.
pub fn file_url_signature(track_id: &str, format_id: &str, ts: &str, secret: &str) -> String {
    use md5::{Digest, Md5};
    let payload =
        format!("trackgetFileUrlformat_id{format_id}intentstreamtrack_id{track_id}{ts}{secret}");
    hex::encode(Md5::digest(payload.as_bytes()))
}

/// Does this app id accept the token? One `user/get`; the status is the whole
/// answer, and the body is never read because it carries account details.
async fn probe_app(
    client: &reqwest::Client,
    app_id: &str,
    user_id: &str,
    token: &str,
) -> AppVerdict {
    let res = client
        .get(format!("{BASE_URL}/user/get"))
        .header("X-App-Id", app_id)
        .header("X-User-Auth-Token", token)
        .query(&[("app_id", app_id), ("user_id", user_id)])
        .send()
        .await;
    match res {
        Ok(r) if r.status().is_success() => AppVerdict::OwnsToken,
        Ok(r) if r.status().as_u16() == 400 => AppVerdict::DeadAppId,
        Ok(_) => AppVerdict::NotThisApp,
        Err(e) => AppVerdict::Unreachable(e.to_string()),
    }
}

/// Does this secret sign for this app id? Sent with no token at all: Qobuz
/// checks the signature first, so a 401 means the signature passed and only the
/// missing credentials stopped it, while a 400 names what was wrong.
async fn probe_secret(client: &reqwest::Client, app_id: &str, secret: &str) -> SecretVerdict {
    let ts = now_secs().to_string();
    let sig = file_url_signature(PROBE_TRACK_ID, PROBE_FORMAT_ID, &ts, secret);
    let res = client
        .get(format!("{BASE_URL}/track/getFileUrl"))
        .header("X-App-Id", app_id)
        .query(&[
            ("request_ts", ts.as_str()),
            ("request_sig", sig.as_str()),
            ("track_id", PROBE_TRACK_ID),
            ("format_id", PROBE_FORMAT_ID),
            ("intent", "stream"),
            ("app_id", app_id),
        ])
        .send()
        .await;
    let resp = match res {
        Ok(r) => r,
        Err(e) => return SecretVerdict::Unreachable(e.to_string()),
    };
    let status = resp.status().as_u16();
    if status == 401 {
        return SecretVerdict::Signs;
    }
    let body = resp.text().await.unwrap_or_default();
    if body.contains("request_ts") {
        SecretVerdict::ClockSkew
    } else if status == 400 {
        SecretVerdict::BadSignature
    } else {
        SecretVerdict::Signs
    }
}

/// Which of these secrets signs for `app_id`, and why the others did not.
async fn secret_for(
    client: &reqwest::Client,
    app_id: &str,
    candidates: &[String],
    trail: &mut Vec<String>,
) -> Option<String> {
    for secret in candidates {
        match probe_secret(client, app_id, secret).await {
            SecretVerdict::Signs => return Some(secret.clone()),
            SecretVerdict::BadSignature => {}
            SecretVerdict::ClockSkew => {
                trail.push(
                    "signature probe rejected for a stale timestamp — this machine's clock is off"
                        .into(),
                );
                return None;
            }
            SecretVerdict::Unreachable(e) => trail.push(format!("secret probe unreachable: {e}")),
        }
    }
    None
}

/// The signing secret the current web player uses for `app_id`.
///
/// Costs the account nothing — the pairing is settled by the signature check
/// alone. Used after a login capture, where the app_id is already known because
/// the page told us which one it was using.
pub async fn live_secret_for(client: &reqwest::Client, app_id: &str) -> Option<String> {
    let mut trail = Vec::new();
    let version = live_bundle_version(client).await.ok()?;
    let creds = creds_from_bundle(client, &version, &mut trail).await?;
    secret_for(client, app_id, &creds.secrets, &mut trail).await
}

/// The version string the live player links to.
async fn live_bundle_version(client: &reqwest::Client) -> MhResult<String> {
    let login = client
        .get(format!("{PLAY_HOST}/login"))
        .send()
        .await?
        .text()
        .await?;
    let re =
        regex::Regex::new(r#"<script src="/resources/([\d.]+-[a-z]\d+)/bundle\.js"></script>"#)?;
    re.captures(&login)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| MhError::Parse("Qobuz: no bundle.js link on the login page".into()))
}

/// Reads a bundle. They run to nine megabytes, and the credentials can sit
/// anywhere in one, so it is read whole.
async fn fetch_bundle(client: &reqwest::Client, version: &str) -> MhResult<Option<String>> {
    let resp = client
        .get(format!("{PLAY_HOST}/resources/{version}/bundle.js"))
        .send()
        .await?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(MhError::Other(format!(
            "Qobuz: bundle {version} returned HTTP {}",
            resp.status()
        )));
    }

    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        body.extend_from_slice(&chunk.map_err(MhError::Network)?);
        if body.len() >= BUNDLE_BYTE_CEILING {
            break;
        }
    }
    Ok(Some(String::from_utf8_lossy(&body).into_owned()))
}

/// Pulls the app ids and the signing secrets out of a bundle.
///
/// The secrets are never written down whole: each is split across an
/// `initialSeed` call and a timezone entry further up the file, and the player
/// reassembles them at runtime. Two of the timezone keys are emitted behind
/// ternaries that never fire, so the second pair is the live one — hence the
/// swap before the info/extras pass.
pub fn parse_bundle(bundle: &str) -> MhResult<BundleCreds> {
    let mut out = BundleCreds::default();

    let production_re = regex::Regex::new(r#"production:\{api:\{appId:"(\d{9})""#)?;
    out.production_app_id = production_re
        .captures(bundle)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    let any_re = regex::Regex::new(r#"appId:"(\d{9})""#)?;
    for c in any_re.captures_iter(bundle) {
        if let Some(m) = c.get(1) {
            let id = m.as_str().to_string();
            if !out.app_ids.contains(&id) {
                out.app_ids.push(id);
            }
        }
    }

    let seed_re = regex::Regex::new(
        r#"[a-z]\.initialSeed\("(?P<seed>[\w=]+)",window\.utimezone\.(?P<tz>[a-z]+)\)"#,
    )?;
    let mut parts: HashMap<String, Vec<String>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for cap in seed_re.captures_iter(bundle) {
        let tz = cap.name("tz").unwrap().as_str().to_string();
        let seed = cap.name("seed").unwrap().as_str().to_string();
        if !parts.contains_key(&tz) {
            order.push(tz.clone());
        }
        parts.entry(tz).or_default().push(seed);
    }
    if order.len() < 2 {
        return Ok(out);
    }
    order.swap(0, 1);

    let tz_pattern = order
        .iter()
        .map(|tz| {
            let mut chars = tz.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => tz.clone(),
            }
        })
        .collect::<Vec<_>>()
        .join("|");
    let info_re = regex::Regex::new(&format!(
        r#"name:"\w+/(?P<tz>{tz_pattern})",info:"(?P<info>[\w=]+)",extras:"(?P<extras>[\w=]+)""#
    ))?;
    for cap in info_re.captures_iter(bundle) {
        let tz = cap.name("tz").unwrap().as_str().to_lowercase();
        if let Some(bucket) = parts.get_mut(&tz) {
            bucket.push(cap.name("info").unwrap().as_str().to_string());
            bucket.push(cap.name("extras").unwrap().as_str().to_string());
        }
    }

    for tz in &order {
        let Some(bucket) = parts.get(tz) else {
            continue;
        };
        let joined = bucket.concat();
        if joined.len() < 44 {
            continue;
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&joined[..joined.len() - 44])
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();
        if !decoded.is_empty() && !out.secrets.contains(&decoded) {
            out.secrets.push(decoded);
        }
    }

    Ok(out)
}

/// Everything one bundle can tell us, or `None` if Qobuz no longer serves it.
async fn creds_from_bundle(
    client: &reqwest::Client,
    version: &str,
    trail: &mut Vec<String>,
) -> Option<BundleCreds> {
    match fetch_bundle(client, version).await {
        Ok(Some(body)) => match parse_bundle(&body) {
            Ok(creds) if !creds.app_ids.is_empty() => {
                trail.push(format!(
                    "bundle {version}: app_ids {:?}, {} secret(s)",
                    creds.ordered_app_ids(),
                    creds.secrets.len()
                ));
                Some(creds)
            }
            Ok(_) => {
                trail.push(format!("bundle {version}: no app id in this build"));
                None
            }
            Err(e) => {
                trail.push(format!("bundle {version}: unreadable ({e})"));
                None
            }
        },
        Ok(None) => {
            trail.push(format!("bundle {version}: no longer served"));
            None
        }
        Err(e) => {
            trail.push(format!("bundle {version}: {e}"));
            None
        }
    }
}

/// Walks a bundle's app ids until one owns the token, then finds its secret.
async fn pair_from_bundle(
    client: &reqwest::Client,
    creds: &BundleCreds,
    user_id: &str,
    token: &str,
    tried: &mut Vec<String>,
    owner: &mut Option<String>,
    trail: &mut Vec<String>,
) -> Option<AppPair> {
    for app_id in creds.ordered_app_ids() {
        if tried.contains(&app_id) {
            continue;
        }
        tried.push(app_id.clone());
        match probe_app(client, &app_id, user_id, token).await {
            AppVerdict::OwnsToken => {
                trail.push(format!("app_id {app_id} owns this token"));
                *owner = Some(app_id.clone());
                match secret_for(client, &app_id, &creds.secrets, trail).await {
                    Some(secret) => return Some(AppPair { app_id, secret }),
                    None => trail.push(format!("no secret in this bundle signs for {app_id}")),
                }
            }
            AppVerdict::NotThisApp => trail.push(format!("app_id {app_id} rejected the token")),
            AppVerdict::DeadAppId => trail.push(format!("app_id {app_id} is no longer valid")),
            AppVerdict::Unreachable(e) => trail.push(format!("app_id {app_id} unreachable: {e}")),
        }
    }
    None
}

const EXPIRED_ERR: &str = "Qobuz: this token is no longer valid — no Qobuz app accepts it. Click \
                           \"Sign in with Qobuz\" in Settings to capture a fresh one.";

fn no_secret_err(app_id: &str) -> String {
    format!(
        "Qobuz: your token belongs to app_id {app_id}, but none of Qobuz's web bundles ship a \
         signing secret for it. Click \"Sign in with Qobuz\" to mint a token on the current app, \
         or paste a secret under Settings → Qobuz → App credentials override."
    )
}

/// The app id and signing secret this token needs.
///
/// The token carries the date it was minted, and Qobuz still serves the web
/// player build that was live then, so the usual path is one bundle and one
/// confirming call. Everything after that is for tokens whose date is missing or
/// misleading.
pub async fn resolve(
    client: &reqwest::Client,
    settings: &Settings,
) -> (MhResult<AppPair>, Vec<String>) {
    let mut trail: Vec<String> = Vec::new();
    let outcome = resolve_inner(client, settings, &mut trail).await;
    (outcome, trail)
}

async fn resolve_inner(
    client: &reqwest::Client,
    settings: &Settings,
    trail: &mut Vec<String>,
) -> MhResult<AppPair> {
    let user_id = settings.qobuz_email_or_userid.trim();
    let token = settings.qobuz_password_or_token.trim();
    if user_id.is_empty() || token.is_empty() {
        return Err(MhError::Auth(
            "Qobuz credentials not set. Open Settings → Qobuz and click \"Sign in with Qobuz\" \
             to capture your user_id and auth token."
                .into(),
        ));
    }

    let mut tried: Vec<String> = Vec::new();
    let mut owner: Option<String> = None;

    if let Some(pair) = configured_pair(settings) {
        if probe_app(client, &pair.app_id, user_id, token).await == AppVerdict::OwnsToken {
            owner = Some(pair.app_id.clone());
            if probe_secret(client, &pair.app_id, &pair.secret).await == SecretVerdict::Signs {
                trail.push(format!("kept the stored pair for app_id {}", pair.app_id));
                return Ok(pair);
            }
            trail.push(format!(
                "stored secret no longer signs for app_id {}",
                pair.app_id
            ));
        } else {
            trail.push(format!("stored app_id {} rejected the token", pair.app_id));
            tried.push(pair.app_id);
        }
    }

    let minted = minted_at(token);
    match minted {
        Some(ts) => trail.push(format!("token was minted at {ts}")),
        None => trail.push("token carries no readable mint date".into()),
    }

    let era_version = era_bundle_for(minted);
    if let Some(version) = era_version {
        if let Some(creds) = creds_from_bundle(client, version, trail).await {
            if let Some(pair) = pair_from_bundle(
                client, &creds, user_id, token, &mut tried, &mut owner, trail,
            )
            .await
            {
                return Ok(pair);
            }
        }
    }

    let live_version = live_bundle_version(client).await?;
    if era_version != Some(live_version.as_str()) {
        if let Some(creds) = creds_from_bundle(client, &live_version, trail).await {
            if let Some(pair) = pair_from_bundle(
                client, &creds, user_id, token, &mut tried, &mut owner, trail,
            )
            .await
            {
                return Ok(pair);
            }
        }
    }

    for (_, version) in ERA_BUNDLES {
        if era_version == Some(*version) {
            continue;
        }
        if let Some(creds) = creds_from_bundle(client, version, trail).await {
            if let Some(pair) = pair_from_bundle(
                client, &creds, user_id, token, &mut tried, &mut owner, trail,
            )
            .await
            {
                return Ok(pair);
            }
        }
    }

    Err(MhError::Auth(match owner {
        Some(ref app_id) => no_secret_err(app_id),
        None => EXPIRED_ERR.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    /// Pins the byte-for-byte payload Qobuz signs. This is a wire format: if the
    /// parameter order or spelling drifts, every `track/getFileUrl` call 401s, and
    /// nothing else in the codebase would notice.
    #[test]
    fn file_url_signature_pins_the_payload_qobuz_expects() {
        use md5::{Digest, Md5};
        let expected = hex::encode(Md5::digest(
            b"trackgetFileUrlformat_id27intentstreamtrack_id195125741700000000sekrit",
        ));
        assert_eq!(
            file_url_signature("19512574", "27", "1700000000", "sekrit"),
            expected
        );
    }

    use super::*;

    /// A synthetic token: Qobuz packs the mint time into the first four bytes and
    /// the other sixty are opaque, so filler exercises the same parse a real one does.
    fn token_minted_at(secs: u32) -> String {
        let mut raw = [0x5a_u8; 64];
        raw[..4].copy_from_slice(&secs.to_be_bytes());
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
    }

    #[test]
    fn the_mint_date_comes_out_of_the_token() {
        assert_eq!(minted_at(&token_minted_at(1700021250)), Some(1700021250));
    }

    #[test]
    fn a_token_without_a_readable_date_is_rejected() {
        // A mint time that lands in 2053, outside the window a real token can carry.
        let no_date = token_minted_at(0x9dd6_a8c9);
        assert_eq!(minted_at(&no_date), None);
        assert_eq!(minted_at("not a token"), None);
        assert_eq!(minted_at(""), None);
    }

    #[test]
    fn each_era_maps_to_the_build_that_shipped_it() {
        // 2023-11-15, before the first rotation.
        assert_eq!(era_bundle_for(Some(1700021250)), Some("7.1.4-b008"));
        // 2024-10-01, between the rotations.
        assert_eq!(era_bundle_for(Some(1727740800)), Some("7.2.0-b100"));
        // 2026-01-01 and a token with no date both belong to the live bundle.
        assert_eq!(era_bundle_for(Some(1767225600)), None);
        assert_eq!(era_bundle_for(None), None);
    }

    #[test]
    fn a_bundle_yields_its_app_ids_and_secrets() {
        // Shaped exactly like the real file, with one secret split the way the
        // player splits it: seed + info + extras, then 44 trailing chars.
        let secret = "979549437fcc4a3faad4867b5cd25dcb";
        let encoded = base64::engine::general_purpose::STANDARD.encode(secret);
        let (head, tail) = encoded.split_at(8);
        let bundle = format!(
            r#"production:{{api:{{appId:"950096963",appSecret:"x"}}}} appId:"814460817"
               a.initialSeed("{seed}",window.utimezone.berlin)
               b.initialSeed("{seed2}",window.utimezone.london)
               name:"Europe/Berlin",info:"{info}",extras:"{extras}"
               name:"Europe/London",info:"{info}",extras:"{extras}""#,
            seed = head,
            seed2 = head,
            info = tail,
            extras = "A".repeat(44),
        );
        let creds = parse_bundle(&bundle).unwrap();
        assert_eq!(creds.production_app_id.as_deref(), Some("950096963"));
        assert_eq!(creds.ordered_app_ids(), vec!["950096963", "814460817"]);
        assert!(creds.secrets.contains(&secret.to_string()));
    }

    #[test]
    fn a_build_without_credentials_parses_to_nothing() {
        let creds = parse_bundle("var x = 1;").unwrap();
        assert!(creds.app_ids.is_empty());
        assert!(creds.secrets.is_empty());
        assert!(creds.production_app_id.is_none());
    }

    fn settings_with(app_id: &str, secrets: &str, app_secret: &str) -> Settings {
        Settings {
            qobuz_app_id: app_id.into(),
            qobuz_secrets: secrets.into(),
            qobuz_app_secret: app_secret.into(),
            ..Default::default()
        }
    }

    #[test]
    fn the_configured_pair_prefers_the_field_the_ui_writes() {
        let s = settings_with("950096963", "aaa", "bbb");
        assert_eq!(configured_pair(&s).unwrap().secret, "aaa");
        // `qobuz_app_secret` is the older name and still fills in when it is all
        // that was set — that mismatch is what starved OrpheusDL.
        let s = settings_with("950096963", "", "bbb");
        assert_eq!(configured_pair(&s).unwrap().secret, "bbb");
    }

    #[test]
    fn half_a_pair_is_no_pair() {
        assert!(configured_pair(&settings_with("", "aaa", "")).is_none());
        assert!(configured_pair(&settings_with("950096963", " ", " ")).is_none());
        assert_eq!(
            configured_app_id(&settings_with("950096963", "", "")).as_deref(),
            Some("950096963")
        );
    }
}

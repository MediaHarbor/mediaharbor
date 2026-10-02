//! The secret behind the `/api/token` TOTP parameter.
//!
//! `/api/token` refuses to mint a non-anonymous token without a valid `totp` —
//! probed live: omitting it, or sending a wrong code, is a flat
//! `400 Unauthorized request`. (`totpVer` is *not* checked; only the code is.)
//!
//! The secret cannot be recovered from a capture: six digits of HMAC output,
//! valid for thirty seconds, are not invertible. It does not need to be, because
//! the web player ships the table as plain string literals — which is all the
//! community mirrors ever republished. Parsing it out of Spotify's own bundle
//! means a rotation is picked up on the next scrape instead of whenever someone
//! notices and commits.
//!
//! The bundle itself is fetched by `super::web_player`, which sweeps for the
//! pathfinder hashes in the same pass. This module only parses, plus keeps the
//! mirror as a fallback for the day the bundle format changes.

use serde::{Deserialize, Serialize};

use crate::errors::{MhError, MhResult};

/// Community mirror of the same table. Its host runs anti-scraping that answers
/// a request with no `User-Agent` with a 403 HTML page — which used to surface
/// as "Spotify authentication failed" and send people hunting expired cookies.
const MIRROR_URL: &str =
    "https://git.gay/thereallo/totp-secrets/raw/branch/main/secrets/secretDict.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TotpSecret {
    pub version: u32,
    /// The raw secret bytes as they appear in the source, before the XOR step.
    pub cipher: Vec<u8>,
}

impl TotpSecret {
    /// The HMAC key, derived exactly as the web player does:
    /// `charCodeAt(i) ^ (i % 33 + 9)`, joined as decimal text, used as UTF-8.
    pub fn hmac_key(&self) -> Vec<u8> {
        self.cipher
            .iter()
            .enumerate()
            .map(|(i, &b)| (b ^ (((i % 33) + 9) as u8)).to_string())
            .collect::<String>()
            .into_bytes()
    }
}

/// The newest secret in a bundle body, if it carries the table at all.
pub fn best_in(js: &str) -> Option<TotpSecret> {
    highest(parse_secret_table(js))
}

/// Last resort when the bundle sweep found no table: the community mirror.
pub async fn from_mirror_only(client: &reqwest::Client) -> MhResult<TotpSecret> {
    from_mirror(client).await.map_err(|e| {
        MhError::Other(format!(
            "no TOTP secret in Spotify's web player bundles, and the mirror \
             {MIRROR_URL} failed too: {e}"
        ))
    })
}

async fn from_mirror(client: &reqwest::Client) -> MhResult<TotpSecret> {
    let body = get_text(client, MIRROR_URL).await?;
    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| MhError::Parse(format!("mirror JSON: {e}")))?;
    let obj = parsed
        .as_object()
        .ok_or_else(|| MhError::Parse("mirror payload is not an object".into()))?;
    let secrets = obj.iter().filter_map(|(version, cipher)| {
        Some(TotpSecret {
            version: version.parse().ok()?,
            cipher: cipher
                .as_array()?
                .iter()
                .map(|v| v.as_u64().unwrap_or(0) as u8)
                .collect(),
        })
    });
    highest(secrets.collect()).ok_or_else(|| MhError::Parse("mirror listed no secrets".into()))
}

/// Carries a browser `User-Agent` — the mirror 403s a header-less request.
async fn get_text(client: &reqwest::Client, url: &str) -> MhResult<String> {
    let resp = client
        .get(url)
        .header("user-agent", crate::http_client::UA_CHROME_LATEST)
        .send()
        .await
        .map_err(MhError::Network)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(MhError::Other(format!("HTTP {}", status.as_u16())));
    }
    resp.text().await.map_err(MhError::Network)
}

fn highest(mut secrets: Vec<TotpSecret>) -> Option<TotpSecret> {
    secrets.retain(|s| !s.cipher.is_empty());
    secrets.sort_by_key(|s| s.version);
    secrets.pop()
}

/// Pulls `{secret:'…',version:N}` entries out of the minified bundle.
///
/// Hand-scanned rather than matched with a regex because the literals use both
/// quote styles and contain escapes — v60's secret has a backslash in it, and a
/// pattern that misses it silently drops the entry.
fn parse_secret_table(js: &str) -> Vec<TotpSecret> {
    const MARKER: &str = "{secret:";
    let mut out = Vec::new();
    for (idx, _) in js.match_indices(MARKER) {
        let rest = &js[idx + MARKER.len()..];
        let mut chars = rest.char_indices();
        let Some((_, quote)) = chars.next() else {
            continue;
        };
        if quote != '\'' && quote != '"' {
            continue;
        }
        let mut cipher: Vec<u8> = Vec::new();
        let mut closed_at = None;
        let mut escaped = false;
        for (i, c) in chars {
            if escaped {
                let unescaped = match c {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                };
                if (unescaped as u32) > 255 {
                    cipher.clear();
                    break;
                }
                cipher.push(unescaped as u8);
                escaped = false;
                continue;
            }
            match c {
                '\\' => escaped = true,
                c if c == quote => {
                    closed_at = Some(i);
                    break;
                }
                c if (c as u32) > 255 => {
                    cipher.clear();
                    break;
                }
                c => cipher.push(c as u8),
            }
        }
        let (Some(close), false) = (closed_at, cipher.is_empty()) else {
            continue;
        };
        let after = &rest[close + 1..];
        let Some(tail) = after.strip_prefix(",version:") else {
            continue;
        };
        let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
        if let Ok(version) = digits.parse::<u32>() {
            out.push(TotpSecret { version, cipher });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The literal shape as it appears in `web-player.<hash>.js`, captured
    /// 2026-08-24. v60's secret contains an escaped backslash, and the three
    /// entries use both quote styles.
    const TABLE: &str = r#"let eD=[{secret:',7/*F("rLJ2oxaKL^f+E1xvP@N',version:61},{secret:'OmE{ZA.J^":0FG\\Uz?[@WW',version:60},{secret:"{iOFn;4}<1PFYKPV?5{%u14]M>/V0hDH",version:59}].map(e=>{"#;

    #[test]
    fn the_bundle_table_parses_every_entry() {
        let parsed = parse_secret_table(TABLE);
        let versions: Vec<u32> = parsed.iter().map(|s| s.version).collect();
        assert_eq!(versions, vec![61, 60, 59]);
        assert_eq!(parsed[0].cipher.len(), 26);
        assert_eq!(parsed[1].cipher.len(), 22);
        assert!(parsed[1].cipher.contains(&b'\\'));
        assert_eq!(parsed[2].cipher.len(), 32);
    }

    /// Byte-for-byte what the community mirror publishes for v61 — the mirror is
    /// a copy of this table, so the two must never disagree.
    #[test]
    fn the_parsed_secret_matches_the_published_mirror() {
        let parsed = highest(parse_secret_table(TABLE)).expect("a secret");
        assert_eq!(parsed.version, 61);
        assert_eq!(
            parsed.cipher,
            vec![
                44, 55, 47, 42, 70, 40, 34, 114, 76, 74, 50, 111, 120, 97, 75, 76, 94, 102, 43, 69,
                49, 120, 118, 80, 64, 78
            ]
        );
    }

    /// The web player derives its HMAC key with `charCodeAt(i) ^ (i % 33 + 9)`
    /// joined as decimal text; this is that same string.
    #[test]
    fn the_hmac_key_is_derived_the_way_the_web_player_derives_it() {
        let secret = highest(parse_secret_table(TABLE)).expect("a secret");
        assert_eq!(
            String::from_utf8(secret.hmac_key()).unwrap(),
            "376136387538459893883312310911992847112448894410210511297108"
        );
    }

    #[test]
    fn the_newest_version_wins_regardless_of_order() {
        let out_of_order = vec![
            TotpSecret {
                version: 59,
                cipher: vec![1],
            },
            TotpSecret {
                version: 61,
                cipher: vec![2],
            },
            TotpSecret {
                version: 60,
                cipher: vec![3],
            },
        ];
        assert_eq!(highest(out_of_order).unwrap().version, 61);
        assert!(highest(vec![]).is_none());
        assert!(highest(vec![TotpSecret {
            version: 61,
            cipher: vec![]
        }])
        .is_none());
    }

    #[test]
    fn a_bundle_without_the_table_yields_nothing() {
        assert!(parse_secret_table("let eD=[{encode:(e,t)=>t}];").is_empty());
        assert!(parse_secret_table("{secret:unquoted,version:61}").is_empty());
    }
}

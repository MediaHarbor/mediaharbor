use std::path::Path;

use serde::Deserialize;

use crate::errors::{MhError, MhResult};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiCredentials {
    #[serde(rename = "SPOTIFY_CLIENT_ID", default)]
    pub spotify_client_id: String,

    #[serde(rename = "SPOTIFY_CLIENT_SECRET", default)]
    pub spotify_client_secret: String,

    #[serde(rename = "TIDAL_CLIENT_ID", default)]
    pub tidal_client_id: String,

    #[serde(rename = "TIDAL_CLIENT_SECRET", default)]
    pub tidal_client_secret: String,

    #[serde(rename = "YOUTUBE_API_KEY", default)]
    pub youtube_api_key: String,

    #[serde(rename = "QOBUZ_APP_ID", default)]
    pub qobuz_app_id: String,

    #[serde(rename = "QOBUZ_AUTH_TOKEN", default)]
    pub qobuz_auth_token: String,
}

pub fn load_credentials(path: &Path) -> MhResult<ApiCredentials> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| MhError::Config(format!("Cannot read credentials file: {}", e)))?;
    let creds: ApiCredentials = serde_json::from_str(&data)
        .map_err(|e| MhError::Config(format!("Cannot parse credentials file: {}", e)))?;
    Ok(creds)
}

pub fn load_credentials_or_default(path: &Path) -> ApiCredentials {
    load_credentials(path).unwrap_or_default()
}

fn yt_key() -> String {
    const O: &[u8] = &[
        0x1B, 0x13, 0x20, 0x3B, 0x09, 0x23, 0x1B, 0x6B, 0x68, 0x3F, 0x28, 0x10, 0x36, 0x69, 0x02,
        0x38, 0x3B, 0x00, 0x0D, 0x31, 0x3D, 0x3C, 0x03, 0x3F, 0x0C, 0x0D, 0x33, 0x02, 0x35, 0x02,
        0x02, 0x2B, 0x17, 0x3F, 0x32, 0x39, 0x30, 0x3F, 0x62,
    ];
    O.iter().map(|&b| (b ^ 0x5A) as char).collect()
}

pub fn bundled() -> ApiCredentials {
    ApiCredentials {
        spotify_client_id: "".into(),
        spotify_client_secret: "".into(),
        tidal_client_id: "N6Wz7fZO8PTt8Q5e".into(),
        tidal_client_secret: "APESJMjvIY0fxS7QFiYqVMq0IECbcz7aon9A4pyGZ28=".into(),
        youtube_api_key: yt_key(),
        qobuz_app_id: String::new(),
        qobuz_auth_token: String::new(),
    }
}

fn overlay(dst: &mut String, src: String) {
    if !src.trim().is_empty() {
        *dst = src;
    }
}

/// The per-service setting when the user filled it in, otherwise the bundled key.
///
/// The narrowest tier of the same precedence `overlay` applies: settings beat a
/// credentials file, which beats what ships with the build. The call sites need it one
/// field at a time, and each had retyped the `is_empty` ternary — which is how some of
/// them ended up treating a whitespace-only key as a real one.
pub fn preferred(setting: &str, bundled: &str) -> String {
    if setting.trim().is_empty() {
        bundled.to_string()
    } else {
        setting.to_string()
    }
}

/// The bundled keys with every non-empty field of `path` layered over them.
///
/// The middle of three tiers — per-service settings still win at the call sites,
/// so this is for packagers shipping their own keys and for users who would rather
/// drop in a file than type keys into Settings. A missing file is not an error;
/// an unparsable one is, so a typo does not silently fall back.
pub fn bundled_with_overrides(path: &Path) -> MhResult<ApiCredentials> {
    let mut out = bundled();
    if !path.exists() {
        return Ok(out);
    }

    let file = load_credentials(path)?;
    overlay(&mut out.spotify_client_id, file.spotify_client_id);
    overlay(&mut out.spotify_client_secret, file.spotify_client_secret);
    overlay(&mut out.tidal_client_id, file.tidal_client_id);
    overlay(&mut out.tidal_client_secret, file.tidal_client_secret);
    overlay(&mut out.youtube_api_key, file.youtube_api_key);
    overlay(&mut out.qobuz_app_id, file.qobuz_app_id);
    overlay(&mut out.qobuz_auth_token, file.qobuz_auth_token);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
        let p = dir.path().join("credentials.json");
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn missing_file_keeps_bundled_keys() {
        let dir = tempfile::TempDir::new().unwrap();
        let creds = bundled_with_overrides(&dir.path().join("credentials.json")).unwrap();
        assert_eq!(creds.tidal_client_id, bundled().tidal_client_id);
        assert_eq!(creds.youtube_api_key, bundled().youtube_api_key);
    }

    #[test]
    fn partial_file_overrides_only_what_it_sets() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = write(
            &dir,
            r#"{"SPOTIFY_CLIENT_ID":"abc","QOBUZ_APP_ID":"  ","TIDAL_CLIENT_ID":""}"#,
        );
        let creds = bundled_with_overrides(&path).unwrap();
        assert_eq!(creds.spotify_client_id, "abc");
        assert_eq!(creds.tidal_client_id, bundled().tidal_client_id);
        assert_eq!(creds.youtube_api_key, bundled().youtube_api_key);
        assert_eq!(creds.qobuz_app_id, "");
    }

    #[test]
    fn malformed_file_is_an_error_not_a_silent_fallback() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = write(&dir, "{ not json");
        assert!(bundled_with_overrides(&path).is_err());
    }
}

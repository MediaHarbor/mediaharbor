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

pub fn bundled() -> ApiCredentials {
    ApiCredentials {
        spotify_client_id:     "83f950693ddf4e6196c43d92db6f700f".into(),
        spotify_client_secret: "41854f6fd822421283b5df1facbc0925".into(),
        tidal_client_id:       "N6Wz7fZO8PTt8Q5e".into(),
        tidal_client_secret:   "APESJMjvIY0fxS7QFiYqVMq0IECbcz7aon9A4pyGZ28=".into(),
        youtube_api_key:       "AIzaSyAa8RX-ZL8XbYco39ymM4q3alDx2lqRXTY".into(),
        qobuz_app_id:          String::new(),
        qobuz_auth_token:      String::new(),
    }
}

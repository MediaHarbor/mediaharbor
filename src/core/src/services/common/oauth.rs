use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::errors::{MhError, MhResult};

/// A bearer token and the instant it stops being usable, refreshed on demand.
#[derive(Default)]
pub struct TokenCache {
    inner: RwLock<Option<(String, Instant)>>,
}

impl TokenCache {
    /// The cached token, or a freshly minted one. `label` names the service in errors.
    pub async fn client_credentials(
        &self,
        http: &Client,
        token_url: &str,
        label: &str,
        client_id: &str,
        client_secret: &str,
    ) -> MhResult<String> {
        {
            let guard = self.inner.read().await;
            if let Some((token, expires_at)) = guard.as_ref() {
                if Instant::now() < *expires_at {
                    return Ok(token.clone());
                }
            }
        }

        let auth = B64.encode(format!("{}:{}", client_id, client_secret));
        let resp = http
            .post(token_url)
            .header("Authorization", format!("Basic {}", auth))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("grant_type=client_credentials")
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(crate::services::common::http::auth_error(label, resp).await);
        }

        let data: Value = resp.json().await.map_err(MhError::Network)?;
        let token = data["access_token"]
            .as_str()
            .ok_or_else(|| MhError::Auth(format!("Missing access_token in {label} response")))?
            .to_string();
        let ttl = data["expires_in"]
            .as_u64()
            .unwrap_or(3600)
            .saturating_sub(60);
        *self.inner.write().await =
            Some((token.clone(), Instant::now() + Duration::from_secs(ttl)));
        Ok(token)
    }
}

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::auth::netscape;
use crate::defaults::Settings;
use crate::ipc_contract::{CredentialStatusChangedEvent, CredentialsHealthSnapshot};
use crate::services::common::library::ServicePlatform;
use crate::EventEmitter;

const WARN_WINDOW_SECS: i64 = 72 * 3600;

pub const TIDAL_REFRESH_WINDOW_SECS: i64 = 10 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CredentialStatus {
    NotConfigured,
    Ok {
        #[serde(rename = "expiresAt")]
        expires_at: Option<i64>,
    },
    ExpiringSoon {
        #[serde(rename = "expiresAt")]
        expires_at: i64,
    },
    Expired {
        since: Option<i64>,
    },
    Unknown {
        #[serde(rename = "lastCheck")]
        last_check: i64,
        error: String,
    },
}

impl CredentialStatus {
    fn from_expiry(expires_at: Option<i64>) -> Self {
        let now = chrono::Utc::now().timestamp();
        match expires_at {
            None => CredentialStatus::Ok { expires_at: None },
            Some(ts) if ts <= now => CredentialStatus::Expired { since: Some(ts) },
            Some(ts) if ts - now <= WARN_WINDOW_SECS => {
                CredentialStatus::ExpiringSoon { expires_at: ts }
            }
            Some(ts) => CredentialStatus::Ok {
                expires_at: Some(ts),
            },
        }
    }
}

pub fn credential_fingerprint(platform: ServicePlatform, settings: &Settings) -> Option<String> {
    fn hash(bytes: &[u8]) -> String {
        blake3::hash(bytes).to_hex()[..16].to_string()
    }
    fn file_fingerprint(path: &str) -> Option<String> {
        let path = path.trim();
        if path.is_empty() {
            return None;
        }
        let path = PathBuf::from(path);
        if let Ok(bytes) = std::fs::read(&path) {
            return Some(hash(&bytes));
        }
        let meta = std::fs::metadata(&path).ok()?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Some(hash(
            format!("{}:{}:{}", path.display(), meta.len(), mtime).as_bytes(),
        ))
    }
    fn value_fingerprint(value: &str) -> Option<String> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        Some(hash(value.as_bytes()))
    }

    match platform {
        ServicePlatform::Spotify => file_fingerprint(&settings.spotify_cookies_path),
        ServicePlatform::YtMusic | ServicePlatform::Youtube => file_fingerprint(&settings.cookies),
        ServicePlatform::AppleMusic => file_fingerprint(&settings.apple_cookies_path),
        ServicePlatform::Tidal => value_fingerprint(&settings.tidal_refresh_token)
            .or_else(|| value_fingerprint(&settings.tidal_access_token)),
        ServicePlatform::Qobuz => value_fingerprint(&settings.qobuz_password_or_token),
        ServicePlatform::Deezer => value_fingerprint(&settings.deezer_arl),
    }
}

pub struct CredentialsHealth {
    inner: Arc<RwLock<HashMap<ServicePlatform, CredentialStatus>>>,
    fingerprints: Arc<RwLock<HashMap<ServicePlatform, String>>>,
    emitter: Arc<dyn EventEmitter>,
}

impl CredentialsHealth {
    pub fn new(emitter: Arc<dyn EventEmitter>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            fingerprints: Arc::new(RwLock::new(HashMap::new())),
            emitter,
        }
    }

    pub async fn get(&self, platform: ServicePlatform) -> Option<CredentialStatus> {
        self.inner.read().await.get(&platform).cloned()
    }

    pub async fn set(&self, platform: ServicePlatform, status: CredentialStatus) {
        let mut guard = self.inner.write().await;
        let changed = guard
            .get(&platform)
            .map(|prev| prev != &status)
            .unwrap_or(true);
        guard.insert(platform, status.clone());
        drop(guard);
        if changed {
            let fingerprint = self.fingerprints.read().await.get(&platform).cloned();
            self.emitter
                .emit_credential_status_changed(&CredentialStatusChangedEvent {
                    platform: platform.as_str().to_string(),
                    status,
                    fingerprint,
                });
        }
    }

    async fn record_fingerprint(&self, platform: ServicePlatform, settings: &Settings) {
        let mut guard = self.fingerprints.write().await;
        match credential_fingerprint(platform, settings) {
            Some(fp) => {
                guard.insert(platform, fp);
            }
            None => {
                guard.remove(&platform);
            }
        }
    }

    /// Report a failed call so a stale credential is re-checked. Only `Auth` means the
    /// stored credential went bad — every other error passes through untouched, so
    /// callers can hand the whole result over without classifying it themselves.
    pub async fn note_auth_failure(
        &self,
        platform: ServicePlatform,
        error: &crate::errors::MhError,
    ) {
        if matches!(error, crate::errors::MhError::Auth(_)) {
            self.mark_expired(platform, error.to_string()).await;
        }
    }

    pub async fn mark_expired(&self, platform: ServicePlatform, _msg: String) {
        let now = chrono::Utc::now().timestamp();
        self.set(platform, CredentialStatus::Expired { since: Some(now) })
            .await;
    }

    pub async fn snapshot(&self) -> CredentialsHealthSnapshot {
        let guard = self.inner.read().await;
        let services = guard
            .iter()
            .map(|(p, s)| (p.as_str().to_string(), s.clone()))
            .collect();
        drop(guard);
        let fingerprints = self
            .fingerprints
            .read()
            .await
            .iter()
            .map(|(p, f)| (p.as_str().to_string(), f.clone()))
            .collect();
        CredentialsHealthSnapshot {
            services,
            fingerprints,
            generated_at: chrono::Utc::now().timestamp(),
        }
    }

    pub async fn sweep_cookie_only(&self, settings: &Settings, librespot: LibrespotHandle<'_>) {
        self.sweep(ServicePlatform::COOKIE_BACKED, settings, None, librespot)
            .await;
    }

    pub async fn sweep_all(
        &self,
        settings_arc: &Arc<RwLock<Settings>>,
        librespot: LibrespotHandle<'_>,
    ) {
        let settings = settings_arc.read().await.clone();

        self.sweep_cookie_only(&settings, librespot).await;
        self.sweep(
            ServicePlatform::TOKEN_BACKED,
            &settings,
            Some(settings_arc),
            librespot,
        )
        .await;
    }

    /// Fingerprint, check and record a set of platforms concurrently.
    ///
    /// Both sweeps used to spell their platform list out three times over — once to
    /// fingerprint, once as a hardcoded `join!` tuple, once as a run of `set` calls.
    /// The tuples were the dangerous form: leaving a service out of one still compiled.
    async fn sweep(
        &self,
        platforms: &[ServicePlatform],
        settings: &Settings,
        settings_arc: Option<&Arc<RwLock<Settings>>>,
        librespot: LibrespotHandle<'_>,
    ) {
        for platform in platforms {
            self.record_fingerprint(*platform, settings).await;
        }
        let checked = futures_util::future::join_all(platforms.iter().map(|platform| async move {
            (
                *platform,
                check_one(*platform, settings, settings_arc, librespot).await,
            )
        }))
        .await;
        for (platform, status) in checked {
            self.set(platform, status).await;
        }
    }

    pub async fn recheck(
        &self,
        platform: ServicePlatform,
        settings_arc: &Arc<RwLock<Settings>>,
        librespot: LibrespotHandle<'_>,
    ) {
        let settings = settings_arc.read().await.clone();
        self.record_fingerprint(platform, &settings).await;
        let status = check_one(platform, &settings, Some(settings_arc), librespot).await;
        self.set(platform, status).await;
    }
}

/// The one place that says which checker belongs to which service.
async fn check_one(
    platform: ServicePlatform,
    settings: &Settings,
    settings_arc: Option<&Arc<RwLock<Settings>>>,
    librespot: LibrespotHandle<'_>,
) -> CredentialStatus {
    match platform {
        ServicePlatform::Spotify => check_spotify(settings, librespot).await,
        ServicePlatform::YtMusic => check_ytmusic(settings).await,
        ServicePlatform::AppleMusic => check_apple_music(settings).await,
        ServicePlatform::Tidal => check_tidal(settings, settings_arc.cloned()).await,
        ServicePlatform::Qobuz => check_qobuz(settings).await,
        ServicePlatform::Deezer => check_deezer(settings).await,
        ServicePlatform::Youtube => check_youtube(settings).await,
    }
}

type LibrespotHandle<'a> =
    Option<&'a Arc<RwLock<crate::services::spotify::session::LibrespotService>>>;

async fn check_spotify(
    settings: &Settings,
    librespot: Option<&Arc<RwLock<crate::services::spotify::session::LibrespotService>>>,
) -> CredentialStatus {
    if settings.spotify_cookies_path.trim().is_empty() {
        return CredentialStatus::NotConfigured;
    }
    let path = PathBuf::from(&settings.spotify_cookies_path);
    if !path.exists() {
        return CredentialStatus::NotConfigured;
    }
    let Some(librespot) = librespot else {
        return CredentialStatus::Ok { expires_at: None };
    };
    match librespot.write().await.probe_session().await {
        Ok(true) => CredentialStatus::Ok { expires_at: None },
        Ok(false) => CredentialStatus::Expired { since: None },
        Err(e) => CredentialStatus::Unknown {
            last_check: chrono::Utc::now().timestamp(),
            error: e.to_string(),
        },
    }
}

async fn check_ytmusic(settings: &Settings) -> CredentialStatus {
    if settings.cookies.trim().is_empty() {
        return CredentialStatus::NotConfigured;
    }
    let path = PathBuf::from(&settings.cookies);
    if !path.exists() {
        return CredentialStatus::NotConfigured;
    }
    let primary = netscape::cookie_expiry(&path, "SAPISID", Some("youtube.com"));
    let secure = netscape::cookie_expiry(&path, "__Secure-3PAPISID", Some("youtube.com"));
    let pick = match (primary, secure) {
        (None, None) => return CredentialStatus::Expired { since: None },
        (Some(a), Some(b)) => match (a, b) {
            (Some(x), Some(y)) => Some(x.max(y)),
            (Some(x), None) | (None, Some(x)) => Some(x),
            (None, None) => None,
        },
        (Some(a), None) | (None, Some(a)) => a,
    };
    let date_status = CredentialStatus::from_expiry(pick);
    if matches!(date_status, CredentialStatus::Expired { .. }) {
        return date_status;
    }

    match crate::services::ytmusic::library::YtMusicLibrary::from_settings(settings) {
        Err(_) => CredentialStatus::Expired { since: None },
        Ok(lib) => match lib.probe_session().await {
            Ok(true) => CredentialStatus::Ok { expires_at: None },
            Ok(false) => CredentialStatus::Expired { since: None },
            Err(e) => CredentialStatus::Unknown {
                last_check: chrono::Utc::now().timestamp(),
                error: e.to_string(),
            },
        },
    }
}

async fn check_youtube(settings: &Settings) -> CredentialStatus {
    check_ytmusic(settings).await
}

async fn check_apple_music(settings: &Settings) -> CredentialStatus {
    if settings.apple_cookies_path.trim().is_empty() {
        return CredentialStatus::NotConfigured;
    }
    let path = PathBuf::from(&settings.apple_cookies_path);
    if !path.exists() {
        return CredentialStatus::NotConfigured;
    }
    match crate::services::apple_music::library::AppleMusicLibrary::from_settings(settings) {
        Err(_) => CredentialStatus::Expired { since: None },
        Ok(lib) => match lib.probe_session().await {
            Ok(true) => CredentialStatus::Ok { expires_at: None },
            Ok(false) => CredentialStatus::Expired { since: None },
            Err(e) => CredentialStatus::Unknown {
                last_check: chrono::Utc::now().timestamp(),
                error: e.to_string(),
            },
        },
    }
}

async fn check_tidal(
    settings: &Settings,
    settings_arc: Option<Arc<RwLock<Settings>>>,
) -> CredentialStatus {
    if settings.tidal_access_token.trim().is_empty() {
        return CredentialStatus::NotConfigured;
    }
    let expires_at = settings
        .tidal_token_expiry
        .parse::<f64>()
        .ok()
        .map(|n| n as i64)
        .filter(|n| *n > 0);
    let now = chrono::Utc::now().timestamp();

    let mut refreshed_ok = false;
    if let (Some(exp), Some(arc)) = (expires_at, settings_arc.as_ref()) {
        if exp - now <= TIDAL_REFRESH_WINDOW_SECS && !settings.tidal_refresh_token.trim().is_empty()
        {
            refreshed_ok = crate::services::tidal::library::refresh_token_standalone(
                &settings.tidal_refresh_token,
                arc,
            )
            .await
            .is_ok();
        }
    }

    let access_expired = expires_at.map(|ts| ts <= now).unwrap_or(false);
    if !settings.tidal_refresh_token.trim().is_empty() && (!access_expired || refreshed_ok) {
        return CredentialStatus::Ok { expires_at: None };
    }

    CredentialStatus::from_expiry(expires_at)
}

async fn check_qobuz(settings: &Settings) -> CredentialStatus {
    if settings.qobuz_password_or_token.trim().is_empty() {
        return CredentialStatus::NotConfigured;
    }
    let Some(app_id) = crate::services::qobuz::app_credentials::configured_app_id(settings) else {
        return CredentialStatus::Unknown {
            last_check: chrono::Utc::now().timestamp(),
            error: "Qobuz app credentials not resolved yet".into(),
        };
    };
    let client = match crate::http_client::build_probe_client() {
        Ok(c) => c,
        Err(e) => {
            return CredentialStatus::Unknown {
                last_check: chrono::Utc::now().timestamp(),
                error: e.to_string(),
            }
        }
    };
    let url = format!("https://www.qobuz.com/api.json/0.2/user/get?app_id={app_id}");
    let res = client
        .get(&url)
        .header("X-App-Id", &app_id)
        .header("X-User-Auth-Token", &settings.qobuz_password_or_token)
        .header("accept", "application/json")
        .send()
        .await;
    match res {
        Ok(r) if r.status().is_success() => CredentialStatus::Ok { expires_at: None },
        Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => {
            CredentialStatus::Unknown {
                last_check: chrono::Utc::now().timestamp(),
                error: format!("app_id {app_id} rejected this token — re-resolving on next use"),
            }
        }
        Ok(r) => CredentialStatus::Unknown {
            last_check: chrono::Utc::now().timestamp(),
            error: format!("HTTP {}", r.status()),
        },
        Err(e) => CredentialStatus::Unknown {
            last_check: chrono::Utc::now().timestamp(),
            error: e.to_string(),
        },
    }
}

async fn check_deezer(settings: &Settings) -> CredentialStatus {
    if settings.deezer_arl.trim().is_empty() {
        return CredentialStatus::NotConfigured;
    }
    let client = match crate::http_client::build_probe_client() {
        Ok(c) => c,
        Err(e) => {
            return CredentialStatus::Unknown {
                last_check: chrono::Utc::now().timestamp(),
                error: e.to_string(),
            }
        }
    };
    let url = "https://www.deezer.com/ajax/gw-light.php?method=deezer.getUserData&input=3&api_version=1.0&api_token=null";
    let res = client
        .post(url)
        .header("cookie", format!("arl={}", settings.deezer_arl))
        .header("content-type", "application/json")
        .header(
            "user-agent",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36",
        )
        .body("{}")
        .send()
        .await;
    let res = match res {
        Ok(r) => r,
        Err(e) => {
            return CredentialStatus::Unknown {
                last_check: chrono::Utc::now().timestamp(),
                error: e.to_string(),
            }
        }
    };
    let v: serde_json::Value = match res.json().await {
        Ok(v) => v,
        Err(e) => {
            return CredentialStatus::Unknown {
                last_check: chrono::Utc::now().timestamp(),
                error: e.to_string(),
            }
        }
    };
    let user_id = v
        .pointer("/results/USER/USER_ID")
        .and_then(|x| {
            x.as_u64()
                .map(|u| u.to_string())
                .or_else(|| x.as_str().map(str::to_string))
        })
        .unwrap_or_default();
    if user_id.is_empty() || user_id == "0" {
        CredentialStatus::Expired { since: None }
    } else {
        CredentialStatus::Ok { expires_at: None }
    }
}

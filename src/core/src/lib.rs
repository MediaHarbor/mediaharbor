pub mod defaults;
pub mod errors;
pub mod http_client;
pub mod ipc_contract;
pub mod logger;
pub mod orpheus;
pub mod sandbox;
pub mod settings;
pub mod streaming_server;
pub mod subprocess;
pub mod update_checker;
pub mod venv_manager;

pub mod downloads;
pub mod drm;
pub mod installers;
pub mod media;
pub mod services;

pub use defaults::Settings;
pub use errors::{MhError, MhResult};
pub use logger::{LogEmitter, Logger, NoopEmitter};

use std::sync::Arc;

use crate::services::common::lyrics::{embedded_lyrics, found_to_response, sidecar_lyrics};

pub trait EventEmitter: Send + Sync {
    fn emit_log(&self, entry: &ipc_contract::BackendLogEvent);
    fn emit_download_info(&self, event: &ipc_contract::DownloadInfoEvent);
    fn emit_progress(&self, event: &ipc_contract::DownloadProgressEvent);
    fn emit_download_summary(&self, event: &ipc_contract::DownloadSummaryEvent) {
        let _ = event;
    }
    fn emit_stream_ready(&self, event: &ipc_contract::StreamReadyEvent);
    fn emit_install_progress(&self, event: &ipc_contract::InstallationProgressEvent);
    fn emit_app_error(&self, event: &ipc_contract::AppErrorEvent);
    fn emit_stdin_prompt(&self, event: &ipc_contract::ProcessStdinPromptEvent);
}

pub struct NoopEventEmitter;
impl EventEmitter for NoopEventEmitter {
    fn emit_log(&self, _: &ipc_contract::BackendLogEvent) {}
    fn emit_download_info(&self, _: &ipc_contract::DownloadInfoEvent) {}
    fn emit_progress(&self, _: &ipc_contract::DownloadProgressEvent) {}
    fn emit_stream_ready(&self, _: &ipc_contract::StreamReadyEvent) {}
    fn emit_install_progress(&self, _: &ipc_contract::InstallationProgressEvent) {}
    fn emit_app_error(&self, _: &ipc_contract::AppErrorEvent) {}
    fn emit_stdin_prompt(&self, _: &ipc_contract::ProcessStdinPromptEvent) {}
}

use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use dashmap::DashMap;
use tokio::sync::RwLock;

pub struct BackendState {
    pub settings: Arc<RwLock<Settings>>,
    pub active_downloads: Arc<DashMap<u64, Arc<AtomicBool>>>,
    pub stdin_senders: Arc<DashMap<u64, tokio::sync::mpsc::Sender<String>>>,
    pub streaming_server: Option<streaming_server::StreamingServer>,
    pub librespot: Arc<RwLock<crate::services::spotify::session::LibrespotService>>,
    pub license_limiter: std::sync::Arc<crate::services::spotify::rate_limit::LicenseRateLimiter>,
    pub apple_music: Arc<RwLock<crate::services::apple_music::playback::AppleMusicService>>,
    pub credentials: apis::credentials::ApiCredentials,
    pub user_data: PathBuf,
    pub logger: Logger,
    pub emitter: Arc<dyn EventEmitter>,
    pub yt_stream_cache: Arc<crate::services::youtube::stream::YtAudioStreamCache>,
    pub qobuz_client_cache: Arc<RwLock<Option<crate::services::qobuz::client::QobuzClient>>>,
    pub spotify_stream_memo: Arc<crate::services::spotify::session::SpotifyStreamMemo>,
}

impl BackendState {
    pub async fn init(
        user_data: impl Into<PathBuf>,
        emitter: Arc<dyn EventEmitter>,
    ) -> MhResult<Self> {
        let user_data = user_data.into();
        tokio::fs::create_dir_all(&user_data).await?;

        let log_emitter = Arc::new(LogEventBridge(emitter.clone()));
        let logger = Logger::new(&user_data, log_emitter);

        let (loaded, streaming_server) = tokio::join!(
            settings::load_settings(&user_data),
            streaming_server::StreamingServer::start(),
        );

        logger.info("system", "Settings loaded");

        let streaming_server = streaming_server.ok();
        if let Some(ref srv) = streaming_server {
            logger.info("system", &format!("Streaming server on port {}", srv.port));
        }


        let license_limiter =
            std::sync::Arc::new(crate::services::spotify::rate_limit::LicenseRateLimiter::new());
        {
            let em = emitter.clone();
            license_limiter.set_notifier(Box::new(move |ev| {
                em.emit_log(&ipc_contract::BackendLogEvent::info(
                    "spotify",
                    "Spotify",
                    format!(
                        "Pacing to avoid Spotify rate limit (~{:.0}s)",
                        ev.waiting_secs
                    ),
                ));
            }));
        }

        let mut librespot = crate::services::spotify::session::LibrespotService::new();
        librespot.set_license_limiter(license_limiter.clone());
        librespot.set_emitter(emitter.clone());
        librespot.set_key_cache_path(user_data.join("spotify_widevine_keys.json"));

        let apple_music =
            crate::services::apple_music::playback::AppleMusicService::from_settings(&loaded);

        let credentials = apis::credentials::bundled();

        if !loaded.spotify_wvd_path.is_empty() {
            librespot.wvd_path = Some(loaded.spotify_wvd_path.clone());
        }
        if !loaded.spotify_cookies_path.is_empty() {
            let path = PathBuf::from(&loaded.spotify_cookies_path);
            let _ = librespot.login_from_cookies(&path).await;
        }
        let state = BackendState {
            settings: Arc::new(RwLock::new(loaded)),
            active_downloads: Arc::new(DashMap::new()),
            stdin_senders: Arc::new(DashMap::new()),
            streaming_server,
            librespot: Arc::new(RwLock::new(librespot)),
            license_limiter: license_limiter.clone(),
            apple_music: Arc::new(RwLock::new(apple_music)),
            credentials,
            user_data,
            logger: logger.clone(),
            emitter,
            yt_stream_cache: Arc::new(crate::services::youtube::stream::YtAudioStreamCache::new()),
            qobuz_client_cache: Arc::new(RwLock::new(None)),
            spotify_stream_memo: Arc::new(Default::default()),
        };

        {
            let em = state.emitter.clone();
            tokio::spawn(async move {
                if let Err(e) = venv_manager::ensure_venv(|_, _| {}).await {
                    em.emit_log(&ipc_contract::BackendLogEvent::error(
                        "mediaharbor",
                        "MediaHarbor",
                        format!("venv init failed: {e}"),
                    ));
                }
            });
        }

        Ok(state)
    }

    /// The local streaming server, or an error if it never came up. Every
    /// `play_media` arm needs it, so the message lives in one place.
    pub(crate) fn streaming_server(&self) -> MhResult<&streaming_server::StreamingServer> {
        self.streaming_server
            .as_ref()
            .ok_or_else(|| MhError::Other("Streaming server not running".into()))
    }

    pub async fn shutdown(&mut self) {
        if let Some(ref mut srv) = self.streaming_server {
            srv.stop();
        }
        self.logger.info("system", "Backend shutdown complete");
    }

    async fn cached_qobuz_client(
        &self,
        settings: &Settings,
    ) -> MhResult<crate::services::qobuz::client::QobuzClient> {
        {
            let lock = self.qobuz_client_cache.read().await;
            if let Some(ref c) = *lock {
                return Ok(c.clone());
            }
        }
        let (outcome, trail) =
            crate::services::qobuz::client::QobuzClient::authenticate_verbose(settings).await;
        for step in &trail {
            self.logger.info("qobuz", step);
        }
        let client = outcome?;

        let needs_persist = settings.qobuz_app_id != client.app_id
            || !settings
                .qobuz_secrets
                .split(',')
                .any(|s| s.trim() == client.secret);
        if needs_persist {
            let mut new_settings = self.settings.read().await.clone();
            new_settings.qobuz_app_id = client.app_id.clone();
            new_settings.qobuz_secrets = client.secret.clone();
            if let Err(e) = settings::save_settings(&new_settings, &self.user_data).await {
                self.logger.warn(
                    "qobuz",
                    &format!("Failed to persist discovered Qobuz credentials: {}", e),
                );
            } else {
                *self.settings.write().await = new_settings;
            }
        }

        *self.qobuz_client_cache.write().await = Some(client.clone());
        Ok(client)
    }
}

impl BackendState {
    pub async fn perform_search(
        &self,
        req: ipc_contract::PerformSearchRequest,
    ) -> MhResult<serde_json::Value> {
        let provider = crate::services::search_provider(req.platform)
            .ok_or_else(|| MhError::Unsupported("unknown search platform".into()))?;
        let ctx = crate::services::common::search::SearchContext::from_state(self);
        provider.search(&req, &ctx).await
    }

    pub async fn search_suggestions(
        &self,
        req: ipc_contract::SearchSuggestionsRequest,
    ) -> MhResult<Vec<String>> {
        let query = req.query.trim();
        if query.len() < 2 {
            return Ok(Vec::new());
        }
        let Some(provider) = crate::services::search_provider(req.platform) else {
            return Ok(Vec::new());
        };
        let ctx = crate::services::common::search::SearchContext::from_state(self);
        provider.suggestions(query, &ctx).await
    }
}

pub(crate) fn is_progressive_only_apple(e: &MhError) -> bool {
    e.to_string().contains("byterange")
}

impl BackendState {
    pub async fn play_media(
        &self,
        req: ipc_contract::PlayMediaRequest,
    ) -> MhResult<ipc_contract::PlayMediaResponse> {
        let settings = self.settings.read().await.clone();
        let platform = req.platform.as_str();

        if let Some(target) =
            crate::services::common::playback::PlaybackTarget::from_platform(platform)
        {
            return crate::services::playback_provider(target)
                .play(&req, &settings, self)
                .await;
        }

        if req.url.is_empty() || req.url == "null" {
            return Err(MhError::NotFound(format!(
                "No stream found for {}",
                platform
            )));
        }

        let path = std::path::Path::new(&req.url);
        if path.is_absolute() && path.exists() {
            let server = self.streaming_server()?;

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            let is_video = crate::media::file_discovery::VIDEO_FORMATS.contains(&ext.as_str());
            let mime_type: &str = match ext.as_str() {
                "mp3" => "audio/mpeg",
                "flac" => "audio/flac",
                "m4a" | "aac" => "audio/mp4",
                "opus" => "audio/ogg",
                "wav" => "audio/wav",
                "ogg" => "audio/ogg",
                "mp4" | "m4v" => "video/mp4",
                "mkv" => "video/x-matroska",
                "webm" => "video/webm",
                "mov" => "video/quicktime",
                "avi" => "video/x-msvideo",
                "flv" => "video/x-flv",
                _ => {
                    if is_video {
                        "video/mp4"
                    } else {
                        "audio/mpeg"
                    }
                }
            };

            let id = uuid::Uuid::new_v4().to_string();
            let stream_url = server.register(
                &id,
                streaming_server::StreamContent::Local {
                    path: path.to_path_buf(),
                },
                mime_type,
            );
            return Ok(ipc_contract::PlayMediaResponse::new(
                stream_url,
                "local",
                if is_video { "video" } else { "audio" },
                false,
            ));
        }

        Ok(ipc_contract::PlayMediaResponse {
            stream_url: req.url,
            platform: platform.to_string(),
            duration_sec: None,
            media_type: None,
            is_live: false,
            audio_stream_url: None,
        })
    }
}

pub(crate) fn emit_terminal(
    emitter: &Arc<dyn EventEmitter>,
    download_id: u64,
    result: &MhResult<()>,
) {
    if let Err(e) = result {
        emitter.emit_progress(&ipc_contract::DownloadProgressEvent::error(download_id, e));
    } else {
        emitter.emit_progress(&ipc_contract::DownloadProgressEvent::completed(download_id));
    }
}

/// Terminal event for a multi-track download. Always emits the per-item summary
/// first so a partial run reaches the UI as a warning rather than a clean success.
pub(crate) fn emit_batch_terminal(
    emitter: &Arc<dyn EventEmitter>,
    download_id: u64,
    service: &str,
    result: &MhResult<crate::services::common::download::BatchOutcome>,
) {
    match result {
        Ok(outcome) => {
            emitter.emit_download_summary(&outcome.to_event(download_id));
            if !outcome.failures.is_empty() {
                emitter.emit_log(&ipc_contract::BackendLogEvent::new(
                    "warning",
                    "download",
                    service,
                    outcome.failure_report(service),
                ));
            }
            emit_terminal(emitter, download_id, &Ok(()));
        }
        Err(e) => {
            emitter.emit_progress(&ipc_contract::DownloadProgressEvent::error(download_id, e))
        }
    }
}

impl BackendState {
    pub async fn start_orpheus_download(
        &self,
        req: ipc_contract::StartOrpheusDownloadRequest,
    ) -> ipc_contract::StartDownloadResponse {
        let download_id = self.next_download_id();
        let settings = self.settings.read().await.clone();
        if settings.download_location.is_empty() {
            return crate::services::common::download::DownloadContext::from_state(self)
                .emit_no_download_location(download_id);
        }
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_downloads
            .insert(download_id, cancel_flag.clone());

        let meta = ipc_contract::DownloadMetadata {
            title: req.title.clone(),
            artist: req.artist.clone(),
            album: req.album.clone(),
            thumbnail: req.thumbnail.clone(),
            platform: Some(req.module_id.clone()),
            quality: None,
        };
        self.emitter
            .emit_download_info(&ipc_contract::DownloadInfoEvent { download_id, meta });

        if !orpheus::is_orpheus_installed() {
            self.active_downloads.remove(&download_id);
            self.emitter
                .emit_progress(&ipc_contract::DownloadProgressEvent {
                download_id,
                percent: 0.0,
                speed: None,
                eta: None,
                status:
                    "error: OrpheusDL is not installed. Go to Updates → OrpheusDL and install it."
                        .into(),
                item_index: None,
                item_total: None,
                quality: None,
            });
            return ipc_contract::StartDownloadResponse::failed(
                download_id,
                "OrpheusDL not installed",
            );
        }

        if !orpheus::is_module_installed(&req.module_id) {
            self.active_downloads.remove(&download_id);
            self.emitter
                .emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 0.0,
                    speed: None,
                    eta: None,
                    status: format!(
                        "error: OrpheusDL module '{}' is not installed. Go to Updates → Modules.",
                        req.module_id
                    ),
                    item_index: None,
                    item_total: None,
                    quality: None,
                });
            return ipc_contract::StartDownloadResponse::failed(
                download_id,
                format!("Module {} not installed", req.module_id),
            );
        }

        let emitter = self.emitter.clone();
        let active = self.active_downloads.clone();
        let stdin_senders = self.stdin_senders.clone();
        let url = req.url.clone();
        let output_dir = req.output_dir.clone();
        let module_id = req.module_id.clone();

        let (stdin_tx, stdin_rx) = tokio::sync::mpsc::channel::<String>(4);
        stdin_senders.insert(download_id, stdin_tx);

        tokio::spawn(async move {
            let _ = orpheus::run_orpheus_download(
                &url,
                &output_dir,
                &module_id,
                download_id,
                &settings,
                cancel_flag,
                emitter,
                stdin_rx,
            )
            .await;
            active.remove(&download_id);
            stdin_senders.remove(&download_id);
        });

        ipc_contract::StartDownloadResponse::ok(download_id)
    }
}

impl BackendState {
    /// Probes a configured wrapper daemon, optionally signing in. Lets the settings
    /// screen validate the daemon and credentials without starting a download.
    pub async fn probe_apple_wrapper(
        &self,
        req: ipc_contract::WrapperProbeRequest,
    ) -> ipc_contract::WrapperProbeResponse {
        use crate::services::apple_music::wrapper::{LoginOutcome, WrapperClient, WrapperConfig};

        let mut out = ipc_contract::WrapperProbeResponse {
            reachable: false,
            authenticated: false,
            needs_two_factor: false,
            state: String::new(),
            playback_ready: false,
            version: String::new(),
            runtime: String::new(),
            apple_id: None,
            error: None,
        };

        let settings = self.settings.read().await.clone();
        let Some(cfg) = WrapperConfig::from_settings(&settings) else {
            out.error = Some("Turn the Wrapper section on first.".into());
            return out;
        };
        let client = match WrapperClient::new(cfg) {
            Ok(c) => c,
            Err(e) => {
                out.error = Some(e.to_string());
                return out;
            }
        };

        match client.status().await {
            Ok(status) => {
                out.reachable = true;
                out.authenticated = status.authenticated;
                out.state = status.state;
                out.playback_ready = status.playback_ready;
                out.version = status.version;
                out.runtime = status.runtime;
                out.apple_id = status.apple_id;
            }
            Err(e) => {
                out.error = Some(e.to_string());
                return out;
            }
        }

        if req.sign_out {
            match client.logout().await {
                Ok(()) => match client.status().await {
                    Ok(status) => {
                        out.authenticated = status.authenticated;
                        out.state = status.state;
                        out.playback_ready = status.playback_ready;
                        out.apple_id = status.apple_id;
                    }
                    Err(e) => out.error = Some(e.to_string()),
                },
                Err(e) => out.error = Some(e.to_string()),
            }
            return out;
        }

        if !req.sign_in || out.authenticated {
            return out;
        }

        let outcome = match req.code.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            Some(code) => client.submit_2fa(code).await,
            None => {
                if settings.apple_wrapper_email.trim().is_empty()
                    || settings.apple_wrapper_password.is_empty()
                {
                    out.error = Some("Enter an Apple ID and password first.".into());
                    return out;
                }
                client
                    .login(
                        &settings.apple_wrapper_email,
                        &settings.apple_wrapper_password,
                    )
                    .await
            }
        };

        match outcome {
            Ok(LoginOutcome::Authenticated) => out.authenticated = true,
            Ok(LoginOutcome::Needs2fa) => out.needs_two_factor = true,
            Ok(LoginOutcome::Failed(reason)) => out.error = Some(reason),
            Err(e) => out.error = Some(e.to_string()),
        }

        if let Ok(status) = client.status().await {
            out.authenticated = status.authenticated;
            out.state = status.state;
            out.playback_ready = status.playback_ready;
            out.apple_id = status.apple_id;
        }
        out
    }

    pub async fn send_process_stdin(
        &self,
        req: ipc_contract::SendProcessStdinRequest,
    ) -> ipc_contract::SendProcessStdinResponse {
        if let Some(tx) = self.stdin_senders.get(&req.download_id) {
            let success = tx.send(req.input).await.is_ok();
            ipc_contract::SendProcessStdinResponse { success }
        } else {
            ipc_contract::SendProcessStdinResponse { success: false }
        }
    }

    async fn authenticate_tidal(
        &self,
        settings: &Settings,
    ) -> MhResult<crate::services::tidal::client::TidalClient> {
        crate::services::tidal::client::TidalClient::authenticate_and_persist(
            settings,
            &self.settings,
            &self.user_data,
        )
        .await
    }

    pub async fn get_lyrics(
        &self,
        req: ipc_contract::GetLyricsRequest,
    ) -> MhResult<ipc_contract::GetLyricsResponse> {
        let settings = self.settings.read().await.clone();

        let empty = || ipc_contract::GetLyricsResponse {
            synced: None,
            plain: None,
            word_synced: None,
        };

        for source in crate::services::common::lyrics::SOURCE_ORDER {
            let found = match *source {
                "sidecar" => sidecar_lyrics(std::path::Path::new(&req.url))
                    .await
                    .map(found_to_response),
                "tags" => embedded_lyrics(std::path::Path::new(&req.url))
                    .await
                    .map(found_to_response),
                "service" => {
                    let r = self.service_lyrics(&req, &settings).await;
                    (!response_is_empty(&r)).then_some(r)
                }
                "community" => crate::services::common::lyrics::fetch_fallback_lyrics(
                    &req.title,
                    &req.artist,
                    req.duration,
                    &settings.deezer_arl,
                    crate::services::common::lyrics::FallbackSources::from_settings(&settings),
                )
                .await
                .map(found_to_response),
                _ => None,
            };
            if let Some(found) = found {
                if !response_is_empty(&found) {
                    return Ok(found);
                }
            }
        }

        Ok(empty())
    }

    /// The lyrics the service the track came from publishes for it.
    async fn service_lyrics(
        &self,
        req: &ipc_contract::GetLyricsRequest,
        settings: &defaults::Settings,
    ) -> ipc_contract::GetLyricsResponse {
        use crate::services::common::lyrics::empty_response;
        match services::common::playback::PlaybackTarget::from_platform(&req.platform)
            .and_then(services::lyrics_provider)
        {
            Some(provider) => provider.fetch(req, settings, self).await,
            None => empty_response(),
        }
    }
}

fn response_is_empty(r: &ipc_contract::GetLyricsResponse) -> bool {
    r.synced.is_none() && r.plain.is_none() && r.word_synced.is_none()
}

pub(crate) fn make_log_buffer() -> (
    std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    impl Fn(String) + Clone + Send + Sync + 'static,
) {
    let buf: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let buf_c = buf.clone();
    let on_log = move |msg: String| {
        if let Ok(mut v) = buf_c.lock() {
            v.push(msg);
        }
    };
    (buf, on_log)
}

impl BackendState {
    pub async fn cancel_download(&self, download_id: u64) -> bool {
        if let Some(flag) = self.active_downloads.get(&download_id) {
            flag.store(true, Ordering::Relaxed);
            self.stdin_senders.remove(&download_id);
            true
        } else {
            false
        }
    }
}

impl BackendState {
    pub async fn tidal_start_auth(&self) -> MhResult<ipc_contract::TidalStartAuthResponse> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        use sha2::{Digest, Sha256};

        let mut code_verifier_bytes = [0u8; 32];
        getrandom_bytes(&mut code_verifier_bytes)?;
        let code_verifier = URL_SAFE_NO_PAD.encode(code_verifier_bytes);

        let mut hasher = Sha256::new();
        hasher.update(code_verifier.as_bytes());
        let code_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

        let qs = format!(
            "response_type=code&redirect_uri={}&client_id=6BDSRdpK9hqEBTgU\
             &scope=r_usr%2Bw_usr%2Bw_sub&code_challenge_method=S256\
             &code_challenge={}&appMode=android&lang=en_US",
            url_encode("https://tidal.com/android/login/auth"),
            url_encode(&code_challenge)
        );
        let auth_url = format!("https://login.tidal.com/authorize?{}", qs);

        Ok(ipc_contract::TidalStartAuthResponse {
            code_verifier,
            auth_url,
        })
    }

    pub async fn tidal_exchange_code(
        &self,
        req: ipc_contract::TidalExchangeCodeRequest,
    ) -> MhResult<ipc_contract::TidalExchangeCodeResponse> {
        let parsed = url::Url::parse(&req.redirect_url)?;
        let code = parsed
            .query_pairs()
            .find(|(k, _)| k == "code")
            .map(|(_, v)| v.to_string())
            .ok_or_else(|| MhError::Auth("No auth code in redirect URL".into()))?;

        let body = format!(
            "code={}&client_id=6BDSRdpK9hqEBTgU&grant_type=authorization_code\
             &redirect_uri={}&scope=r_usr%2Bw_usr%2Bw_sub&code_verifier={}",
            url_encode(&code),
            url_encode("https://tidal.com/android/login/auth"),
            url_encode(&req.code_verifier)
        );

        let client = http_client::build_client()?;
        let resp = client
            .post("https://auth.tidal.com/v1/oauth2/token")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(MhError::Network)?;

        let status = resp.status();
        let body_text = resp.text().await.map_err(MhError::Network)?;
        if !status.is_success() {
            return Err(MhError::Auth(format!(
                "Tidal token exchange failed ({}): {}",
                status, body_text
            )));
        }
        let json: serde_json::Value = serde_json::from_str(&body_text)
            .map_err(|e| MhError::Auth(format!("Tidal response parse error: {}", e)))?;
        let access_token = json["access_token"]
            .as_str()
            .ok_or_else(|| MhError::Auth("No access_token in Tidal response".into()))?
            .to_string();
        let refresh_token = json["refresh_token"].as_str().unwrap_or("").to_string();
        let expires_in = json["expires_in"].as_u64().unwrap_or(86400);

        let user_resp = client
            .get("https://openapi.tidal.com/v2/users/me")
            .bearer_auth(&access_token)
            .send()
            .await?;
        let user_json: serde_json::Value =
            user_resp.json().await.unwrap_or(serde_json::json!({}));
        let user_id = user_json["data"]["id"].as_str().unwrap_or("").to_string();
        let country_code = user_json["data"]["attributes"]["country"]
            .as_str()
            .unwrap_or("US")
            .to_string();

        Ok(ipc_contract::TidalExchangeCodeResponse {
            access_token,
            refresh_token,
            expires_in,
            user_id,
            country_code,
        })
    }
}

use std::sync::LazyLock;

static SPOTIFY_ID_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?:spotify\.com/(?:track|episode|album|artist|playlist)/|spotify:(?:track|episode|album|artist|playlist):)([a-zA-Z0-9]+)",
    )
    .unwrap()
});
static TIDAL_TRACK_ID_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"tidal\.com/(?:browse/)?(?:track|album|video)/(\d+)").unwrap()
});
pub(crate) fn extract_spotify_id(url: &str) -> Option<String> {
    SPOTIFY_ID_RE
        .captures(url)?
        .get(1)
        .map(|m| m.as_str().to_string())
}

pub(crate) fn extract_tidal_track_id(url: &str) -> Option<String> {
    TIDAL_TRACK_ID_RE
        .captures(url)?
        .get(1)
        .map(|m| m.as_str().to_string())
}

fn getrandom_bytes(buf: &mut [u8]) -> MhResult<()> {
    getrandom::fill(buf).map_err(|e| MhError::Io(std::io::Error::other(e)))
}

fn url_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

struct LogEventBridge(Arc<dyn EventEmitter>);

impl logger::LogEmitter for LogEventBridge {
    fn emit(&self, entry: &logger::LogEntry) {
        let msg_len = entry.message.len().min(120);
        self.0.emit_log(&ipc_contract::BackendLogEvent {
            level: entry.level.clone(),
            source: entry.source.clone(),
            title: format!("[{}] {}", entry.source, &entry.message[..msg_len]),
            message: entry.message.clone(),
            timestamp: entry.timestamp.clone(),
        });
    }
}

impl BackendState {
    pub async fn get_settings(&self) -> ipc_contract::GetSettingsResponse {
        ipc_contract::GetSettingsResponse {
            settings: self.settings.read().await.clone(),
        }
    }

    pub async fn set_settings(
        &self,
        req: ipc_contract::SetSettingsRequest,
    ) -> ipc_contract::SetSettingsResponse {
        if let Err(e) = settings::save_settings(&req.settings, &self.user_data).await {
            return ipc_contract::SetSettingsResponse {
                success: false,
                error: Some(e.to_string()),
            };
        }

        {
            let old = self.settings.read().await;
            if old.qobuz_email_or_userid != req.settings.qobuz_email_or_userid
                || old.qobuz_password_or_token != req.settings.qobuz_password_or_token
            {
                *self.qobuz_client_cache.write().await = None;
            }
        }

        {
            let mut s = self.settings.write().await;
            *s = req.settings.clone();
        }

        {
            let mut am = self.apple_music.write().await;
            *am = crate::services::apple_music::playback::AppleMusicService::from_settings(
                &req.settings,
            );
        }

        {
            let mut svc = self.librespot.write().await;
            svc.wvd_path = if req.settings.spotify_wvd_path.is_empty() {
                None
            } else {
                Some(req.settings.spotify_wvd_path.clone())
            };
            if !req.settings.spotify_cookies_path.is_empty() {
                let path = PathBuf::from(&req.settings.spotify_cookies_path);
                let _ = svc.login_from_cookies(&path).await;
            }
        }


        ipc_contract::SetSettingsResponse {
            success: true,
            error: None,
        }
    }
}

impl BackendState {
    pub async fn pause_media(&self) -> ipc_contract::PauseMediaResponse {
        ipc_contract::PauseMediaResponse { success: true }
    }
}

impl BackendState {
    pub async fn spotify_oauth_login(&self) -> MhResult<ipc_contract::SpotifyOAuthLoginResponse> {
        let mut svc = self.librespot.write().await;
        let cookies_path = {
            let s = self.settings.read().await;
            PathBuf::from(&s.spotify_cookies_path)
        };
        let profile = svc.login_from_cookies(&cookies_path).await?;
        Ok(ipc_contract::SpotifyOAuthLoginResponse { profile })
    }

    pub async fn spotify_oauth_logout(&self) -> ipc_contract::SpotifyOAuthLogoutResponse {
        let mut svc = self.librespot.write().await;
        svc.logout();
        ipc_contract::SpotifyOAuthLogoutResponse { success: true }
    }

    pub async fn spotify_oauth_status(&self) -> ipc_contract::SpotifyOAuthStatusResponse {
        let svc = self.librespot.read().await;
        let logged_in = svc.is_logged_in();
        let profile = if logged_in {
            svc.cached_profile()
        } else {
            None
        };
        ipc_contract::SpotifyOAuthStatusResponse { logged_in, profile }
    }

    pub async fn spotify_get_token(&self) -> ipc_contract::SpotifyGetTokenResponse {
        let svc = self.librespot.read().await;
        let token = svc.cached_access_token();
        ipc_contract::SpotifyGetTokenResponse { token }
    }

    pub async fn clear_spotify_credentials(&self) -> MhResult<()> {
        let config_path = settings::spotify_config_path(&self.user_data);
        if config_path.exists() {
            tokio::fs::remove_file(&config_path).await?;
        }
        {
            let mut svc = self.librespot.write().await;
            svc.logout();
        }
        {
            let mut s = self.settings.write().await;
            s.spotify_cookies_path = String::new();
        }
        Ok(())
    }
}

impl BackendState {
    pub async fn get_album_details(
        &self,
        req: ipc_contract::GetAlbumDetailsRequest,
    ) -> MhResult<ipc_contract::MediaDetailsResponse> {
        let settings = self.settings.read().await.clone();
        let album_id = &req.album_id;
        let data = match req.platform {
            ipc_contract::SearchPlatform::Spotify => {
                let client_id = if settings.spotify_client_id.is_empty() {
                    self.credentials.spotify_client_id.clone()
                } else {
                    settings.spotify_client_id.clone()
                };
                let client_secret = if settings.spotify_client_secret.is_empty() {
                    self.credentials.spotify_client_secret.clone()
                } else {
                    settings.spotify_client_secret.clone()
                };
                let client = apis::spotify_api::SpotifyApiClient::new(client_id, client_secret)?;
                if album_id.starts_with("audiobook::") {
                    let ab_id = &album_id["audiobook::".len()..];
                    client.get_audiobook_chapters(ab_id).await?
                } else {
                    client.get_album_tracks(album_id).await?
                }
            }
            ipc_contract::SearchPlatform::Tidal => {
                let client = self.authenticate_tidal(&settings).await?;
                client.get_album_details_json(album_id).await?
            }
            ipc_contract::SearchPlatform::Deezer => {
                let client = apis::deezer_api::DeezerApiClient::new()?;
                client.get_track_list(album_id, "album").await?
            }
            ipc_contract::SearchPlatform::Qobuz => {
                let client = if settings.qobuz_app_id.is_empty() {
                    apis::qobuz_api::QobuzApiClient::with_bundled_credentials()?
                } else {
                    apis::qobuz_api::QobuzApiClient::new(
                        settings.qobuz_app_id.clone(),
                        settings.qobuz_password_or_token.clone(),
                        settings.qobuz_app_secret.clone(),
                    )?
                };
                let raw = client.get_track_list(album_id, "album").await?;
                let tracks = raw["tracks"]["items"].clone();
                serde_json::json!({
                    "tracks": if tracks.is_array() { tracks } else { serde_json::json!([]) },
                    "thumbnail": raw["image"]["large"],
                    "album": {
                        "title": raw["title"],
                        "artist": raw["artist"]["name"],
                        "releaseDate": raw["release_date_original"],
                        "coverUrl": raw["image"]["large"],
                    }
                })
            }
            ipc_contract::SearchPlatform::YoutubeMusic => {
                let client = apis::ytmusic_search_api::YtMusicClient::init().await?;
                let (tracks, album_title, cover_url) = client.get_album_details(album_id).await?;
                serde_json::json!({
                    "tracks": tracks,
                    "album": {
                        "title": album_title,
                        "coverUrl": cover_url,
                    },
                    "url": format!("https://music.youtube.com/browse/{}", album_id),
                })
            }
            ipc_contract::SearchPlatform::AppleMusic => {
                let client = apis::apple_music_api::AppleMusicApiClient::new(None)?;
                client.get_album_tracks(album_id, "us").await?
            }
            ipc_contract::SearchPlatform::Youtube => {
                return Err(MhError::Unsupported("Album details not supported for YouTube".into()));
            }
        };
        Ok(ipc_contract::MediaDetailsResponse { data })
    }

    pub async fn get_playlist_details(
        &self,
        req: ipc_contract::GetPlaylistDetailsRequest,
    ) -> MhResult<ipc_contract::MediaDetailsResponse> {
        let settings = self.settings.read().await.clone();
        let playlist_id = &req.playlist_id;
        let data = match req.platform {
            ipc_contract::SearchPlatform::Spotify => {
                let client_id = if settings.spotify_client_id.is_empty() {
                    self.credentials.spotify_client_id.clone()
                } else {
                    settings.spotify_client_id.clone()
                };
                let client_secret = if settings.spotify_client_secret.is_empty() {
                    self.credentials.spotify_client_secret.clone()
                } else {
                    settings.spotify_client_secret.clone()
                };
                let client = apis::spotify_api::SpotifyApiClient::new(client_id, client_secret)?;
                client.get_playlist_tracks(playlist_id).await?
            }
            ipc_contract::SearchPlatform::Deezer => {
                let client = apis::deezer_api::DeezerApiClient::new()?;
                client.get_track_list(playlist_id, "playlist").await?
            }
            ipc_contract::SearchPlatform::Qobuz => {
                let client = if settings.qobuz_app_id.is_empty() {
                    apis::qobuz_api::QobuzApiClient::with_bundled_credentials()?
                } else {
                    apis::qobuz_api::QobuzApiClient::new(
                        settings.qobuz_app_id.clone(),
                        settings.qobuz_password_or_token.clone(),
                        settings.qobuz_app_secret.clone(),
                    )?
                };
                let raw = client.get_track_list(playlist_id, "playlist").await?;
                let tracks = raw["tracks"]["items"].clone();
                serde_json::json!({
                    "tracks": if tracks.is_array() { tracks } else { serde_json::json!([]) },
                    "thumbnail": raw["image_rectangle_mini"].clone(),
                    "playlist": {
                        "title": raw["name"],
                        "creator": raw["owner"]["name"],
                        "coverUrl": raw["image_rectangle_mini"],
                    }
                })
            }
            ipc_contract::SearchPlatform::YoutubeMusic => {
                let client = apis::ytmusic_search_api::YtMusicClient::init().await?;
                if playlist_id.starts_with("podcast::") {
                    let browse_id = &playlist_id["podcast::".len()..];
                    let (episodes, title, cover_url) = client.get_podcast_details(browse_id).await?;
                    serde_json::json!({
                        "tracks": episodes,
                        "playlist": { "title": title, "coverUrl": cover_url },
                        "url": format!("https://music.youtube.com/browse/{}", browse_id),
                    })
                } else {
                    let (tracks, title, cover_url) = client.get_playlist_details(playlist_id).await?;
                    let list_id = if playlist_id.starts_with("VL") {
                        playlist_id[2..].to_string()
                    } else {
                        playlist_id.to_string()
                    };
                    serde_json::json!({
                        "tracks": tracks,
                        "playlist": { "title": title, "coverUrl": cover_url },
                        "url": format!("https://music.youtube.com/playlist?list={}", list_id),
                    })
                }
            }
            ipc_contract::SearchPlatform::Tidal => {
                let client = self.authenticate_tidal(&settings).await?;
                client.get_playlist_details_json(playlist_id).await?
            }
            ipc_contract::SearchPlatform::AppleMusic => {
                return Err(MhError::Unsupported("Apple Music playlist details not yet implemented".into()));
            }
            ipc_contract::SearchPlatform::Youtube => {
                let yt_key = if settings.youtube_api_key.is_empty() {
                    self.credentials.youtube_api_key.clone()
                } else {
                    settings.youtube_api_key.clone()
                };
                let client = apis::yt_search_api::YtSearchClient::new(yt_key)?;
                client.get_playlist_videos(playlist_id).await?
            }
        };
        Ok(ipc_contract::MediaDetailsResponse { data })
    }

    pub async fn get_artist_details(
        &self,
        req: ipc_contract::GetArtistDetailsRequest,
    ) -> MhResult<ipc_contract::MediaDetailsResponse> {
        let settings = self.settings.read().await.clone();
        let artist_id = &req.artist_id;
        let data = match req.platform {
            ipc_contract::SearchPlatform::Spotify => {
                let client_id = if settings.spotify_client_id.is_empty() {
                    self.credentials.spotify_client_id.clone()
                } else {
                    settings.spotify_client_id.clone()
                };
                let client_secret = if settings.spotify_client_secret.is_empty() {
                    self.credentials.spotify_client_secret.clone()
                } else {
                    settings.spotify_client_secret.clone()
                };
                let client = apis::spotify_api::SpotifyApiClient::new(client_id, client_secret)?;
                let raw = client.get_artist_albums(artist_id).await?;
                let items = raw["items"].as_array().cloned().unwrap_or_default();
                let albums: Vec<serde_json::Value> = items.iter().map(|a| serde_json::json!({
                    "id": a["id"],
                    "title": a["name"],
                    "thumbnail": a["images"][0]["url"],
                    "releaseDate": a["release_date"],
                    "trackCount": a["total_tracks"],
                    "url": a["external_urls"]["spotify"].as_str().or_else(|| a["uri"].as_str()),
                    "explicit": a["explicit"].as_bool().unwrap_or(false),
                })).collect();
                serde_json::json!({ "albums": albums })
            }
            ipc_contract::SearchPlatform::Tidal => {
                let client_id = if settings.tidal_client_id.is_empty() {
                    self.credentials.tidal_client_id.clone()
                } else {
                    settings.tidal_client_id.clone()
                };
                let client_secret = if settings.tidal_client_secret.is_empty() {
                    self.credentials.tidal_client_secret.clone()
                } else {
                    settings.tidal_client_secret.clone()
                };
                let client = apis::tidal_api::TidalApiClient::new(client_id, client_secret)?;
                let cc = if settings.tidal_country_code.is_empty() {
                    "US"
                } else {
                    &settings.tidal_country_code
                };
                let user_tok = if settings.tidal_access_token.is_empty() {
                    None
                } else {
                    Some(settings.tidal_access_token.as_str())
                };
                let raw = client.get_artist_albums(artist_id, cc, user_tok).await?;
                let items = raw["items"].as_array().cloned().unwrap_or_default();
                let albums: Vec<serde_json::Value> = items.iter().map(|a| {
                    let album_id = a["id"].as_i64().map(|n| n.to_string())
                        .or_else(|| a["id"].as_str().map(|s| s.to_string()))
                        .unwrap_or_default();
                    let thumbnail = a["cover"].as_str().map(|c| {
                        format!(
                            "https://resources.tidal.com/images/{}/640x640.jpg",
                            c.replace('-', "/")
                        )
                    });
                    let url = format!("https://tidal.com/browse/album/{}", album_id);
                    serde_json::json!({
                        "id": album_id,
                        "title": a["title"].as_str(),
                        "thumbnail": thumbnail,
                        "releaseDate": a["releaseDate"].as_str(),
                        "trackCount": a["numberOfTracks"].as_i64(),
                        "url": url,
                        "explicit": a["explicit"].as_bool().unwrap_or(false),
                    })
                }).collect();
                serde_json::json!({ "albums": albums })
            }
            ipc_contract::SearchPlatform::Deezer => {
                let client = apis::deezer_api::DeezerApiClient::new()?;
                let raw = client.get_artist_albums(artist_id).await?;
                let items = if raw.is_array() {
                    raw.as_array().cloned().unwrap_or_default()
                } else {
                    raw["data"].as_array().cloned().unwrap_or_default()
                };
                let albums: Vec<serde_json::Value> = items.iter().map(|a| {
                    let id = a["id"].as_i64().map(|n| n.to_string())
                        .or_else(|| a["id"].as_str().map(|s| s.to_string()))
                        .unwrap_or_default();
                    let url = a["link"].as_str().map(|s| s.to_string())
                        .unwrap_or_else(|| format!("https://www.deezer.com/album/{}", id));
                    serde_json::json!({
                        "id": id,
                        "title": a["title"],
                        "thumbnail": a["cover_xl"].as_str().or_else(|| a["cover_big"].as_str()),
                        "releaseDate": a["release_date"],
                        "trackCount": a["nb_tracks"],
                        "url": url,
                        "explicit": a["explicit_lyrics"].as_i64().map(|v| v == 1).unwrap_or(false),
                    })
                }).collect();
                serde_json::json!({ "albums": albums })
            }
            ipc_contract::SearchPlatform::Qobuz => {
                let client = if settings.qobuz_app_id.is_empty() {
                    apis::qobuz_api::QobuzApiClient::with_bundled_credentials()?
                } else {
                    apis::qobuz_api::QobuzApiClient::new(
                        settings.qobuz_app_id.clone(),
                        settings.qobuz_password_or_token.clone(),
                        settings.qobuz_app_secret.clone(),
                    )?
                };
                let raw = client.get_artist_albums(artist_id).await?;
                let items = raw["items"].as_array().cloned().unwrap_or_default();
                let albums: Vec<serde_json::Value> = items.iter().map(|a| {
                    let id = a["id"].as_i64().map(|n| n.to_string())
                        .or_else(|| a["id"].as_str().map(|s| s.to_string()))
                        .unwrap_or_default();
                    let release_date = a["released_at"].as_i64()
                        .filter(|&ts| ts > 0)
                        .and_then(|ts| {
                            use std::time::{UNIX_EPOCH, Duration};
                            let secs = if ts > 9_999_999_999 { ts / 1000 } else { ts };
                            UNIX_EPOCH.checked_add(Duration::from_secs(secs as u64))
                                .map(|d| {
                                    let dt = chrono::DateTime::<chrono::Utc>::from(d);
                                    dt.format("%Y-%m-%d").to_string()
                                })
                        })
                        .or_else(|| a["release_date_original"].as_str().map(|s| s.to_string()));
                    serde_json::json!({
                        "id": id,
                        "title": a["title"],
                        "thumbnail": a["image"]["large"].as_str().or_else(|| a["image"]["small"].as_str()),
                        "releaseDate": release_date,
                        "trackCount": a["tracks_count"],
                        "url": format!("https://play.qobuz.com/album/{}", id),
                        "explicit": a["parental_warning"].as_bool().unwrap_or(false),
                        "hires": a["hires_streamable"].as_bool().unwrap_or(false),
                    })
                }).collect();
                serde_json::json!({ "albums": albums })
            }
            ipc_contract::SearchPlatform::AppleMusic => {
                let client = apis::apple_music_api::AppleMusicApiClient::new(None)?;
                let raw = client.get_artist_albums(artist_id, "us").await?;
                let items = raw.as_array().cloned().unwrap_or_default();
                let albums: Vec<serde_json::Value> = items.iter().map(|a| {
                    let id = a["collectionId"].as_i64().map(|n| n.to_string())
                        .or_else(|| a["collectionId"].as_str().map(|s| s.to_string()))
                        .unwrap_or_default();
                    let thumbnail = a["artworkUrl100"].as_str()
                        .map(|s| s.replace("100x100", "640x640"));
                    serde_json::json!({
                        "id": id,
                        "title": a["collectionName"],
                        "thumbnail": thumbnail,
                        "releaseDate": a["releaseDate"],
                        "trackCount": a["trackCount"],
                        "url": a["collectionViewUrl"],
                        "explicit": a["collectionExplicitness"].as_str() == Some("explicit"),
                    })
                }).collect();
                serde_json::json!({ "albums": albums })
            }
            ipc_contract::SearchPlatform::YoutubeMusic => {
                let client = apis::ytmusic_search_api::YtMusicClient::init().await?;
                let albums = client.get_artist_albums(artist_id).await.unwrap_or_default();
                serde_json::json!({ "albums": albums })
            }
            ipc_contract::SearchPlatform::Youtube => {
                let yt_key = if settings.youtube_api_key.is_empty() {
                    self.credentials.youtube_api_key.clone()
                } else {
                    settings.youtube_api_key.clone()
                };
                let client = apis::yt_search_api::YtSearchClient::new(yt_key)?;
                client.get_channel_uploads(artist_id).await?
            }
        };
        Ok(ipc_contract::MediaDetailsResponse { data })
    }
}
/// `start_download` pass-throughs. The `+ quality` arm backfills `meta.quality` from the
/// request's own tier when the download dialog left it empty.
macro_rules! start_downloads {
    ($( $name:ident($req:ident) => $target:ident $(+ $quality:ident)? ; )*) => {
        impl BackendState {
            $( start_downloads!(@one $name, $req, $target $(, $quality)?); )*
        }
    };
    (@one $name:ident, $req:ident, $target:ident) => {
        pub async fn $name(
            &self,
            req: ipc_contract::$req,
        ) -> ipc_contract::StartDownloadResponse {
            self.start_download(
                crate::services::DownloadTarget::$target,
                &req,
                &req.url,
                &req.meta,
            )
            .await
        }
    };
    (@one $name:ident, $req:ident, $target:ident, $quality:ident) => {
        pub async fn $name(
            &self,
            req: ipc_contract::$req,
        ) -> ipc_contract::StartDownloadResponse {
            let mut meta = req.meta.clone();
            if meta.quality.as_deref().unwrap_or("").is_empty() {
                meta.quality = req.$quality.map(|q| q.to_string());
            }
            self.start_download(
                crate::services::DownloadTarget::$target,
                &req,
                &req.url,
                &meta,
            )
            .await
        }
    };
}

start_downloads! {
    start_yt_music_download(StartYtMusicDownloadRequest) => YtMusic;
    start_yt_video_download(StartYtVideoDownloadRequest) => YtVideo;
    start_spotify_download(StartSpotifyDownloadRequest) => Spotify;
    start_apple_download(StartAppleDownloadRequest) => AppleMusic;
    start_qobuz_download(StartQobuzDownloadRequest) => Qobuz + quality;
    start_deezer_download(StartDeezerDownloadRequest) => Deezer + quality;
    start_tidal_download(StartTidalDownloadRequest) => Tidal + quality;
}

impl BackendState {
    fn next_download_id(&self) -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    /// Fills the download card before authentication or the first byte. Albums,
    /// playlists and artists used to bail out here, which is why a collection link
    /// showed a bare "Downloading…" with no cover for the whole run.
    async fn try_prefetch_pipeline_meta(
        &self,
        url: &str,
        settings: &Settings,
    ) -> Option<ipc_contract::DownloadMetadata> {
        use crate::services::common::pipeline::orchestrator::Platform as SrPlatform;
        use crate::services::common::pipeline::orchestrator::{
            detect_or_resolve, extract_platform_id,
        };

        let (url, platform, content_type) = detect_or_resolve(url).await.ok()?;
        let id = extract_platform_id(&url, platform, content_type)?;

        match platform {
            SrPlatform::Deezer => self.prefetch_deezer_meta(&id, content_type).await,
            SrPlatform::Qobuz => self.prefetch_qobuz_meta(&id, content_type, settings).await,
            SrPlatform::Tidal => self.prefetch_tidal_meta(&id, content_type, settings).await,
        }
    }

    async fn prefetch_deezer_meta(
        &self,
        id: &str,
        content_type: crate::services::common::pipeline::orchestrator::ContentType,
    ) -> Option<ipc_contract::DownloadMetadata> {
        use crate::services::common::pipeline::orchestrator::ContentType;
        let client = crate::services::deezer::api::DeezerApiClient::new().ok()?;
        let cover = |v: &serde_json::Value, keys: &[&str]| -> Option<String> {
            keys.iter()
                .find_map(|k| v[*k].as_str().filter(|s| !s.is_empty()))
                .map(String::from)
        };

        let (title, artist, album, thumbnail) = match content_type {
            ContentType::Track => {
                let d = client.get_track(id).await.ok()?;
                (
                    d["title"].as_str().map(String::from),
                    d["artist"]["name"].as_str().map(String::from),
                    d["album"]["title"].as_str().map(String::from),
                    cover(&d["album"], &["cover_xl", "cover_big", "cover_medium"]),
                )
            }
            ContentType::Album => {
                let d = client.get_album(id).await.ok()?;
                (
                    d["title"].as_str().map(String::from),
                    d["artist"]["name"].as_str().map(String::from),
                    d["title"].as_str().map(String::from),
                    cover(&d, &["cover_xl", "cover_big", "cover_medium"]),
                )
            }
            ContentType::Playlist => {
                let d = client.get_playlist(id).await.ok()?;
                (
                    d["title"].as_str().map(String::from),
                    d["creator"]["name"].as_str().map(String::from),
                    None,
                    cover(&d, &["picture_xl", "picture_big", "picture_medium"]),
                )
            }
            _ => return None,
        };

        Some(ipc_contract::DownloadMetadata {
            title,
            artist,
            album,
            thumbnail,
            platform: Some("deezer".into()),
            quality: None,
        })
    }

    async fn prefetch_qobuz_meta(
        &self,
        id: &str,
        content_type: crate::services::common::pipeline::orchestrator::ContentType,
        settings: &Settings,
    ) -> Option<ipc_contract::DownloadMetadata> {
        use crate::services::common::pipeline::orchestrator::ContentType;
        let client = match crate::services::qobuz::app_credentials::configured_pair(settings) {
            Some(pair) => crate::services::qobuz::api::QobuzApiClient::new(
                pair.app_id,
                settings.qobuz_password_or_token.clone(),
                pair.secret,
            )
            .ok()?,
            None => crate::services::qobuz::api::QobuzApiClient::with_bundled_credentials().ok()?,
        };

        let (title, artist, album, thumbnail) = match content_type {
            ContentType::Track => {
                let d = client.get_track(id).await.ok()?;
                (
                    d["title"].as_str().map(String::from),
                    d["performer"]["name"]
                        .as_str()
                        .or_else(|| d["album"]["artist"]["name"].as_str())
                        .map(String::from),
                    d["album"]["title"].as_str().map(String::from),
                    d["album"]["image"]["large"].as_str().map(String::from),
                )
            }
            ContentType::Album => {
                let d = client.get_album(id).await.ok()?;
                (
                    d["title"].as_str().map(String::from),
                    d["artist"]["name"].as_str().map(String::from),
                    d["title"].as_str().map(String::from),
                    d["image"]["large"].as_str().map(String::from),
                )
            }
            ContentType::Playlist => {
                let d = client.get_playlist(id).await.ok()?;
                (
                    d["name"].as_str().map(String::from),
                    d["owner"]["name"].as_str().map(String::from),
                    None,
                    d["images300"][0]
                        .as_str()
                        .or_else(|| d["image_rectangle"][0].as_str())
                        .map(String::from),
                )
            }
            _ => return None,
        };

        Some(ipc_contract::DownloadMetadata {
            title,
            artist,
            album,
            thumbnail,
            platform: Some("qobuz".into()),
            quality: None,
        })
    }

    async fn prefetch_tidal_meta(
        &self,
        id: &str,
        content_type: crate::services::common::pipeline::orchestrator::ContentType,
        settings: &Settings,
    ) -> Option<ipc_contract::DownloadMetadata> {
        use crate::services::common::pipeline::orchestrator::ContentType;
        if settings.tidal_access_token.is_empty() {
            return None;
        }
        let country = if settings.tidal_country_code.is_empty() {
            "US"
        } else {
            &settings.tidal_country_code
        };
        let http = http_client::build_client().ok()?;
        let path = match content_type {
            ContentType::Track => format!("tracks/{}", id),
            ContentType::Album => format!("albums/{}", id),
            ContentType::Playlist => format!("playlists/{}", id),
            _ => return None,
        };
        let fetch = |token: String| {
            let http = http.clone();
            let path = path.clone();
            let country = country.to_string();
            async move {
                http.get(format!("https://api.tidal.com/v1/{}", path))
                    .header("Authorization", format!("Bearer {}", token))
                    .query(&[("countryCode", country.as_str())])
                    .send()
                    .await
                    .ok()
            }
        };

        let mut resp = fetch(settings.tidal_access_token.clone()).await?;
        if resp.status() == 401 {
            let client = crate::services::tidal::client::TidalClient::authenticate_and_persist(
                settings,
                &self.settings,
                &self.user_data,
            )
            .await
            .ok()?;
            resp = fetch(client.access_token.clone()).await?;
        }
        if !resp.status().is_success() {
            return None;
        }
        let d: serde_json::Value = resp.json().await.ok()?;

        let art = |uuid: Option<&str>, size: &str| -> Option<String> {
            let uuid = uuid.filter(|s| !s.is_empty())?;
            Some(format!(
                "https://resources.tidal.com/images/{}/{}.jpg",
                uuid.replace('-', "/"),
                size
            ))
        };

        let (title, artist, album, thumbnail) = match content_type {
            ContentType::Track => (
                d["title"].as_str().map(String::from),
                d["artist"]["name"].as_str().map(String::from),
                d["album"]["title"].as_str().map(String::from),
                art(d["album"]["cover"].as_str(), "640x640"),
            ),
            ContentType::Album => (
                d["title"].as_str().map(String::from),
                d["artist"]["name"].as_str().map(String::from),
                d["title"].as_str().map(String::from),
                art(d["cover"].as_str(), "640x640"),
            ),
            ContentType::Playlist => (
                d["title"].as_str().map(String::from),
                d["creator"]["name"].as_str().map(String::from),
                None,
                art(d["squareImage"].as_str(), "640x640")
                    .or_else(|| art(d["image"].as_str(), "640x428")),
            ),
            _ => return None,
        };

        Some(ipc_contract::DownloadMetadata {
            title,
            artist,
            album,
            thumbnail,
            platform: Some("tidal".into()),
            quality: None,
        })
    }

    async fn try_prefetch_spotify_meta(
        &self,
        url: &str,
        settings: &Settings,
    ) -> Option<ipc_contract::DownloadMetadata> {
        if !url.contains("/track/") && !url.starts_with("spotify:track:") {
            return None;
        }
        let track_id = if let Some(rest) = url.strip_prefix("spotify:track:") {
            rest.to_string()
        } else {
            url.trim_end_matches('/')
                .rsplit('/')
                .next()?
                .split('?')
                .next()?
                .to_string()
        };
        let client_id = crate::auth::credentials::preferred(
            &settings.spotify_client_id,
            &self.credentials.spotify_client_id,
        );
        let client_secret = crate::auth::credentials::preferred(
            &settings.spotify_client_secret,
            &self.credentials.spotify_client_secret,
        );
        let client =
            crate::services::spotify::api::SpotifyApiClient::new(client_id, client_secret).ok()?;
        let data = client.get_track(&track_id).await.ok()?;
        Some(ipc_contract::DownloadMetadata {
            title: data["name"].as_str().map(String::from),
            artist: data["artists"][0]["name"].as_str().map(String::from),
            album: data["album"]["name"].as_str().map(String::from),
            thumbnail: data["album"]["images"][0]["url"].as_str().map(String::from),
            platform: Some("spotify".into()),
            quality: None,
        })
    }

    async fn try_prefetch_apple_meta(&self, url: &str) -> Option<ipc_contract::DownloadMetadata> {
        let lookup_id = if let Some(pos) = url.find("?i=") {
            url[pos + 3..].split('&').next()?.to_string()
        } else if url.contains("/album/") {
            url.trim_end_matches('/')
                .rsplit('/')
                .next()?
                .split('?')
                .next()?
                .to_string()
        } else {
            return None;
        };
        let client =
            crate::services::apple_music::api::AppleMusicApiClient::unauthenticated().ok()?;
        let data = client.lookup_by_id(&lookup_id).await.ok()?;
        if data.is_null() {
            return None;
        }
        let thumbnail = data["artworkUrl100"]
            .as_str()
            .map(|s| s.replace("100x100", "640x640"));
        Some(ipc_contract::DownloadMetadata {
            title: data["trackName"]
                .as_str()
                .or_else(|| data["collectionName"].as_str())
                .map(String::from),
            artist: data["artistName"].as_str().map(String::from),
            album: data["collectionName"].as_str().map(String::from),
            thumbnail,
            platform: Some("applemusic".into()),
            quality: None,
        })
    }

    async fn try_prefetch_ytdlp_meta(
        &self,
        url: &str,
        fallback: &ipc_contract::DownloadMetadata,
    ) -> Option<ipc_contract::DownloadMetadata> {
        let yt_dlp_cmd = downloads::yt_dlp::find_yt_dlp_command();
        let fetched = downloads::yt_dlp::prefetch_metadata(url, &yt_dlp_cmd)
            .await
            .ok()?;
        Some(ipc_contract::DownloadMetadata {
            title: if fetched.title.is_empty() {
                fallback.title.clone()
            } else {
                Some(fetched.title)
            },
            artist: if fetched.uploader.is_empty() {
                fallback.artist.clone()
            } else {
                Some(fetched.uploader)
            },
            album: fallback.album.clone(),
            thumbnail: if fetched.thumbnail.is_empty() {
                fallback.thumbnail.clone()
            } else {
                Some(fetched.thumbnail)
            },
            platform: fallback.platform.clone(),
            quality: fallback.quality.clone(),
        })
    }

    async fn resolve_download_meta(
        &self,
        target: crate::services::DownloadTarget,
        url: &str,
        settings: &Settings,
        fallback: &ipc_contract::DownloadMetadata,
    ) -> ipc_contract::DownloadMetadata {
        use crate::services::DownloadTarget as T;
        let platform_label = match target {
            T::YtMusic => "youtubemusic",
            T::YtVideo => "youtube",
            T::Spotify => "spotify",
            T::AppleMusic => "applemusic",
            T::Qobuz => "qobuz",
            T::Deezer => "deezer",
            T::Tidal => "tidal",
        };
        let backfill = |mut m: ipc_contract::DownloadMetadata| {
            if m.platform.as_deref().unwrap_or("").is_empty() {
                m.platform = fallback
                    .platform
                    .clone()
                    .filter(|p| !p.is_empty())
                    .or_else(|| Some(platform_label.to_string()));
            }
            if m.quality.as_deref().unwrap_or("").is_empty() {
                m.quality = fallback.quality.clone().filter(|q| !q.is_empty());
            }
            m
        };

        if !matches!(fallback.title.as_deref(), None | Some("")) {
            return backfill(fallback.clone());
        }
        let fetched = match target {
            T::YtMusic | T::YtVideo => self.try_prefetch_ytdlp_meta(url, fallback).await,
            T::Spotify => self.try_prefetch_spotify_meta(url, settings).await,
            T::AppleMusic => self.try_prefetch_apple_meta(url).await,
            T::Qobuz | T::Deezer | T::Tidal => self.try_prefetch_pipeline_meta(url, settings).await,
        };
        backfill(fetched.unwrap_or_else(|| fallback.clone()))
    }

    async fn start_download<R: serde::Serialize>(
        &self,
        target: crate::services::DownloadTarget,
        req: &R,
        url: &str,
        meta: &ipc_contract::DownloadMetadata,
    ) -> ipc_contract::StartDownloadResponse {
        let download_id = self.next_download_id();
        let settings = self.settings.read().await.clone();
        let ctx = crate::services::common::download::DownloadContext::from_state(self);
        if settings.download_location.is_empty() {
            return ctx.emit_no_download_location(download_id);
        }

        let meta = self
            .resolve_download_meta(target, url, &settings, meta)
            .await;
        self.emitter
            .emit_download_info(&ipc_contract::DownloadInfoEvent { download_id, meta });

        let req_value = match serde_json::to_value(req) {
            Ok(v) => v,
            Err(e) => {
                return ipc_contract::StartDownloadResponse::failed(
                    download_id,
                    format!("serialize request failed: {e}"),
                );
            }
        };
        crate::services::download_provider(target, self)
            .start(req_value, &ctx, download_id)
            .await
    }
}
impl BackendState {
    pub async fn scan_directory(
        &self,
        req: ipc_contract::ScanDirectoryRequest,
    ) -> MhResult<serde_json::Value> {
        let dir = PathBuf::from(&req.directory);
        let force = req.force.unwrap_or(false);
        let cache = media::scanner::CacheManager::new(self.user_data.clone());
        let flat_items = media::scanner::scan_directory(&dir, &cache, force).await?;

        let (audio_items, video_items): (Vec<_>, Vec<_>) =
            flat_items.into_iter().partition(|i| !i.is_video);

        let albums = media::scanner::organize_into_albums(&audio_items);

        let mut result: Vec<serde_json::Value> = Vec::new();

        for album in albums {
            let thumbnail = album
                .tracks
                .iter()
                .find_map(|t| t.cover_thumbnail.as_ref())
                .map(|b64| serde_json::json!({ "data": b64, "format": "jpeg" }));

            let tracks: Vec<serde_json::Value> = album
                .tracks
                .iter()
                .map(|t| {
                    let size = format_file_size(t.file_size);
                    let date = file_modified_date(&t.path);
                    let thumb = t.cover_thumbnail.as_ref().map(|b64| {
                        serde_json::json!({ "data": b64, "format": "jpeg" })
                    });
                    serde_json::json!({
                        "type": "music",
                        "title": t.title,
                        "size": size,
                        "date": date,
                        "path": t.path,
                        "duration": t.duration_secs,
                        "thumbnail": thumb,
                        "metadata": {
                            "artist": t.artist.clone().unwrap_or_default(),
                            "album": t.album.clone().unwrap_or_default(),
                            "year": t.year.clone().unwrap_or_default(),
                        }
                    })
                })
                .collect();

            result.push(serde_json::json!({
                "type": "music",
                "album": album.title,
                "artist": album.artist,
                "year": album.year,
                "thumbnail": thumbnail,
                "tracks": tracks,
            }));
        }

        for item in video_items {
            let size = format_file_size(item.file_size);
            let date = file_modified_date(&item.path);
            let thumb = item.cover_thumbnail.as_ref().map(|b64| {
                serde_json::json!({ "data": b64, "format": "jpeg" })
            });
            result.push(serde_json::json!({
                "type": "video",
                "title": item.title,
                "size": size,
                "date": date,
                "path": item.path,
                "duration": item.duration_secs,
                "thumbnail": thumb,
                "metadata": {},
            }));
        }

        Ok(serde_json::Value::Array(result))
    }

    pub fn clear_database(&self, _failed: bool, _downloads: bool) -> ipc_contract::ClearDatabaseResponse {
        self.active_downloads.clear();
        ipc_contract::ClearDatabaseResponse { success: true }
    }
}

impl BackendState {
    pub fn show_item_in_folder(
        &self,
        req: ipc_contract::ShowItemInFolderRequest,
    ) -> ipc_contract::ShowItemInFolderResponse {
        let path = std::path::Path::new(&req.path);
        let success = if path.exists() {
            if path.is_dir() {
                opener::open(path).is_ok()
            } else {
                opener::open(path.parent().unwrap_or(path)).is_ok()
            }
        } else {
            false
        };
        ipc_contract::ShowItemInFolderResponse { success }
    }
}

impl BackendState {
    pub fn get_version(&self) -> ipc_contract::GetVersionResponse {
        ipc_contract::GetVersionResponse {
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub async fn check_updates(&self) -> MhResult<ipc_contract::CheckUpdatesResponse> {
        let checker = update_checker::UpdateChecker::new("MediaHarbor", "mediaharbor", env!("CARGO_PKG_VERSION"));
        match checker.check_for_updates().await? {
            Some(release) => Ok(ipc_contract::CheckUpdatesResponse {
                update_available: true,
                latest_version: Some(release.tag_name),
                release_url: Some(release.html_url),
                release_notes: Some(release.body.unwrap_or_default()),
            }),
            None => Ok(ipc_contract::CheckUpdatesResponse {
                update_available: false,
                latest_version: None,
                release_url: None,
                release_notes: None,
            }),
        }
    }

    pub async fn check_deps(&self) -> ipc_contract::CheckDepsResponse {
        let python_ok = venv_manager::is_venv_ready();
        let ffmpeg_ok = which_binary("ffmpeg");

        let (yt_dlp_ok, votify_ok, gamdl_ok, bento4_ok) = if python_ok {
            let mut pip_cmd = tokio::process::Command::new(venv_manager::get_venv_python());
            pip_cmd.args(["-m", "pip", "list"]);
            subprocess::apply_no_window(&mut pip_cmd);
            let pip_list = pip_cmd
                .output()
                .await
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_default();

            let yt_dlp = pip_list.contains("yt-dlp");
            let votify = pip_list.contains("votify");
            let gamdl  = pip_list.contains("gamdl");
            let bento4 = installers::bento4::get_bento4_bin_dir().exists();
            (yt_dlp, votify, gamdl, bento4)
        } else {
            (false, false, false, false)
        };

        ipc_contract::CheckDepsResponse {
            ffmpeg: ffmpeg_ok,
            python: python_ok,
            yt_dlp: yt_dlp_ok,
            votify: votify_ok,
            gamdl: gamdl_ok,
            bento4: bento4_ok,
            is_sandboxed: crate::sandbox::is_sandboxed(),
        }
    }

    pub async fn install_dep(
        &self,
        req: ipc_contract::InstallDepRequest,
    ) -> ipc_contract::InstallDepResponse {
        async fn run_pip(mut c: tokio::process::Command) -> MhResult<()> {
            let output = c
                .output()
                .await
                .map_err(|e| MhError::Subprocess(e.to_string()))?;
            if output.status.success() {
                return Ok(());
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if !stderr.trim().is_empty() {
                stderr.trim().to_string()
            } else {
                stdout.trim().to_string()
            };
            Err(MhError::Subprocess(format!(
                "pip install failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                detail
            )))
        }

        let emitter = self.emitter.clone();
        let dep = req.dependency.clone();

        let make_progress = move |pct: u8, msg: &str| {
            emitter.emit_install_progress(&ipc_contract::InstallationProgressEvent {
                dependency: dep.clone(),
                percent: pct,
                status: msg.to_string(),
            });
        };

        let result: MhResult<()> = match req.dependency.as_str() {
            "python" => {
                match venv_manager::ensure_venv(|pct, msg| make_progress(pct, msg)).await {
                    Ok(()) => Ok(()),
                    Err(_) => {
                        make_progress(1, "Fetching available Python versions…");
                        match installers::python::fetch_python_versions().await {
                            Err(e) => Err(e),
                            Ok(versions) => {
                                match versions
                                    .into_iter()
                                    .max_by_key(|(k, _)| {
                                        k.split('.').nth(1).and_then(|m| m.parse::<u32>().ok()).unwrap_or(0)
                                    })
                                    .map(|(_, full)| full)
                                {
                                    None => Err(MhError::Other(
                                        "No Python versions available on python.org".to_string(),
                                    )),
                                    Some(best_version) => {
                                        make_progress(3, "Downloading Python…");
                                        match installers::python::download_and_install_python(
                                            &best_version,
                                            |pct, msg| make_progress(3 + (pct as u16 * 85 / 100) as u8, msg),
                                        )
                                        .await
                                        {
                                            Err(e) => Err(e),
                                            Ok(()) => {
                                                make_progress(88, "Setting up Python environment…");
                                                venv_manager::ensure_venv(|pct, msg| {
                                                    make_progress(88u8.saturating_add((pct as u16 * 12 / 15) as u8), msg)
                                                })
                                                .await
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "ffmpeg" => {
                installers::ffmpeg::download_and_install_ffmpeg(|pct, msg| make_progress(pct, msg)).await
            }
            "yt_dlp" | "ytdlp" => {
                match venv_manager::ensure_venv(|pct, msg| make_progress(pct, msg)).await {
                    Err(e) => Err(e),
                    Ok(()) => {
                        make_progress(50, "Installing yt-dlp...");
                        let mut c = tokio::process::Command::new(venv_manager::get_venv_python());
                        c.args(["-m", "pip", "install", "--upgrade", "yt-dlp", "isodate"]);
                        subprocess::apply_no_window(&mut c);
                        run_pip(c).await
                    }
                }
            }
            "apple" | "gamdl" => {
                match venv_manager::ensure_venv(|pct, msg| make_progress(pct, msg)).await {
                    Err(e) => Err(e),
                    Ok(()) => {
                        let _ = installers::bento4::download_and_install_bento4(|pct, msg| make_progress(pct, msg)).await;
                        make_progress(50, "Installing gamdl...");
                        let mut c = tokio::process::Command::new(venv_manager::get_venv_python());
                        c.args(["-m", "pip", "install", "--upgrade", "gamdl"]);
                        subprocess::apply_no_window(&mut c);
                        run_pip(c).await
                    }
                }
            }
            "spotify" | "votify" => {
                match venv_manager::ensure_venv(|pct, msg| make_progress(pct, msg)).await {
                    Err(e) => Err(e),
                    Ok(()) => {
                        let _ = installers::bento4::download_and_install_bento4(|pct, msg| make_progress(pct, msg)).await;
                        make_progress(50, "Installing votify...");
                        let mut c = tokio::process::Command::new(venv_manager::get_venv_python());
                        c.args(["-m", "pip", "install", "--upgrade", "votify", "pywidevine"]);
                        subprocess::apply_no_window(&mut c);
                        run_pip(c).await
                    }
                }
            }
            "qobuz" | "deezer" | "tidal" | "ytmusic" | "googleapi" | "pyapplemusicapi" => {
                make_progress(100, "built-in (native Rust)");
                Ok(())
            }
            "orpheus" => {
                orpheus::install_orpheus(|pct, msg| make_progress(pct, msg)).await
            }
            other => Err(MhError::Other(format!("Unknown dependency: {}", other))),
        };

        match result {
            Ok(_) => {
                make_progress(100, "done");
                ipc_contract::InstallDepResponse { success: true, error: None }
            }
            Err(e) => ipc_contract::InstallDepResponse {
                success: false,
                error: Some(e.to_string()),
            },
        }
    }

    pub async fn check_orpheus_deps(&self) -> ipc_contract::CheckOrpheusDepsResponse {
        let settings = self.settings.read().await;
        let mut modules: Vec<ipc_contract::OrpheusModuleStatus> = orpheus::KNOWN_MODULES.iter().map(|(id, label, _)| {
            ipc_contract::OrpheusModuleStatus {
                id: id.to_string(),
                label: label.to_string(),
                installed: orpheus::is_module_installed(id),
            }
        }).collect();

        let custom: Vec<serde_json::Value> = serde_json::from_str(&settings.orpheus_custom_modules).unwrap_or_default();
        for item in custom {
            let id = match item["id"].as_str() {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            };
            if modules.iter().any(|m| m.id == id) {
                continue;
            }
            let label = item["label"].as_str().unwrap_or(&id).to_string();
            modules.push(ipc_contract::OrpheusModuleStatus {
                installed: orpheus::is_module_installed(&id),
                id,
                label,
            });
        }

        ipc_contract::CheckOrpheusDepsResponse {
            orpheus_installed: orpheus::is_orpheus_installed(),
            modules,
        }
    }

    pub async fn install_orpheus_module(
        &self,
        req: ipc_contract::InstallOrpheusModuleRequest,
    ) -> ipc_contract::InstallOrpheusModuleResponse {
        let emitter = self.emitter.clone();
        let dep = format!("orpheus_module_{}", req.module_id);

        let make_progress = move |pct: u8, msg: &str| {
            emitter.emit_install_progress(&ipc_contract::InstallationProgressEvent {
                dependency: dep.clone(),
                percent: pct,
                status: msg.to_string(),
            });
        };

        let git_url = if let Some(url) = req.custom_url.as_deref() {
            url.to_string()
        } else {
            match orpheus::KNOWN_MODULES.iter().find(|(id, _, _)| *id == req.module_id) {
                Some((_, _, url)) => url.to_string(),
                None => {
                    return ipc_contract::InstallOrpheusModuleResponse {
                        success: false,
                        error: Some(format!("Unknown module: {}", req.module_id)),
                    };
                }
            }
        };

        let result = orpheus::install_module(&req.module_id, &git_url, make_progress).await;
        match result {
            Ok(_) => {
                if req.custom_url.is_some() {
                    if let Some(label) = req.label.filter(|l| !l.is_empty()) {
                        let mut settings = self.settings.write().await;
                        let mut custom: Vec<serde_json::Value> = serde_json::from_str(&settings.orpheus_custom_modules).unwrap_or_default();
                        if !custom.iter().any(|m| m["id"].as_str() == Some(&req.module_id)) {
                            custom.push(serde_json::json!({ "id": req.module_id, "label": label }));
                            settings.orpheus_custom_modules = serde_json::to_string(&custom).unwrap_or_else(|_| "[]".into());
                            settings::save_settings(&settings, &self.user_data).await.ok();
                        }
                    }
                }
                ipc_contract::InstallOrpheusModuleResponse { success: true, error: None }
            }
            Err(e) => ipc_contract::InstallOrpheusModuleResponse {
                success: false,
                error: Some(e.to_string()),
            },
        }
    }

    pub async fn get_dependency_versions(&self) -> ipc_contract::GetDependencyVersionsResponse {
        let mut versions = std::collections::HashMap::new();

        for bin in &["ffmpeg"] {
            if let Some(v) = binary_version(bin) {
                versions.insert(bin.to_string(), v);
            }
        }

        if let Ok(v) = venv_manager::find_system_python().await {
            versions.insert("python".to_string(), v);
        }

        if venv_manager::is_venv_ready() {
            let py = venv_manager::get_venv_python();
            for pkg in &["yt-dlp", "gamdl", "votify"] {
                let mut c = tokio::process::Command::new(&py);
                c.args(["-m", "pip", "show", pkg]);
                subprocess::apply_no_window(&mut c);
                let out = c.output().await;
                if let Ok(output) = out {
                    let text = String::from_utf8_lossy(&output.stdout);
                    if let Some(m) = regex::Regex::new(r"(?m)^Version:\s+(.+)$")
                        .ok()
                        .and_then(|re| re.captures(&text))
                        .and_then(|c| c.get(1))
                    {
                        versions.insert(pkg.to_string(), m.as_str().trim().to_string());
                    }
                }
            }
        }

        ipc_contract::GetDependencyVersionsResponse { versions }
    }
}

fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn file_modified_date(path: &std::path::Path) -> String {
    use std::time::UNIX_EPOCH;
    let meta = std::fs::metadata(path).ok();
    let modified = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if modified == 0 {
        return String::from("Unknown");
    }
    let days = modified / 86400;
    let mut y = 1970u32;
    let mut rem_days = days as u32;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if rem_days < days_in_year {
            break;
        }
        rem_days -= days_in_year;
        y += 1;
    }
    let month_days = [31u32, if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0usize;
    while m < 12 && rem_days >= month_days[m] {
        rem_days -= month_days[m];
        m += 1;
    }
    format!("{:04}-{:02}-{:02}", y, m + 1, rem_days + 1)
}

fn which_binary(name: &str) -> bool {
    let mut cmd = std::process::Command::new(name);
    cmd.arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    subprocess::apply_no_window_std(&mut cmd);
    cmd.status().is_ok()
}

fn binary_version(name: &str) -> Option<String> {
    let mut cmd = std::process::Command::new(name);
    cmd.arg("--version");
    subprocess::apply_no_window_std(&mut cmd);
    let output = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().next().map(|l| l.trim().to_string())
}

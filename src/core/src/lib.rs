

pub mod defaults;
pub mod download_order;
pub mod errors;
pub mod fetchers;
pub mod http_client;
pub mod ipc_contract;
pub mod logger;
pub mod settings;
pub mod streaming_server;
pub mod subprocess;
pub mod update_checker;
pub mod venv_manager;

pub mod crypto;
pub mod installers;
pub mod streamrip;
pub mod apis;
pub mod meta;
pub mod media;
pub mod downloads;

pub use defaults::Settings;
pub use errors::{MhError, MhResult};
pub use logger::{Logger, LogEmitter, NoopEmitter};

use std::sync::Arc;

pub trait EventEmitter: Send + Sync {
    fn emit_log(&self, entry: &ipc_contract::BackendLogEvent);
    fn emit_download_info(&self, event: &ipc_contract::DownloadInfoEvent);
    fn emit_progress(&self, event: &ipc_contract::DownloadProgressEvent);
    fn emit_stream_ready(&self, event: &ipc_contract::StreamReadyEvent);
    fn emit_install_progress(&self, event: &ipc_contract::InstallationProgressEvent);
    fn emit_app_error(&self, event: &ipc_contract::AppErrorEvent);
}

pub struct NoopEventEmitter;
impl EventEmitter for NoopEventEmitter {
    fn emit_log(&self, _: &ipc_contract::BackendLogEvent) {}
    fn emit_download_info(&self, _: &ipc_contract::DownloadInfoEvent) {}
    fn emit_progress(&self, _: &ipc_contract::DownloadProgressEvent) {}
    fn emit_stream_ready(&self, _: &ipc_contract::StreamReadyEvent) {}
    fn emit_install_progress(&self, _: &ipc_contract::InstallationProgressEvent) {}
    fn emit_app_error(&self, _: &ipc_contract::AppErrorEvent) {}
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
    pub streaming_server: Option<streaming_server::StreamingServer>,
    pub librespot: Arc<RwLock<apis::librespot_service::LibrespotService>>,
    pub apple_music: Arc<RwLock<apis::apple_music_service::AppleMusicService>>,
    pub credentials: apis::credentials::ApiCredentials,
    pub user_data: PathBuf,
    pub logger: Logger,
    pub emitter: Arc<dyn EventEmitter>,
    pub yt_stream_cache: Arc<apis::yt_audio_stream::YtAudioStreamCache>,
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

        let loaded = settings::load_settings(&user_data).await;
        logger.info("system", "Settings loaded");

        let streaming_server = streaming_server::StreamingServer::start().await.ok();
        if let Some(ref srv) = streaming_server {
            logger.info("system", &format!("Streaming server on port {}", srv.port));
        }

        let mut librespot = apis::librespot_service::LibrespotService::new();

        let apple_music = apis::apple_music_service::AppleMusicService::from_settings(&loaded);

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
            streaming_server,
            librespot: Arc::new(RwLock::new(librespot)),
            apple_music: Arc::new(RwLock::new(apple_music)),
            credentials,
            user_data,
            logger,
            emitter,
            yt_stream_cache: Arc::new(apis::yt_audio_stream::YtAudioStreamCache::new()),
        };

        Ok(state)
    }

    pub async fn shutdown(&mut self) {
        if let Some(ref mut srv) = self.streaming_server {
            srv.stop();
        }
        self.logger.info("system", "Backend shutdown complete");
    }
}

impl BackendState {
    pub async fn perform_search(
        &self,
        req: ipc_contract::PerformSearchRequest,
    ) -> MhResult<serde_json::Value> {
        use ipc_contract::{SearchPlatform, SearchType};

        let settings = self.settings.read().await.clone();

        match req.platform {
            SearchPlatform::Spotify => {
                let mut librespot = self.librespot.write().await;
                if librespot.is_logged_in()
                    && !matches!(
                        req.search_type,
                        SearchType::Audiobook | SearchType::Episode
                    )
                {
                    return librespot
                        .search(&req.query, search_type_to_str(&req.search_type), 20)
                        .await;
                }
                drop(librespot);
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
                client
                    .search(&req.query, map_spotify_type(&req.search_type), 20)
                    .await
            }

            SearchPlatform::Tidal => {
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
                if !settings.tidal_access_token.is_empty() {
                    match client
                        .search_v1(
                            &req.query,
                            map_tidal_type(&req.search_type),
                            &settings.tidal_country_code,
                            &settings.tidal_access_token,
                        )
                        .await
                    {
                        Ok(r) => Ok(r),
                        Err(e) if e.to_string().contains("401") || e.to_string().contains("Unauthorized") => {
                            self.emitter.emit_app_error(&ipc_contract::AppErrorEvent {
                                message: "Tidal session expired — please sign in again.".into(),
                                context: Some("tidal_search".into()),
                                needs_auth: Some("tidal".into()),
                            });
                            client
                                .search_v2(&req.query, map_tidal_type(&req.search_type), &settings.tidal_country_code)
                                .await
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    client
                        .search_v2(&req.query, map_tidal_type(&req.search_type), &settings.tidal_country_code)
                        .await
                }
            }

            SearchPlatform::Deezer => {
                let client = apis::deezer_api::DeezerApiClient::new()?;
                client
                    .search(&req.query, map_deezer_type(&req.search_type), 20)
                    .await
            }

            SearchPlatform::Qobuz => {
                let client = if settings.qobuz_app_id.is_empty() {
                    apis::qobuz_api::QobuzApiClient::with_bundled_credentials()?
                } else {
                    apis::qobuz_api::QobuzApiClient::new(
                        settings.qobuz_app_id.clone(),
                        settings.qobuz_password_or_token.clone(),
                        settings.qobuz_app_secret.clone(),
                    )?
                };
                client
                    .search(&req.query, search_type_to_str(&req.search_type), 20)
                    .await
            }

            SearchPlatform::AppleMusic => {
                let client = apis::apple_music_api::AppleMusicApiClient::new(None)?;
                client
                    .search(&req.query, map_apple_entity(&req.search_type), 20)
                    .await
            }

            SearchPlatform::YoutubeMusic => {
                let client = apis::ytmusic_search_api::YtMusicClient::init().await?;
                let filter = map_ytmusic_filter(&req.search_type);
                let results =
                    apis::ytmusic_search_api::search(&client, &req.query, filter).await?;
                Ok(serde_json::to_value(results)?)
            }

            SearchPlatform::Youtube => {
                let yt_key = if settings.youtube_api_key.is_empty() {
                    self.credentials.youtube_api_key.clone()
                } else {
                    settings.youtube_api_key.clone()
                };
                let client = apis::yt_search_api::YtSearchClient::new(yt_key)?;
                match req.search_type {
                    SearchType::Playlist => client.search_playlists(&req.query, 20).await,
                    SearchType::Channel | SearchType::Artist => {
                        client.search_channels(&req.query, 20).await
                    }
                    _ => client.search_videos(&req.query, 20).await,
                }
            }
        }
    }
}

impl BackendState {
    pub async fn play_media(
        &self,
        req: ipc_contract::PlayMediaRequest,
    ) -> MhResult<ipc_contract::PlayMediaResponse> {
        let settings = self.settings.read().await.clone();
        let platform = req.platform.as_str();

        match platform {
            "youtube" => {
                use std::process::Stdio;
                use tokio::io::AsyncReadExt;

                let (video_info, audio_info) =
                    apis::yt_audio_stream::get_video_stream_url(&req.url, None).await?;

                let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, String>>(16);

                if video_info.url == audio_info.url {
                    let url = video_info.url.clone();
                    tokio::spawn(async move {
                        use futures_util::StreamExt;
                        let client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(300))
                            .gzip(false)
                            .build()
                            .unwrap();
                        match client
                            .get(&url)
                            .header("Referer", "https://www.youtube.com/")
                            .header("Range", "bytes=0-")
                            .send()
                            .await
                        {
                            Ok(resp) => {
                                let mut stream = resp.bytes_stream();
                                while let Some(chunk) = stream.next().await {
                                    match chunk {
                                        Ok(b) => { if tx.send(Ok(b)).await.is_err() { break; } }
                                        Err(e) => { let _ = tx.send(Err(e.to_string())).await; break; }
                                    }
                                }
                            }
                            Err(e) => { let _ = tx.send(Err(e.to_string())).await; }
                        }
                    });
                } else {
                    let video_url = video_info.url.clone();
                    let audio_url = audio_info.url.clone();
                    tokio::spawn(async move {
                        let mut child = match tokio::process::Command::new("ffmpeg")
                            .args([
                                "-loglevel", "error",
                                "-i", &video_url,
                                "-i", &audio_url,
                                "-c", "copy",
                                "-f", "mp4",
                                "-movflags", "frag_keyframe+empty_moov+default_base_moof",
                                "pipe:1",
                            ])
                            .stdout(Stdio::piped())
                            .stderr(Stdio::null())
                            .spawn()
                        {
                            Ok(c) => c,
                            Err(e) => {
                                let _ = tx.send(Err(format!("ffmpeg not found: {}", e))).await;
                                return;
                            }
                        };

                        if let Some(mut stdout) = child.stdout.take() {
                            let mut buf = vec![0u8; 65_536];
                            loop {
                                match stdout.read(&mut buf).await {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        if tx.send(Ok(bytes::Bytes::copy_from_slice(&buf[..n]))).await.is_err() {
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        let _ = tx.send(Err(e.to_string())).await;
                                        break;
                                    }
                                }
                            }
                        }
                        let _ = child.wait().await;
                    });
                }

                let server = self
                    .streaming_server
                    .as_ref()
                    .ok_or_else(|| MhError::Other("Streaming server not running".into()))?;
                let id = uuid::Uuid::new_v4().to_string();
                let stream_url = server.register_stream_progressive(&id, rx, "video/mp4");

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url,
                    platform: platform.to_string(),
                    duration_sec: None,
                    media_type: Some("video".to_string()),
                })
            }

            "youtubeMusic" | "youtubemusic" => {
                let stream_info =
                    apis::yt_audio_stream::get_audio_stream_url(&req.url, &self.yt_stream_cache, None).await?;
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(300))
                    .connect_timeout(std::time::Duration::from_secs(10))
                    .redirect(reqwest::redirect::Policy::limited(10))
                    .gzip(false)
                    .build()
                    .map_err(MhError::Network)?;
                let resp = client
                    .get(&stream_info.url)
                    .header("Referer", "https://www.youtube.com/")
                    .header("Origin", "https://www.youtube.com")
                    .header("Range", "bytes=0-")
                    .send()
                    .await
                    .map_err(MhError::Network)?;
                if resp.status().as_u16() >= 400 {
                    return Err(MhError::Network(resp.error_for_status().unwrap_err()));
                }
                let data = resp.bytes().await.map_err(MhError::Network)?;
                let content_type = stream_info.mime_type.clone();

                let server = self
                    .streaming_server
                    .as_ref()
                    .ok_or_else(|| MhError::Other("Streaming server not running".into()))?;
                let id = uuid::Uuid::new_v4().to_string();
                let stream_url = server.register_stream(&id, data, &content_type);

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url,
                    platform: platform.to_string(),
                    duration_sec: None,
                    media_type: Some("audio".to_string()),
                })
            }

            "spotify" => {
                let mut librespot = self.librespot.write().await;
                if !librespot.is_logged_in() {
                    return Err(MhError::Auth(
                        "Spotify streaming requires login. Configure cookies in Settings.".into(),
                    ));
                }
                let track_id = extract_spotify_id(&req.url).unwrap_or_else(|| req.url.clone());
                let venv_py = if venv_manager::is_venv_ready() {
                    Some(venv_manager::get_venv_python())
                } else {
                    None
                };
                let (data, content_type) = librespot
                    .get_track_stream(&track_id, venv_py.as_deref())
                    .await?;
                drop(librespot);

                let server = self
                    .streaming_server
                    .as_ref()
                    .ok_or_else(|| MhError::Other("Streaming server not running".into()))?;
                let id = uuid::Uuid::new_v4().to_string();
                let stream_url = server.register_stream(&id, data, &content_type);

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url,
                    platform: "spotify".to_string(),
                    duration_sec: None,
                    media_type: Some("audio".to_string()),
                })
            }

            "tidal" => {
                let client = streamrip::tidal_client::TidalClient::authenticate(&settings).await?;
                let track_id = extract_tidal_track_id(&req.url)
                    .ok_or_else(|| MhError::Parse("Could not extract Tidal track ID".into()))?;

                let server = self
                    .streaming_server
                    .as_ref()
                    .ok_or_else(|| MhError::Other("Streaming server not running".into()))?;
                let id = uuid::Uuid::new_v4().to_string();

                let (tx, rx) = tokio::sync::mpsc::channel(32);
                let mime = client.fetch_audio_progressive(&track_id, tx).await?;
                let stream_url = server.register_stream_progressive(&id, rx, mime);

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url,
                    platform: "tidal".to_string(),
                    duration_sec: None,
                    media_type: Some("audio".to_string()),
                })
            }

            "deezer" => {
                if settings.deezer_arl.is_empty() {
                    return Err(MhError::Auth(
                        "Deezer ARL not configured. Add it in Settings → Deezer → ARL Token.".into(),
                    ));
                }
                let client =
                    streamrip::deezer_client::DeezerClient::new(&settings.deezer_arl)?;
                client.authenticate().await?;
                let track_id = streamrip::orchestrator::extract_platform_id(
                    &req.url,
                    streamrip::orchestrator::Platform::Deezer,
                    streamrip::orchestrator::ContentType::Track,
                ).ok_or_else(|| MhError::Parse("Could not extract Deezer track ID".into()))?;
                let (url_str, ext) = client.get_stream_url(&track_id, 3).await?;
                let mime_type: &'static str = if ext == "flac" { "audio/flac" } else { "audio/mpeg" };

                let server = self
                    .streaming_server
                    .as_ref()
                    .ok_or_else(|| MhError::Other("Streaming server not running".into()))?;
                let id = uuid::Uuid::new_v4().to_string();

                let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, String>>(32);
                let http_client = http_client::build_mozilla_client()?;
                let track_id_clone = track_id.clone();
                tokio::spawn(async move {
                    use futures_util::StreamExt;
                    use crate::crypto::deezer::{generate_blowfish_key, decrypt_chunk};

                    const CHUNK: usize = 6144;
                    const ENC: usize = 2048;

                    let key = generate_blowfish_key(&track_id_clone);
                    let resp = match http_client.get(&url_str).send().await {
                        Ok(r) => r,
                        Err(e) => { let _ = tx.send(Err(e.to_string())).await; return; }
                    };

                    let mut buf: Vec<u8> = Vec::with_capacity(CHUNK * 2);
                    let mut stream = resp.bytes_stream();

                    while let Some(result) = stream.next().await {
                        match result {
                            Ok(incoming) => buf.extend_from_slice(&incoming),
                            Err(e) => { let _ = tx.send(Err(e.to_string())).await; return; }
                        }

                        while buf.len() >= CHUNK {
                            let raw: Vec<u8> = buf.drain(..CHUNK).collect();
                            let mut out = decrypt_chunk(&key, &raw[..ENC]);
                            out.extend_from_slice(&raw[ENC..]);
                            if tx.send(Ok(bytes::Bytes::from(out))).await.is_err() { return; }
                        }
                    }

                    if !buf.is_empty() {
                        let raw = buf;
                        let out: Vec<u8> = if raw.len() >= ENC {
                            let mut dec = decrypt_chunk(&key, &raw[..ENC]);
                            dec.extend_from_slice(&raw[ENC..]);
                            dec
                        } else {
                            raw
                        };
                        let _ = tx.send(Ok(bytes::Bytes::from(out))).await;
                    }
                });

                let stream_url = server.register_stream_progressive(&id, rx, mime_type);

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url,
                    platform: "deezer".to_string(),
                    duration_sec: None,
                    media_type: Some("audio".to_string()),
                })
            }

            "qobuz" => {
                let client =
                    streamrip::qobuz_client::QobuzClient::authenticate(&settings).await?;
                let track_id = streamrip::orchestrator::extract_platform_id(
                    &req.url,
                    streamrip::orchestrator::Platform::Qobuz,
                    streamrip::orchestrator::ContentType::Track,
                ).ok_or_else(|| MhError::Parse("Could not extract Qobuz track ID".into()))?;
                let stream_url = client.get_file_url(&track_id, 27).await?;

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url,
                    platform: "qobuz".to_string(),
                    duration_sec: None,
                    media_type: Some("audio".to_string()),
                })
            }

            "applemusic" => {
                let apple = self.apple_music.read().await;
                if !apple.is_configured() {
                    return Err(MhError::Auth(
                        "Apple Music requires cookies. Configure in Settings → Apple.".into(),
                    ));
                }
                let track = apple.get_track_stream(&req.url, None).await?;
                drop(apple);

                let server = self
                    .streaming_server
                    .as_ref()
                    .ok_or_else(|| MhError::Other("Streaming server not running".into()))?;
                let id = uuid::Uuid::new_v4().to_string();
                let stream_url = server.register_stream(&id, track.data, &track.content_type);

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url,
                    platform: "applemusic".to_string(),
                    duration_sec: None,
                    media_type: Some("audio".to_string()),
                })
            }

            _ => {
                if req.url.is_empty() || req.url == "null" {
                    return Err(MhError::NotFound(format!(
                        "No stream found for {}",
                        platform
                    )));
                }

                let path = std::path::Path::new(&req.url);
                if path.is_absolute() && path.exists() {
                    let server = self.streaming_server.as_ref()
                        .ok_or_else(|| MhError::Other("Streaming server not running".into()))?;

                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                    let is_video = matches!(ext.as_str(), "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "m4v");
                    let mime_type: &str = match ext.as_str() {
                        "mp3"         => "audio/mpeg",
                        "flac"        => "audio/flac",
                        "m4a" | "aac" => "audio/mp4",
                        "opus"        => "audio/ogg",
                        "wav"         => "audio/wav",
                        "ogg"         => "audio/ogg",
                        "mp4" | "m4v" => "video/mp4",
                        "mkv"         => "video/x-matroska",
                        "webm"        => "video/webm",
                        "mov"         => "video/quicktime",
                        "avi"         => "video/x-msvideo",
                        "flv"         => "video/x-flv",
                        _             => if is_video { "video/mp4" } else { "audio/mpeg" },
                    };

                    let data = tokio::fs::read(path).await
                        .map_err(|e| MhError::Other(format!("Failed to read local file: {}", e)))?;

                    let id = uuid::Uuid::new_v4().to_string();
                    let stream_url = server.register_stream(&id, bytes::Bytes::from(data), mime_type);
                    return Ok(ipc_contract::PlayMediaResponse {
                        stream_url,
                        platform: "local".to_string(),
                        duration_sec: None,
                        media_type: Some(if is_video { "video" } else { "audio" }.to_string()),
                    });
                }

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url: req.url,
                    platform: platform.to_string(),
                    duration_sec: None,
                    media_type: None,
                })
            }
        }
    }
}

impl BackendState {
    pub async fn cancel_download(&self, download_id: u64) -> bool {
        if let Some(flag) = self.active_downloads.get(&download_id) {
            flag.store(true, Ordering::Relaxed);
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

fn search_type_to_str(t: &ipc_contract::SearchType) -> &'static str {
    use ipc_contract::SearchType::*;
    match t {
        Track | Song => "track",
        Album => "album",
        Artist => "artist",
        Playlist => "playlist",
        Episode => "episode",
        Podcast | Show => "podcast",
        Audiobook => "audiobook",
        Video | MusicVideo => "video",
        Channel => "channel",
    }
}

fn map_spotify_type(t: &ipc_contract::SearchType) -> apis::spotify_api::SpotifySearchType {
    use ipc_contract::SearchType::*;
    match t {
        Album => apis::spotify_api::SpotifySearchType::Album,
        Artist => apis::spotify_api::SpotifySearchType::Artist,
        Playlist => apis::spotify_api::SpotifySearchType::Playlist,
        Episode => apis::spotify_api::SpotifySearchType::Episode,
        Podcast | Show => apis::spotify_api::SpotifySearchType::Show,
        Audiobook => apis::spotify_api::SpotifySearchType::Audiobook,
        _ => apis::spotify_api::SpotifySearchType::Track,
    }
}

fn map_tidal_type(t: &ipc_contract::SearchType) -> apis::tidal_api::TidalSearchType {
    use ipc_contract::SearchType::*;
    match t {
        Album => apis::tidal_api::TidalSearchType::Albums,
        Artist => apis::tidal_api::TidalSearchType::Artists,
        Playlist => apis::tidal_api::TidalSearchType::Playlists,
        Video => apis::tidal_api::TidalSearchType::Videos,
        _ => apis::tidal_api::TidalSearchType::Tracks,
    }
}

fn map_deezer_type(t: &ipc_contract::SearchType) -> apis::deezer_api::DeezerSearchType {
    use ipc_contract::SearchType::*;
    match t {
        Album => apis::deezer_api::DeezerSearchType::Album,
        Artist => apis::deezer_api::DeezerSearchType::Artist,
        Playlist => apis::deezer_api::DeezerSearchType::Playlist,
        Podcast => apis::deezer_api::DeezerSearchType::Podcast,
        Episode => apis::deezer_api::DeezerSearchType::Episode,
        _ => apis::deezer_api::DeezerSearchType::Track,
    }
}

fn map_apple_entity(t: &ipc_contract::SearchType) -> apis::apple_music_api::AppleMusicMediaType {
    use ipc_contract::SearchType::*;
    match t {
        Album => apis::apple_music_api::AppleMusicMediaType::Album,
        Artist => apis::apple_music_api::AppleMusicMediaType::Artist,
        Playlist => apis::apple_music_api::AppleMusicMediaType::Playlist,
        Video | MusicVideo => apis::apple_music_api::AppleMusicMediaType::MusicVideo,
        _ => apis::apple_music_api::AppleMusicMediaType::Song,
    }
}

fn map_ytmusic_filter(t: &ipc_contract::SearchType) -> apis::ytmusic_search_api::YtMusicFilter {
    use ipc_contract::SearchType::*;
    match t {
        Album => apis::ytmusic_search_api::YtMusicFilter::Album,
        Playlist => apis::ytmusic_search_api::YtMusicFilter::Playlist,
        Artist => apis::ytmusic_search_api::YtMusicFilter::Artist,
        Podcast => apis::ytmusic_search_api::YtMusicFilter::Podcast,
        Video => apis::ytmusic_search_api::YtMusicFilter::Video,
        _ => apis::ytmusic_search_api::YtMusicFilter::Song,
    }
}

fn extract_spotify_id(url: &str) -> Option<String> {
    let re = regex::Regex::new(
        r"spotify\.com/(?:track|episode|album|artist|playlist)/([a-zA-Z0-9]+)",
    )
    .ok()?;
    re.captures(url)?.get(1).map(|m| m.as_str().to_string())
}

fn extract_tidal_track_id(url: &str) -> Option<String> {
    let re = regex::Regex::new(r"tidal\.com/(?:browse/)?(?:track|album|video)/(\d+)").ok()?;
    re.captures(url)?.get(1).map(|m| m.as_str().to_string())
}

fn getrandom_bytes(buf: &mut [u8]) -> MhResult<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(buf))
        .map_err(MhError::Io)
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
            let mut s = self.settings.write().await;
            *s = req.settings.clone();
        }

        {
            let mut am = self.apple_music.write().await;
            *am = apis::apple_music_service::AppleMusicService::from_settings(&req.settings);
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

        ipc_contract::SetSettingsResponse { success: true, error: None }
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
                let client = streamrip::tidal_client::TidalClient::authenticate(&settings).await?;
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
                let client = streamrip::tidal_client::TidalClient::authenticate(&settings).await?;
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
                let raw = client.get_artist_albums(artist_id, cc).await?;
                let items = raw["data"].as_array().cloned().unwrap_or_default();
                let albums: Vec<serde_json::Value> = items.iter().map(|a| {
                    let attr = if a["attributes"].is_object() { &a["attributes"] } else { a };
                    let img_arr = attr["imageCover"].as_array();
                    let thumbnail = img_arr.and_then(|arr| {
                        arr.iter()
                            .find(|i| i["width"].as_i64().unwrap_or(0) >= 640)
                            .or_else(|| arr.last())
                            .and_then(|i| i["href"].as_str())
                            .map(|s| s.to_string())
                    });
                    let album_id = a["id"].as_str().map(|s| s.to_string())
                        .or_else(|| a["id"].as_i64().map(|n| n.to_string()))
                        .unwrap_or_default();
                    let url = attr["url"].as_str().map(|s| s.to_string())
                        .unwrap_or_else(|| format!("https://tidal.com/browse/album/{}", album_id));
                    serde_json::json!({
                        "id": album_id,
                        "title": attr["title"].as_str().or_else(|| a["title"].as_str()),
                        "thumbnail": thumbnail,
                        "releaseDate": attr["releaseDate"].as_str().or_else(|| a["releaseDate"].as_str()),
                        "trackCount": attr["numberOfItems"].as_i64().or_else(|| attr["numberOfTracks"].as_i64()),
                        "url": url,
                        "explicit": attr["explicit"].as_bool().unwrap_or(false),
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

impl BackendState {
    fn next_download_id(&self) -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    async fn try_prefetch_streamrip_meta(
        &self,
        url: &str,
        settings: &Settings,
    ) -> Option<ipc_contract::DownloadMetadata> {
        use streamrip::orchestrator::{detect_platform_and_type, extract_platform_id};
        use streamrip::orchestrator::Platform as SrPlatform;
        use streamrip::orchestrator::ContentType;

        let (platform, content_type) = detect_platform_and_type(url)?;
        if !matches!(content_type, ContentType::Track) {
            return None;
        }
        let id = extract_platform_id(url, platform, content_type)?;

        match platform {
            SrPlatform::Deezer => {
                let client = apis::deezer_api::DeezerApiClient::new().ok()?;
                let data = client.get_track(&id).await.ok()?;
                Some(ipc_contract::DownloadMetadata {
                    title:     data["title"].as_str().map(String::from),
                    artist:    data["artist"]["name"].as_str().map(String::from),
                    album:     data["album"]["title"].as_str().map(String::from),
                    thumbnail: data["album"]["cover_xl"].as_str()
                                   .or_else(|| data["album"]["cover_big"].as_str())
                                   .map(String::from),
                    platform:  Some("deezer".into()),
                    quality:   None,
                })
            }
            SrPlatform::Qobuz => {
                let client = if settings.qobuz_app_id.is_empty() {
                    apis::qobuz_api::QobuzApiClient::with_bundled_credentials().ok()?
                } else {
                    apis::qobuz_api::QobuzApiClient::new(
                        settings.qobuz_app_id.clone(),
                        settings.qobuz_password_or_token.clone(),
                        settings.qobuz_app_secret.clone(),
                    ).ok()?
                };
                let data = client.get_track(&id).await.ok()?;
                Some(ipc_contract::DownloadMetadata {
                    title:     data["title"].as_str().map(String::from),
                    artist:    data["performer"]["name"].as_str()
                                   .or_else(|| data["album"]["artist"]["name"].as_str())
                                   .map(String::from),
                    album:     data["album"]["title"].as_str().map(String::from),
                    thumbnail: data["album"]["image"]["large"].as_str().map(String::from),
                    platform:  Some("qobuz".into()),
                    quality:   None,
                })
            }
            SrPlatform::Tidal => {
                if settings.tidal_access_token.is_empty() {
                    return None;
                }
                let country = if settings.tidal_country_code.is_empty() { "US" } else { &settings.tidal_country_code };
                let http = http_client::build_client().ok()?;
                let resp = http
                    .get(format!("https://api.tidal.com/v1/tracks/{}", id))
                    .header("Authorization", format!("Bearer {}", settings.tidal_access_token))
                    .query(&[("countryCode", country)])
                    .send()
                    .await
                    .ok()?;
                if !resp.status().is_success() { return None; }
                let data: serde_json::Value = resp.json().await.ok()?;
                let cover_id = data["album"]["cover"].as_str().unwrap_or("").replace('-', "/");
                Some(ipc_contract::DownloadMetadata {
                    title:     data["title"].as_str().map(String::from),
                    artist:    data["artist"]["name"].as_str().map(String::from),
                    album:     data["album"]["title"].as_str().map(String::from),
                    thumbnail: if cover_id.is_empty() { None } else {
                        Some(format!("https://resources.tidal.com/images/{}/640x640.jpg", cover_id))
                    },
                    platform:  Some("tidal".into()),
                    quality:   None,
                })
            }
        }
    }

    async fn try_prefetch_spotify_meta(
        &self,
        url: &str,
        settings: &Settings,
    ) -> Option<ipc_contract::DownloadMetadata> {
        if !url.contains("/track/") && !url.starts_with("spotify:track:") {
            return None;
        }
        let track_id = if url.starts_with("spotify:track:") {
            url["spotify:track:".len()..].to_string()
        } else {
            url.trim_end_matches('/')
                .rsplit('/')
                .next()?
                .split('?')
                .next()?
                .to_string()
        };
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
        let client = apis::spotify_api::SpotifyApiClient::new(client_id, client_secret).ok()?;
        let data = client.get_track(&track_id).await.ok()?;
        Some(ipc_contract::DownloadMetadata {
            title:     data["name"].as_str().map(String::from),
            artist:    data["artists"][0]["name"].as_str().map(String::from),
            album:     data["album"]["name"].as_str().map(String::from),
            thumbnail: data["album"]["images"][0]["url"].as_str().map(String::from),
            platform:  Some("spotify".into()),
            quality:   None,
        })
    }

    async fn try_prefetch_apple_meta(
        &self,
        url: &str,
    ) -> Option<ipc_contract::DownloadMetadata> {
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
        let client = apis::apple_music_api::AppleMusicApiClient::unauthenticated().ok()?;
        let data = client.lookup_by_id(&lookup_id).await.ok()?;
        if data.is_null() {
            return None;
        }
        let thumbnail = data["artworkUrl100"]
            .as_str()
            .map(|s| s.replace("100x100", "640x640"));
        Some(ipc_contract::DownloadMetadata {
            title:     data["trackName"].as_str()
                           .or_else(|| data["collectionName"].as_str())
                           .map(String::from),
            artist:    data["artistName"].as_str().map(String::from),
            album:     data["collectionName"].as_str().map(String::from),
            thumbnail,
            platform:  Some("applemusic".into()),
            quality:   None,
        })
    }

    pub async fn start_yt_music_download(
        &self,
        req: ipc_contract::StartYtMusicDownloadRequest,
    ) -> ipc_contract::StartDownloadResponse {
        let download_id = self.next_download_id();
        let settings = self.settings.read().await.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_downloads.insert(download_id, cancel_flag.clone());

        let meta = if req.meta.title.is_none() || req.meta.title.as_deref() == Some("") {
            let yt_dlp_cmd = downloads::yt_dlp::find_yt_dlp_command(None);
            if let Ok(fetched) = downloads::yt_dlp::prefetch_metadata(&req.url, &yt_dlp_cmd).await {
                ipc_contract::DownloadMetadata {
                    title: if fetched.title.is_empty() { req.meta.title.clone() } else { Some(fetched.title) },
                    artist: if fetched.uploader.is_empty() { req.meta.artist.clone() } else { Some(fetched.uploader) },
                    album: req.meta.album.clone(),
                    thumbnail: if fetched.thumbnail.is_empty() { req.meta.thumbnail.clone() } else { Some(fetched.thumbnail) },
                    platform: req.meta.platform.clone(),
                    quality: req.meta.quality.clone(),
                }
            } else {
                req.meta.clone()
            }
        } else {
            req.meta.clone()
        };

        self.emitter.emit_download_info(&ipc_contract::DownloadInfoEvent {
            download_id,
            meta: meta.clone(),
        });

        let emitter = self.emitter.clone();
        let active = self.active_downloads.clone();

        tokio::spawn(async move {
            use downloads::yt_dlp::{download_music, music_args_from_settings};
            let yt_dlp_cmd = downloads::yt_dlp::find_yt_dlp_command(None);
            let quality = req.quality.as_deref().unwrap_or("best").to_string();
            let mut args = music_args_from_settings(&req.url, &quality, &settings, false);
            let effective_output_dir = if settings.create_platform_subfolders {
                format!("{}/YouTube Music", req.output_dir)
            } else {
                req.output_dir.clone()
            };
            args.download_path = effective_output_dir.clone();
            tokio::fs::create_dir_all(&effective_output_dir).await.ok();

            let emitter_p = emitter.clone();
            let result = download_music(args, &yt_dlp_cmd, move |p| {
                let spd = if p.speed.is_empty() { None } else { Some(p.speed.clone()) };
                let eta = if p.eta.is_empty() { None } else { Some(p.eta.clone()) };
                emitter_p.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: p.percent,
                    speed: spd,
                    eta,
                    status: "downloading".into(),
                    item_index: p.item_index,
                    item_total: p.item_total,
                });
            }, cancel_flag).await;

            active.remove(&download_id);
            if let Err(e) = result {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 0.0,
                    speed: None,
                    eta: None,
                    status: format!("error: {}", e),
                    item_index: None,
                    item_total: None,
                });
            } else {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 100.0,
                    speed: None,
                    eta: None,
                    status: "completed".into(),
                    item_index: None,
                    item_total: None,
                });
            }
        });

        ipc_contract::StartDownloadResponse {
            download_id,
            success: true,
            error: None,
        }
    }

    pub async fn start_yt_video_download(
        &self,
        req: ipc_contract::StartYtVideoDownloadRequest,
    ) -> ipc_contract::StartDownloadResponse {
        let download_id = self.next_download_id();
        let settings = self.settings.read().await.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_downloads.insert(download_id, cancel_flag.clone());

        let meta = if req.meta.title.is_none() || req.meta.title.as_deref() == Some("") {
            let yt_dlp_cmd = downloads::yt_dlp::find_yt_dlp_command(None);
            if let Ok(fetched) = downloads::yt_dlp::prefetch_metadata(&req.url, &yt_dlp_cmd).await {
                ipc_contract::DownloadMetadata {
                    title: if fetched.title.is_empty() { req.meta.title.clone() } else { Some(fetched.title) },
                    artist: if fetched.uploader.is_empty() { req.meta.artist.clone() } else { Some(fetched.uploader) },
                    album: req.meta.album.clone(),
                    thumbnail: if fetched.thumbnail.is_empty() { req.meta.thumbnail.clone() } else { Some(fetched.thumbnail) },
                    platform: req.meta.platform.clone(),
                    quality: req.meta.quality.clone(),
                }
            } else {
                req.meta.clone()
            }
        } else {
            req.meta.clone()
        };

        self.emitter.emit_download_info(&ipc_contract::DownloadInfoEvent {
            download_id,
            meta: meta.clone(),
        });

        let emitter = self.emitter.clone();
        let active = self.active_downloads.clone();

        tokio::spawn(async move {
            use downloads::yt_dlp::{download_video, video_args_from_settings};
            let yt_dlp_cmd = downloads::yt_dlp::find_yt_dlp_command(None);
            let quality = req.resolution.as_deref().unwrap_or("bestvideo+bestaudio/best").to_string();
            let mut args = video_args_from_settings(&req.url, &quality, &settings, false, false);
            let effective_output_dir = if settings.create_platform_subfolders {
                format!("{}/YouTube", req.output_dir)
            } else {
                req.output_dir.clone()
            };
            args.download_path = effective_output_dir.clone();
            tokio::fs::create_dir_all(&effective_output_dir).await.ok();
            if let Some(f) = req.format {
                args.merge_output_format = Some(f);
            }

            let emitter_p = emitter.clone();
            let result = download_video(args, &yt_dlp_cmd, move |p| {
                let spd = if p.speed.is_empty() { None } else { Some(p.speed.clone()) };
                let eta = if p.eta.is_empty() { None } else { Some(p.eta.clone()) };
                emitter_p.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: p.percent,
                    speed: spd,
                    eta,
                    status: "downloading".into(),
                    item_index: p.item_index,
                    item_total: p.item_total,
                });
            }, cancel_flag).await;

            active.remove(&download_id);
            if let Err(e) = result {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 0.0,
                    speed: None,
                    eta: None,
                    status: format!("error: {}", e),
                    item_index: None,
                    item_total: None,
                });
            } else {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 100.0,
                    speed: None,
                    eta: None,
                    status: "completed".into(),
                    item_index: None,
                    item_total: None,
                });
            }
        });

        ipc_contract::StartDownloadResponse {
            download_id,
            success: true,
            error: None,
        }
    }

    pub async fn start_spotify_download(
        &self,
        req: ipc_contract::StartSpotifyDownloadRequest,
    ) -> ipc_contract::StartDownloadResponse {
        let download_id = self.next_download_id();
        let settings = self.settings.read().await.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_downloads.insert(download_id, cancel_flag.clone());

        let meta = if req.meta.title.is_none() || req.meta.title.as_deref() == Some("") {
            self.try_prefetch_spotify_meta(&req.url, &settings).await
                .unwrap_or_else(|| req.meta.clone())
        } else {
            req.meta.clone()
        };

        self.emitter.emit_download_info(&ipc_contract::DownloadInfoEvent {
            download_id,
            meta: meta.clone(),
        });

        let emitter = self.emitter.clone();
        let active = self.active_downloads.clone();
        let config_path = settings::spotify_config_path(&self.user_data);

        tokio::spawn(async move {
            let _ = downloads::gamrip::write_votify_config(&settings, &config_path).await;

            let result = downloads::gamrip::download_with_votify(
                &settings,
                &req.url,
                &config_path,
                {
                    let emitter_p = emitter.clone();
                    move |p: downloads::gamrip::BatchProgress| {
                        emitter_p.emit_progress(&ipc_contract::DownloadProgressEvent {
                            download_id,
                            percent: p.percent,
                            speed: None,
                            eta: None,
                            status: if p.current_track.is_empty() { "downloading".into() } else { p.current_track.clone() },
                            item_index: Some(p.completed),
                            item_total: Some(p.total),
                        });
                    }
                },
                |_| {},
                cancel_flag,
            )
            .await;

            active.remove(&download_id);
            if let Err(e) = result {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 0.0,
                    speed: None,
                    eta: None,
                    status: format!("error: {}", e),
                    item_index: None,
                    item_total: None,
                });
            } else {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 100.0,
                    speed: None,
                    eta: None,
                    status: "completed".into(),
                    item_index: None,
                    item_total: None,
                });
            }
        });

        ipc_contract::StartDownloadResponse {
            download_id,
            success: true,
            error: None,
        }
    }

    pub async fn start_apple_download(
        &self,
        req: ipc_contract::StartAppleDownloadRequest,
    ) -> ipc_contract::StartDownloadResponse {
        let download_id = self.next_download_id();
        let settings = self.settings.read().await.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_downloads.insert(download_id, cancel_flag.clone());

        let meta = if req.meta.title.is_none() || req.meta.title.as_deref() == Some("") {
            self.try_prefetch_apple_meta(&req.url).await
                .unwrap_or_else(|| req.meta.clone())
        } else {
            req.meta.clone()
        };

        self.emitter.emit_download_info(&ipc_contract::DownloadInfoEvent {
            download_id,
            meta: meta.clone(),
        });

        let emitter = self.emitter.clone();
        let active = self.active_downloads.clone();
        let config_path = settings::apple_config_path(&self.user_data);

        tokio::spawn(async move {
            let _ = downloads::gamrip::write_gamdl_config(&settings, &config_path).await;

            let result = downloads::gamrip::download_with_gamdl(
                &settings,
                &req.url,
                &config_path,
                {
                    let emitter_p = emitter.clone();
                    move |p: downloads::gamrip::BatchProgress| {
                        emitter_p.emit_progress(&ipc_contract::DownloadProgressEvent {
                            download_id,
                            percent: p.percent,
                            speed: None,
                            eta: None,
                            status: if p.current_track.is_empty() { "downloading".into() } else { p.current_track.clone() },
                            item_index: Some(p.completed),
                            item_total: Some(p.total),
                        });
                    }
                },
                |_| {},
                cancel_flag,
            )
            .await;

            active.remove(&download_id);
            if let Err(e) = result {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 0.0,
                    speed: None,
                    eta: None,
                    status: format!("error: {}", e),
                    item_index: None,
                    item_total: None,
                });
            } else {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 100.0,
                    speed: None,
                    eta: None,
                    status: "completed".into(),
                    item_index: None,
                    item_total: None,
                });
            }
        });

        ipc_contract::StartDownloadResponse {
            download_id,
            success: true,
            error: None,
        }
    }

    pub async fn start_qobuz_download(
        &self,
        req: ipc_contract::StartQobuzDownloadRequest,
    ) -> ipc_contract::StartDownloadResponse {
        let download_id = self.next_download_id();
        let settings = self.settings.read().await.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_downloads.insert(download_id, cancel_flag.clone());

        let meta = if req.meta.title.is_none() || req.meta.title.as_deref() == Some("") {
            self.try_prefetch_streamrip_meta(&req.url, &settings).await.unwrap_or(req.meta.clone())
        } else {
            req.meta.clone()
        };

        self.emitter.emit_download_info(&ipc_contract::DownloadInfoEvent {
            download_id,
            meta: meta.clone(),
        });

        let emitter = self.emitter.clone();
        let active = self.active_downloads.clone();
        tokio::spawn(async move {
            let mut settings = settings;
            settings.download_location = req.output_dir.clone();
            if let Some(q) = req.quality {
                settings.qobuz_quality = q;
            }
            let qobuz_client = match streamrip::qobuz_client::QobuzClient::authenticate(&settings).await {
                Ok(c) => c,
                Err(e) => {
                    active.remove(&download_id);
                    emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                        download_id,
                        percent: 0.0,
                        speed: None,
                        eta: None,
                        status: format!("error: {}", e),
                        item_index: None,
                        item_total: None,
                    });
                    return;
                }
            };

            let cancel_clone = cancel_flag.clone();
            let sr_clients = streamrip::orchestrator::StreamripClients {
                qobuz: Some(qobuz_client),
                tidal: None,
                deezer: None,
            };
            let result = tokio::select! {
                r = streamrip::orchestrator::download_url(
                    &req.url,
                    &settings,
                    &sr_clients,
                    {
                        let emitter_p = emitter.clone();
                        move |done: u64, total: u64| {
                            let pct = if total > 0 { (done as f32 / total as f32) * 100.0 } else { 0.0 };
                            emitter_p.emit_progress(&ipc_contract::DownloadProgressEvent {
                                download_id,
                                percent: pct,
                                speed: None,
                                eta: None,
                                status: "downloading".into(),
                                item_index: None,
                                item_total: None,
                            });
                        }
                    },
                ) => r,
                _ = async {
                    loop {
                        if cancel_clone.load(std::sync::atomic::Ordering::Relaxed) { break; }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                } => Err(MhError::Cancelled),
            };

            active.remove(&download_id);
            if let Err(e) = result {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 0.0,
                    speed: None,
                    eta: None,
                    status: format!("error: {}", e),
                    item_index: None,
                    item_total: None,
                });
            } else {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 100.0,
                    speed: None,
                    eta: None,
                    status: "completed".into(),
                    item_index: None,
                    item_total: None,
                });
            }
        });

        ipc_contract::StartDownloadResponse {
            download_id,
            success: true,
            error: None,
        }
    }

    pub async fn start_deezer_download(
        &self,
        req: ipc_contract::StartDeezerDownloadRequest,
    ) -> ipc_contract::StartDownloadResponse {
        let download_id = self.next_download_id();
        let settings = self.settings.read().await.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_downloads.insert(download_id, cancel_flag.clone());

        let meta = if req.meta.title.is_none() || req.meta.title.as_deref() == Some("") {
            self.try_prefetch_streamrip_meta(&req.url, &settings).await.unwrap_or(req.meta.clone())
        } else {
            req.meta.clone()
        };

        self.emitter.emit_download_info(&ipc_contract::DownloadInfoEvent {
            download_id,
            meta: meta.clone(),
        });

        let emitter = self.emitter.clone();
        let active = self.active_downloads.clone();
        tokio::spawn(async move {
            let mut settings = settings;
            settings.download_location = req.output_dir.clone();
            if let Some(q) = req.quality {
                settings.deezer_quality = match q {
                    2 => "FLAC".into(),
                    1 => "MP3_320".into(),
                    _ => "MP3_128".into(),
                };
            }
            if settings.deezer_arl.trim().is_empty() {
                active.remove(&download_id);
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 0.0,
                    speed: None,
                    eta: None,
                    status: "error: Deezer ARL not set. Go to Settings → Deezer and paste your ARL token.".into(),
                    item_index: None,
                    item_total: None,
                });
                return;
            }
            let deezer_client = match streamrip::deezer_client::DeezerClient::new(&settings.deezer_arl) {
                Ok(c) => c,
                Err(e) => {
                    active.remove(&download_id);
                    emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                        download_id,
                        percent: 0.0,
                        speed: None,
                        eta: None,
                        status: format!("error: {}", e),
                        item_index: None,
                        item_total: None,
                    });
                    return;
                }
            };
            if let Err(e) = deezer_client.authenticate().await {
                active.remove(&download_id);
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 0.0,
                    speed: None,
                    eta: None,
                    status: format!("error: {}", e),
                    item_index: None,
                    item_total: None,
                });
                return;
            }

            let cancel_clone = cancel_flag.clone();
            let sr_clients = streamrip::orchestrator::StreamripClients {
                deezer: Some(deezer_client),
                tidal: None,
                qobuz: None,
            };
            let result = tokio::select! {
                r = streamrip::orchestrator::download_url(
                    &req.url,
                    &settings,
                    &sr_clients,
                    {
                        let emitter_p = emitter.clone();
                        move |done: u64, total: u64| {
                            let pct = if total > 0 { (done as f32 / total as f32) * 100.0 } else { 0.0 };
                            emitter_p.emit_progress(&ipc_contract::DownloadProgressEvent {
                                download_id,
                                percent: pct,
                                speed: None,
                                eta: None,
                                status: "downloading".into(),
                                item_index: None,
                                item_total: None,
                            });
                        }
                    },
                ) => r,
                _ = async {
                    loop {
                        if cancel_clone.load(std::sync::atomic::Ordering::Relaxed) { break; }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                } => Err(MhError::Cancelled),
            };

            active.remove(&download_id);
            if let Err(e) = result {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 0.0,
                    speed: None,
                    eta: None,
                    status: format!("error: {}", e),
                    item_index: None,
                    item_total: None,
                });
            } else {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 100.0,
                    speed: None,
                    eta: None,
                    status: "completed".into(),
                    item_index: None,
                    item_total: None,
                });
            }
        });

        ipc_contract::StartDownloadResponse {
            download_id,
            success: true,
            error: None,
        }
    }

    pub async fn start_tidal_download(
        &self,
        req: ipc_contract::StartTidalDownloadRequest,
    ) -> ipc_contract::StartDownloadResponse {
        let download_id = self.next_download_id();
        let settings = self.settings.read().await.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_downloads.insert(download_id, cancel_flag.clone());

        let meta = if req.meta.title.is_none() || req.meta.title.as_deref() == Some("") {
            self.try_prefetch_streamrip_meta(&req.url, &settings).await.unwrap_or(req.meta.clone())
        } else {
            req.meta.clone()
        };

        self.emitter.emit_download_info(&ipc_contract::DownloadInfoEvent {
            download_id,
            meta: meta.clone(),
        });

        let emitter = self.emitter.clone();
        let active = self.active_downloads.clone();
        tokio::spawn(async move {
            let mut settings = settings;
            settings.download_location = req.output_dir.clone();
            let tidal_client = match streamrip::tidal_client::TidalClient::authenticate(&settings).await {
                Ok(c) => c,
                Err(e) => {
                    active.remove(&download_id);
                    emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                        download_id,
                        percent: 0.0,
                        speed: None,
                        eta: None,
                        status: format!("error: {}", e),
                        item_index: None,
                        item_total: None,
                    });
                    return;
                }
            };

            let cancel_clone = cancel_flag.clone();
            let sr_clients = streamrip::orchestrator::StreamripClients {
                tidal: Some(tidal_client),
                deezer: None,
                qobuz: None,
            };
            let result = tokio::select! {
                r = streamrip::orchestrator::download_url(
                    &req.url,
                    &settings,
                    &sr_clients,
                    {
                        let emitter_p = emitter.clone();
                        move |done: u64, total: u64| {
                            let pct = if total > 0 { (done as f32 / total as f32) * 100.0 } else { 0.0 };
                            emitter_p.emit_progress(&ipc_contract::DownloadProgressEvent {
                                download_id,
                                percent: pct,
                                speed: None,
                                eta: None,
                                status: "downloading".into(),
                                item_index: None,
                                item_total: None,
                            });
                        }
                    },
                ) => r,
                _ = async {
                    loop {
                        if cancel_clone.load(std::sync::atomic::Ordering::Relaxed) { break; }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                } => Err(MhError::Cancelled),
            };

            active.remove(&download_id);
            if let Err(e) = result {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 0.0,
                    speed: None,
                    eta: None,
                    status: format!("error: {}", e),
                    item_index: None,
                    item_total: None,
                });
            } else {
                emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                    download_id,
                    percent: 100.0,
                    speed: None,
                    eta: None,
                    status: "completed".into(),
                    item_index: None,
                    item_total: None,
                });
            }
        });

        ipc_contract::StartDownloadResponse {
            download_id,
            success: true,
            error: None,
        }
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
        let git_ok = which_binary("git");

        let (yt_dlp_ok, votify_ok, gamdl_ok, pywidevine_ok, bento4_ok) = if python_ok {
            let pip_list = tokio::process::Command::new(venv_manager::get_venv_python())
                .args(["-m", "pip", "list"])
                .output()
                .await
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_default();

            let yt_dlp = pip_list.contains("yt-dlp");
            let votify = pip_list.contains("votify");
            let gamdl  = pip_list.contains("gamdl");
            let pywv   = pip_list.contains("pywidevine");
            let bento4 = installers::bento4::get_bento4_bin_dir().exists();
            (yt_dlp, votify, gamdl, pywv, bento4)
        } else {
            (false, false, false, false, false)
        };

        ipc_contract::CheckDepsResponse {
            ffmpeg: ffmpeg_ok,
            python: python_ok,
            git: git_ok,
            yt_dlp: yt_dlp_ok,
            votify: votify_ok,
            gamdl: gamdl_ok,
            pywidevine: pywidevine_ok,
            bento4: bento4_ok,
        }
    }

    pub async fn install_dep(
        &self,
        req: ipc_contract::InstallDepRequest,
    ) -> ipc_contract::InstallDepResponse {
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
            "git" => {
                let r = installers::git::download_and_install_git(|pct, msg| make_progress(pct, msg)).await;
                r
            }
            "python" => {
                venv_manager::ensure_venv(|pct, msg| make_progress(pct, msg)).await
            }
            "ffmpeg" => {
                installers::ffmpeg::download_and_install_ffmpeg(|pct, msg| make_progress(pct, msg)).await
            }
            "yt_dlp" | "ytdlp" => {
                let py = venv_manager::get_venv_python();
                tokio::process::Command::new(py)
                    .args(["-m", "pip", "install", "--upgrade", "yt-dlp", "isodate"])
                    .output()
                    .await
                    .map(|_| ())
                    .map_err(|e| MhError::Subprocess(e.to_string()))
            }
            "apple" | "gamdl" => {
                let _ = installers::bento4::download_and_install_bento4(|pct, msg| make_progress(pct, msg)).await;
                let py = venv_manager::get_venv_python();
                tokio::process::Command::new(py)
                    .args(["-m", "pip", "install", "--upgrade", "gamdl"])
                    .output()
                    .await
                    .map(|_| ())
                    .map_err(|e| MhError::Subprocess(e.to_string()))
            }
            "spotify" | "votify" => {
                let _ = installers::bento4::download_and_install_bento4(|pct, msg| make_progress(pct, msg)).await;
                let py = venv_manager::get_venv_python();
                tokio::process::Command::new(py)
                    .args(["-m", "pip", "install", "--upgrade", "votify"])
                    .output()
                    .await
                    .map(|_| ())
                    .map_err(|e| MhError::Subprocess(e.to_string()))
            }
            "pywidevine" => {
                let py = venv_manager::get_venv_python();
                tokio::process::Command::new(py)
                    .args(["-m", "pip", "install", "--upgrade", "pywidevine"])
                    .output()
                    .await
                    .map(|_| ())
                    .map_err(|e| MhError::Subprocess(e.to_string()))
            }
            "qobuz" | "deezer" | "tidal" | "ytmusic" | "googleapi" | "pyapplemusicapi" => {
                make_progress(100, "built-in (native Rust)");
                Ok(())
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

    pub async fn get_dependency_versions(&self) -> ipc_contract::GetDependencyVersionsResponse {
        let mut versions = std::collections::HashMap::new();

        for bin in &["ffmpeg", "git"] {
            if let Some(v) = binary_version(bin) {
                versions.insert(bin.to_string(), v);
            }
        }

        if let Ok(v) = venv_manager::find_system_python().await {
            versions.insert("python".to_string(), v);
        }

        if venv_manager::is_venv_ready() {
            let py = venv_manager::get_venv_python();
            for pkg in &["yt-dlp", "gamdl", "votify", "pywidevine"] {
                let out = tokio::process::Command::new(&py)
                    .args(["-m", "pip", "show", pkg])
                    .output()
                    .await;
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
    std::process::Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn binary_version(name: &str) -> Option<String> {
    let output = std::process::Command::new(name)
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().next().map(|l| l.trim().to_string())
}



pub mod defaults;
pub mod download_order;
pub mod errors;
pub mod orpheus;
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
            stdin_senders: Arc::new(DashMap::new()),
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
                            if !settings.tidal_refresh_token.is_empty() {
                                if let Ok(tidal_client) = self.authenticate_tidal(&settings).await {
                                    if let Ok(r) = client
                                        .search_v1(
                                            &req.query,
                                            map_tidal_type(&req.search_type),
                                            &settings.tidal_country_code,
                                            &tidal_client.access_token,
                                        )
                                        .await
                                    {
                                        return Ok(r);
                                    }
                                }
                            }
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

                let info = apis::yt_audio_stream::get_video_stream_info(&req.url, None).await?;

                let server = self
                    .streaming_server
                    .as_ref()
                    .ok_or_else(|| MhError::Other("Streaming server not running".into()))?;
                let id = uuid::Uuid::new_v4().to_string();

                if info.is_live {
                    let hls_url = info.video.url.clone();
                    let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, String>>(32);
                    tokio::spawn(async move {
                        use tokio::io::AsyncReadExt;
                        let ffmpeg_bin = venv_manager::resolve_ffmpeg();
                        let mut child = match tokio::process::Command::new(&ffmpeg_bin)
                            .args([
                                "-loglevel", "error",
                                "-i", &hls_url,
                                "-c", "copy",
                                "-f", "mpegts",
                                "pipe:1",
                            ])
                            .stdout(Stdio::piped())
                            .stderr(Stdio::null())
                            .spawn()
                        {
                            Ok(c) => c,
                            Err(e) => { let _ = tx.send(Err(format!("ffmpeg: {}", e))).await; return; }
                        };
                        if let Some(mut stdout) = child.stdout.take() {
                            let mut buf = vec![0u8; 65_536];
                            loop {
                                match stdout.read(&mut buf).await {
                                    Ok(0) => break,
                                    Ok(n) => {
                                        if tx.send(Ok(bytes::Bytes::copy_from_slice(&buf[..n]))).await.is_err() { break; }
                                    }
                                    Err(e) => { let _ = tx.send(Err(e.to_string())).await; break; }
                                }
                            }
                        }
                        let _ = child.wait().await;
                    });
                    let stream_url = server.register_stream_progressive(&id, rx, "video/mp2t");
                    return Ok(ipc_contract::PlayMediaResponse {
                        stream_url,
                        platform: platform.to_string(),
                        duration_sec: None,
                        media_type: Some("video".to_string()),
                        is_live: true,
                    });
                }

                let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, String>>(16);

                if info.video.url == info.audio.url {
                    let url = info.video.url.clone();
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
                    let video_url = info.video.url.clone();
                    let audio_url = info.audio.url.clone();
                    tokio::spawn(async move {
                        use tokio::io::AsyncReadExt;
                        let ffmpeg_bin = venv_manager::resolve_ffmpeg();
                        let mut child = match tokio::process::Command::new(&ffmpeg_bin)
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

                let stream_url = server.register_stream_progressive(&id, rx, "video/mp4");

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url,
                    platform: platform.to_string(),
                    duration_sec: None,
                    media_type: Some("video".to_string()),
                    is_live: false,
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
                    is_live: false,
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
                    is_live: false,
                })
            }

            "tidal" => {
                let client = self.authenticate_tidal(&settings).await?;
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
                    is_live: false,
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
                let (url_str, ext, filesize) = client.get_stream_url(&track_id, 3).await?;
                let mime_type: &'static str = if ext == "flac" { "audio/flac" } else { "audio/mpeg" };

                let server = self
                    .streaming_server
                    .as_ref()
                    .ok_or_else(|| MhError::Other("Streaming server not running".into()))?;
                let id = uuid::Uuid::new_v4().to_string();

                let stream_url = server.register_stream_deezer(&id, url_str, track_id.clone(), filesize, mime_type);

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url,
                    platform: "deezer".to_string(),
                    duration_sec: None,
                    media_type: Some("audio".to_string()),
                    is_live: false,
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
                let cdn_url = client.get_file_url(&track_id, 27).await?;
                let auth_headers = client.api_headers()?;

                let server = self
                    .streaming_server
                    .as_ref()
                    .ok_or_else(|| MhError::Other("Streaming server not running".into()))?;
                let id = uuid::Uuid::new_v4().to_string();
                let stream_url = server.register_stream_proxied(&id, cdn_url, auth_headers, "audio/flac");

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url,
                    platform: "qobuz".to_string(),
                    duration_sec: None,
                    media_type: Some("audio".to_string()),
                    is_live: false,
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
                    is_live: false,
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
                        is_live: false,
                    });
                }

                Ok(ipc_contract::PlayMediaResponse {
                    stream_url: req.url,
                    platform: platform.to_string(),
                    duration_sec: None,
                    media_type: None,
                    is_live: false,
                })
            }
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
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.active_downloads.insert(download_id, cancel_flag.clone());

        let meta = ipc_contract::DownloadMetadata {
            title: req.title.clone(),
            artist: req.artist.clone(),
            album: req.album.clone(),
            thumbnail: req.thumbnail.clone(),
            platform: Some(req.module_id.clone()),
            quality: None,
        };
        self.emitter.emit_download_info(&ipc_contract::DownloadInfoEvent { download_id, meta });

        if !orpheus::is_orpheus_installed() {
            self.active_downloads.remove(&download_id);
            self.emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                download_id, percent: 0.0, speed: None, eta: None,
                status: "error: OrpheusDL is not installed. Go to Updates → OrpheusDL and install it.".into(),
                item_index: None, item_total: None,
            });
            return ipc_contract::StartDownloadResponse { download_id, success: false, error: Some("OrpheusDL not installed".into()) };
        }

        if !orpheus::is_module_installed(&req.module_id) {
            self.active_downloads.remove(&download_id);
            self.emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
                download_id, percent: 0.0, speed: None, eta: None,
                status: format!("error: OrpheusDL module '{}' is not installed. Go to Updates → Modules.", req.module_id),
                item_index: None, item_total: None,
            });
            return ipc_contract::StartDownloadResponse { download_id, success: false, error: Some(format!("Module {} not installed", req.module_id)) };
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
            let _ = orpheus::run_orpheus_download(&url, &output_dir, &module_id, download_id, &settings, cancel_flag, emitter, stdin_rx).await;
            active.remove(&download_id);
            stdin_senders.remove(&download_id);
        });

        ipc_contract::StartDownloadResponse { download_id, success: true, error: None }
    }
}

impl BackendState {
    pub async fn send_process_stdin(&self, req: ipc_contract::SendProcessStdinRequest) -> ipc_contract::SendProcessStdinResponse {
        if let Some(tx) = self.stdin_senders.get(&req.download_id) {
            let success = tx.send(req.input).await.is_ok();
            ipc_contract::SendProcessStdinResponse { success }
        } else {
            ipc_contract::SendProcessStdinResponse { success: false }
        }
    }

    async fn authenticate_tidal(&self, settings: &Settings) -> MhResult<streamrip::tidal_client::TidalClient> {
        let old_token = settings.tidal_access_token.clone();
        let client = streamrip::tidal_client::TidalClient::authenticate(settings).await?;
        if client.access_token != old_token {
            let mut s = self.settings.write().await;
            s.tidal_access_token = client.access_token.clone();
            s.tidal_token_expiry = client.token_expiry.to_string();
            settings::save_settings(&s, &self.user_data).await.ok();
        }
        Ok(client)
    }

    pub async fn get_lyrics(
        &self,
        req: ipc_contract::GetLyricsRequest,
    ) -> MhResult<ipc_contract::GetLyricsResponse> {
        let settings = self.settings.read().await.clone();
        let platform = req.platform.as_str();

        let empty = || ipc_contract::GetLyricsResponse {
            synced: None, plain: None, word_synced: None,
        };

        let native_result = match platform {
            "tidal" => {
                match self.authenticate_tidal(&settings).await {
                    Err(_) => empty(),
                    Ok(client) => {
                        match extract_tidal_track_id(&req.url) {
                            None => empty(),
                            Some(track_id) => {
                                if let Some(lyr) = client.fetch_lyrics(&track_id).await {
                                    let plain = lyr["lyrics"].as_str().map(|s| s.to_string());
                                    let synced = lyr["subtitles"].as_str().map(|s| s.to_string());
                                    ipc_contract::GetLyricsResponse { synced, plain, word_synced: None }
                                } else {
                                    empty()
                                }
                            }
                        }
                    }
                }
            }

            "deezer" => {
                if settings.deezer_arl.is_empty() {
                    empty()
                } else {
                    match streamrip::deezer_client::DeezerClient::new(&settings.deezer_arl) {
                        Err(_) => empty(),
                        Ok(client) => {
                            if client.authenticate().await.is_err() {
                                empty()
                            } else {
                                let track_id = streamrip::orchestrator::extract_platform_id(
                                    &req.url,
                                    streamrip::orchestrator::Platform::Deezer,
                                    streamrip::orchestrator::ContentType::Track,
                                );
                                match track_id {
                                    None => empty(),
                                    Some(track_id) => {
                                        let word_synced = client.get_word_lyrics(&track_id).await;
                                        match client.get_lyrics(&track_id).await {
                                            Ok(lyr) => {
                                                let ldata = &lyr["results"];
                                                let has_error = lyr["error"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
                                                if !has_error {
                                                    let plain = ldata["LYRICS_TEXT"].as_str().map(|s| s.to_string());
                                                    let synced = ldata["LYRICS_SYNC_JSON"].as_array().and_then(|arr| {
                                                        if arr.is_empty() { return None; }
                                                        let lines: Vec<String> = arr.iter()
                                                            .filter_map(deezer_sync_line_to_lrc)
                                                            .collect();
                                                        if lines.is_empty() { None } else { Some(lines.join("\n")) }
                                                    });
                                                    ipc_contract::GetLyricsResponse { synced, plain, word_synced }
                                                } else {
                                                    ipc_contract::GetLyricsResponse { synced: None, plain: None, word_synced }
                                                }
                                            }
                                            Err(_) => ipc_contract::GetLyricsResponse { synced: None, plain: None, word_synced },
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            "spotify" => {
                let track_id = extract_spotify_id(&req.url).unwrap_or_default();
                if track_id.is_empty() {
                    empty()
                } else {
                    let token_opt = {
                        let librespot = self.librespot.read().await;
                        librespot.cached_access_token()
                    };
                    match token_opt {
                    None => empty(),
                    Some(token) => {
                    let http = http_client::build_client()?;
                    let resp = http
                        .get(&format!("https://spclient.wg.spotify.com/color-lyrics/v2/track/{}", track_id))
                        .header("Authorization", format!("Bearer {}", token))
                        .header("App-Platform", "WebPlayer")
                        .header("Accept", "application/json")
                        .send()
                        .await;
                    let mut result = empty();
                    if let Ok(r) = resp {
                        if r.status().is_success() {
                            if let Ok(body) = r.json::<serde_json::Value>().await {
                                let lyrics_obj = &body["lyrics"];
                                let synced_lines = lyrics_obj["lines"].as_array();
                                if let Some(lines) = synced_lines {
                                    let lrc: Vec<String> = lines.iter().filter_map(|l| {
                                        let ms: u64 = l["startTimeMs"].as_str()
                                            .and_then(|s| s.parse().ok())
                                            .or_else(|| l["startTimeMs"].as_u64())
                                            .or_else(|| l["startTimeMs"].as_f64().map(|f| f as u64))?;
                                        let text = l["words"].as_str().unwrap_or("");
                                        Some(ms_to_lrc_stamp(ms, text))
                                    }).collect();
                                    let plain: Vec<String> = lines.iter().filter_map(|l| {
                                        l["words"].as_str().map(|s| s.to_string())
                                    }).collect();

                                    let word_synced = {
                                        let mut wlines: Vec<serde_json::Value> = Vec::new();
                                        for l in lines {
                                            if let Some(syls) = l["syllables"].as_array() {
                                                if syls.is_empty() { continue; }
                                                let start_ms: f64 = l["startTimeMs"].as_str()
                                                    .and_then(|s| s.parse().ok())
                                                    .or_else(|| l["startTimeMs"].as_f64())
                                                    .unwrap_or(0.0);
                                                let end_ms: f64 = l["endTimeMs"].as_str()
                                                    .and_then(|s| s.parse().ok())
                                                    .or_else(|| l["endTimeMs"].as_f64())
                                                    .unwrap_or(0.0);
                                                let text = l["words"].as_str().unwrap_or("");
                                                let words: Vec<serde_json::Value> = syls.iter().filter_map(|s| {
                                                    let st = s["startTimeMs"].as_str()
                                                        .and_then(|v| v.parse::<f64>().ok())
                                                        .or_else(|| s["startTimeMs"].as_f64())
                                                        .unwrap_or(0.0);
                                                    let en = s["endTimeMs"].as_str()
                                                        .and_then(|v| v.parse::<f64>().ok())
                                                        .or_else(|| s["endTimeMs"].as_f64())
                                                        .unwrap_or(st);
                                                    let w = s["chars"].as_str()
                                                        .or_else(|| s["text"].as_str())
                                                        .or_else(|| s["words"].as_str())
                                                        .unwrap_or("");
                                                    if w.is_empty() { return None; }
                                                    Some(serde_json::json!({
                                                        "start": st / 1000.0,
                                                        "end": en / 1000.0,
                                                        "text": w
                                                    }))
                                                }).collect();
                                                if !words.is_empty() {
                                                    wlines.push(serde_json::json!({
                                                        "startTime": start_ms / 1000.0,
                                                        "endTime": if end_ms > 0.0 { end_ms / 1000.0 } else {
                                                            words.last().and_then(|w| w["end"].as_f64()).unwrap_or(start_ms / 1000.0)
                                                        },
                                                        "text": text,
                                                        "words": words
                                                    }));
                                                }
                                            }
                                        }
                                        if wlines.is_empty() { None }
                                        else { serde_json::to_string(&wlines).ok() }
                                    };

                                    result = ipc_contract::GetLyricsResponse {
                                        synced: if lrc.is_empty() { None } else { Some(lrc.join("\n")) },
                                        plain: if plain.is_empty() { None } else { Some(plain.join("\n")) },
                                        word_synced,
                                    };
                                }
                            }
                        }
                    }
                    result
                    }
                    }
                }
            }

            "applemusic" => {
                let apple = self.apple_music.read().await;
                if !apple.is_configured() {
                    empty()
                } else {
                    match apple.fetch_lyrics(&req.url).await {
                        Some((synced, plain, word_synced)) => ipc_contract::GetLyricsResponse { synced, plain, word_synced },
                        None => empty(),
                    }
                }
            }

            "youtubeMusic" | "youtubemusic" | "ytmusic" => {
                let video_id = extract_yt_video_id(&req.url).unwrap_or_default();
                if video_id.is_empty() {
                    empty()
                } else {
                    match apis::ytmusic_search_api::YtMusicClient::init().await {
                        Ok(client) => {
                            let plain = client.fetch_lyrics(&video_id).await;
                            ipc_contract::GetLyricsResponse { synced: None, plain, word_synced: None }
                        }
                        Err(_) => empty(),
                    }
                }
            }

            _ => {
                empty()
            }
        };

        if native_result.synced.is_some() || native_result.plain.is_some() || native_result.word_synced.is_some() {
            return Ok(native_result);
        }

        if let Some(deezer_wbw) = fetch_deezer_word_lyrics(&req.title, &req.artist, &settings).await {
            return Ok(deezer_wbw);
        }

        if let Some(lrclib_result) = fetch_lrclib_lyrics(&req.title, &req.artist, req.duration).await {
            return Ok(lrclib_result);
        }

        Ok(native_result)
    }
}

async fn fetch_lrclib_lyrics(
    title: &str,
    artist: &str,
    duration: Option<f64>,
) -> Option<ipc_contract::GetLyricsResponse> {
    if title.is_empty() {
        return None;
    }
    let http = http_client::build_client().ok()?;
    let mut url = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}",
        url::form_urlencoded::byte_serialize(title.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(artist.as_bytes()).collect::<String>(),
    );
    if let Some(dur) = duration {
        url.push_str(&format!("&duration={}", dur.round() as u64));
    }
    let resp = http
        .get(&url)
        .header("User-Agent", "MediaHarbor/1.0")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    let synced = body["syncedLyrics"].as_str().map(|s| s.to_string());
    let plain = body["plainLyrics"].as_str().map(|s| s.to_string());
    if synced.is_none() && plain.is_none() {
        return None;
    }
    Some(ipc_contract::GetLyricsResponse { synced, plain, word_synced: None })
}

async fn fetch_deezer_word_lyrics(
    title: &str,
    artist: &str,
    settings: &defaults::Settings,
) -> Option<ipc_contract::GetLyricsResponse> {
    if title.is_empty() { return None; }

    let http = http_client::build_client().ok()?;
    let query = format!("{} {}", title, artist);
    let search_url = format!(
        "https://api.deezer.com/search?q={}&limit=5",
        url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>(),
    );
    let search_resp = http.get(&search_url)
        .header("User-Agent", http_client::UA_MOZILLA)
        .send().await.ok()?;
    let search_body: serde_json::Value = search_resp.json().await.ok()?;
    let track_id = search_body["data"].as_array()
        .and_then(|arr| arr.first())
        .and_then(|t| t["id"].as_u64())
        .map(|id| id.to_string())?;

    let arl = &settings.deezer_arl;
    if !arl.is_empty() {
        let client = streamrip::deezer_client::DeezerClient::new(arl).ok()?;
        client.authenticate().await.ok()?;
        let word_synced = client.get_word_lyrics(&track_id).await;

        let gw_result = client.get_lyrics(&track_id).await.ok();
        let (synced, plain) = if let Some(ref lyr) = gw_result {
            let ldata = &lyr["results"];
            let p = ldata["LYRICS_TEXT"].as_str().map(|s| s.to_string());
            let s = ldata["LYRICS_SYNC_JSON"].as_array().and_then(|arr| {
                if arr.is_empty() { return None; }
                let lines: Vec<String> = arr.iter()
                    .filter_map(deezer_sync_line_to_lrc)
                    .collect();
                if lines.is_empty() { None } else { Some(lines.join("\n")) }
            });
            (s, p)
        } else {
            (None, None)
        };

        if word_synced.is_some() || synced.is_some() || plain.is_some() {
            return Some(ipc_contract::GetLyricsResponse { synced, plain, word_synced });
        }
        return None;
    }

    let word_synced = fetch_public_word_lyrics(&track_id).await;
    if word_synced.is_some() {
        return Some(ipc_contract::GetLyricsResponse { synced: None, plain: None, word_synced });
    }

    None
}

fn make_log_buffer() -> (std::sync::Arc<std::sync::Mutex<Vec<String>>>, impl Fn(String) + Clone + Send + 'static) {
    let buf: std::sync::Arc<std::sync::Mutex<Vec<String>>> = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let buf_c = buf.clone();
    let on_log = move |msg: String| { if let Ok(mut v) = buf_c.lock() { v.push(msg); } };
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

use once_cell::sync::Lazy;

static SPOTIFY_ID_RE: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r"spotify\.com/(?:track|episode|album|artist|playlist)/([a-zA-Z0-9]+)").unwrap()
});
static TIDAL_TRACK_ID_RE: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r"tidal\.com/(?:browse/)?(?:track|album|video)/(\d+)").unwrap()
});
static YT_VIDEO_ID_RE: Lazy<regex::Regex> = Lazy::new(|| {
    regex::Regex::new(r"[?&]v=([a-zA-Z0-9_-]{11})").unwrap()
});

fn extract_spotify_id(url: &str) -> Option<String> {
    SPOTIFY_ID_RE.captures(url)?.get(1).map(|m| m.as_str().to_string())
}

fn extract_tidal_track_id(url: &str) -> Option<String> {
    TIDAL_TRACK_ID_RE.captures(url)?.get(1).map(|m| m.as_str().to_string())
}

fn extract_yt_video_id(url: &str) -> Option<String> {
    if let Some(caps) = YT_VIDEO_ID_RE.captures(url) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }
    if url.len() == 11 && url.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        return Some(url.to_string());
    }
    None
}

fn ms_to_lrc_stamp(ms: u64, line: &str) -> String {
    let m = ms / 60000;
    let s = (ms % 60000) / 1000;
    let cs = (ms % 1000) / 10;
    format!("[{:02}:{:02}.{:02}]{}", m, s, cs, line)
}

fn deezer_sync_line_to_lrc(l: &serde_json::Value) -> Option<String> {
    let line = l["line"].as_str().unwrap_or("");
    if let Some(ts) = l["lrc_timestamp"].as_str() {
        if !ts.is_empty() {
            return Some(format!("{}{}", ts, line));
        }
    }
    let ms = l["milliseconds"].as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| l["milliseconds"].as_u64())
        .or_else(|| l["milliseconds"].as_f64().map(|f| f as u64))
        .or_else(|| l["lrc_timestamp"].as_u64())
        .or_else(|| l["lrc_timestamp"].as_f64().map(|f| f as u64))?;
    Some(ms_to_lrc_stamp(ms, line))
}

async fn fetch_public_word_lyrics(track_id: &str) -> Option<String> {
    let client = streamrip::deezer_client::DeezerClient::new("").ok()?;
    client.get_word_lyrics(track_id).await
}

fn getrandom_bytes(buf: &mut [u8]) -> MhResult<()> {
    getrandom::fill(buf)
        .map_err(|e| MhError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))
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
                    quality:   Some(format!("{} {}", streamrip::deezer_format(&settings.deezer_quality), streamrip::deezer_quality_label(&settings.deezer_quality))),
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
                    quality:   Some(format!("{} {}", if settings.qobuz_quality == 5 { "MP3" } else { "FLAC" }, streamrip::qobuz_quality_label(settings.qobuz_quality as u32))),
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
                    quality:   Some(format!("{} {}", streamrip::tidal_format(settings.tidal_quality), streamrip::tidal_quality_label(settings.tidal_quality))),
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
                std::path::Path::new(&req.output_dir)
                    .join("YouTube Music")
                    .to_string_lossy()
                    .to_string()
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
                std::path::Path::new(&req.output_dir)
                    .join("YouTube")
                    .to_string_lossy()
                    .to_string()
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

        let platform = "qobuz";
        if settings.orpheus_dl
            && settings.orpheus_dl_enabled_modules.split(',').any(|m| m.trim() == platform)
            && orpheus::is_orpheus_installed()
            && orpheus::is_module_installed(platform)
        {
            let emitter = self.emitter.clone();
            let active = self.active_downloads.clone();
            let stdin_senders = self.stdin_senders.clone();
            let url = req.url.clone();
            let output_dir = req.output_dir.clone();
            let mut s = settings.clone();
            if let Some(q) = req.quality { s.qobuz_quality = q; }
            let (stdin_tx, stdin_rx) = tokio::sync::mpsc::channel::<String>(4);
            stdin_senders.insert(download_id, stdin_tx);
            tokio::spawn(async move {
                let _ = orpheus::run_orpheus_download(&url, &output_dir, platform, download_id, &s, cancel_flag, emitter, stdin_rx).await;
                active.remove(&download_id);
                stdin_senders.remove(&download_id);
            });
            return ipc_contract::StartDownloadResponse { download_id, success: true, error: None };
        }

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
            let (log_buf, on_log) = make_log_buffer();
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
                    on_log,
                ) => r,
                _ = async {
                    loop {
                        if cancel_clone.load(std::sync::atomic::Ordering::Relaxed) { break; }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                } => Err(MhError::Cancelled),
            };

            active.remove(&download_id);
            let log_lines = log_buf.lock().map(|v| v.join("\n")).unwrap_or_default();
            emitter.emit_log(&ipc_contract::BackendLogEvent {
                level: if result.is_ok() { "info" } else { "error" }.to_string(),
                source: "streamrip".to_string(),
                title: "Streamrip: Qobuz".to_string(),
                message: log_lines,
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
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

        let platform = "deezer";
        if settings.orpheus_dl
            && settings.orpheus_dl_enabled_modules.split(',').any(|m| m.trim() == platform)
            && orpheus::is_orpheus_installed()
            && orpheus::is_module_installed(platform)
        {
            let emitter = self.emitter.clone();
            let active = self.active_downloads.clone();
            let stdin_senders = self.stdin_senders.clone();
            let url = req.url.clone();
            let output_dir = req.output_dir.clone();
            let mut s = settings.clone();
            if let Some(q) = req.quality {
                s.deezer_quality = match q { 2 => "FLAC".into(), 1 => "MP3_320".into(), _ => "MP3_128".into() };
            }
            let (stdin_tx, stdin_rx) = tokio::sync::mpsc::channel::<String>(4);
            stdin_senders.insert(download_id, stdin_tx);
            tokio::spawn(async move {
                let _ = orpheus::run_orpheus_download(&url, &output_dir, platform, download_id, &s, cancel_flag, emitter, stdin_rx).await;
                active.remove(&download_id);
                stdin_senders.remove(&download_id);
            });
            return ipc_contract::StartDownloadResponse { download_id, success: true, error: None };
        }

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
            let (log_buf, on_log) = make_log_buffer();
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
                    on_log,
                ) => r,
                _ = async {
                    loop {
                        if cancel_clone.load(std::sync::atomic::Ordering::Relaxed) { break; }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                } => Err(MhError::Cancelled),
            };

            active.remove(&download_id);
            let log_lines = log_buf.lock().map(|v| v.join("\n")).unwrap_or_default();
            emitter.emit_log(&ipc_contract::BackendLogEvent {
                level: if result.is_ok() { "info" } else { "error" }.to_string(),
                source: "streamrip".to_string(),
                title: "Streamrip: Deezer".to_string(),
                message: log_lines,
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
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

        let platform = "tidal";
        if settings.orpheus_dl
            && settings.orpheus_dl_enabled_modules.split(',').any(|m| m.trim() == platform)
            && orpheus::is_orpheus_installed()
            && orpheus::is_module_installed(platform)
        {
            let emitter = self.emitter.clone();
            let active = self.active_downloads.clone();
            let stdin_senders = self.stdin_senders.clone();
            let url = req.url.clone();
            let output_dir = req.output_dir.clone();
            let s = settings.clone();
            let (stdin_tx, stdin_rx) = tokio::sync::mpsc::channel::<String>(4);
            stdin_senders.insert(download_id, stdin_tx);
            tokio::spawn(async move {
                let _ = orpheus::run_orpheus_download(&url, &output_dir, platform, download_id, &s, cancel_flag, emitter, stdin_rx).await;
                active.remove(&download_id);
                stdin_senders.remove(&download_id);
            });
            return ipc_contract::StartDownloadResponse { download_id, success: true, error: None };
        }

        let emitter = self.emitter.clone();
        let active = self.active_downloads.clone();
        let settings_arc = self.settings.clone();
        let user_data = self.user_data.clone();
        tokio::spawn(async move {
            let mut settings = settings;
            settings.download_location = req.output_dir.clone();
            let old_token = settings.tidal_access_token.clone();
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
            if tidal_client.access_token != old_token {
                let mut s = settings_arc.write().await;
                s.tidal_access_token = tidal_client.access_token.clone();
                s.tidal_token_expiry = tidal_client.token_expiry.to_string();
                settings::save_settings(&s, &user_data).await.ok();
            }

            let cancel_clone = cancel_flag.clone();
            let sr_clients = streamrip::orchestrator::StreamripClients {
                tidal: Some(tidal_client),
                deezer: None,
                qobuz: None,
            };
            let (log_buf, on_log) = make_log_buffer();
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
                    on_log,
                ) => r,
                _ = async {
                    loop {
                        if cancel_clone.load(std::sync::atomic::Ordering::Relaxed) { break; }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                } => Err(MhError::Cancelled),
            };

            active.remove(&download_id);
            let log_lines = log_buf.lock().map(|v| v.join("\n")).unwrap_or_default();
            emitter.emit_log(&ipc_contract::BackendLogEvent {
                level: if result.is_ok() { "info" } else { "error" }.to_string(),
                source: "streamrip".to_string(),
                title: "Streamrip: Tidal".to_string(),
                message: log_lines,
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
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

        let (yt_dlp_ok, votify_ok, gamdl_ok, bento4_ok) = if python_ok {
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
            let bento4 = installers::bento4::get_bento4_bin_dir().exists();
            (yt_dlp, votify, gamdl, bento4)
        } else {
            (false, false, false, false)
        };

        ipc_contract::CheckDepsResponse {
            ffmpeg: ffmpeg_ok,
            python: python_ok,
            git: git_ok,
            yt_dlp: yt_dlp_ok,
            votify: votify_ok,
            gamdl: gamdl_ok,
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
                    .args(["-m", "pip", "install", "--upgrade", "votify", "pywidevine"])
                    .output()
                    .await
                    .map(|_| ())
                    .map_err(|e| MhError::Subprocess(e.to_string()))
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
            for pkg in &["yt-dlp", "gamdl", "votify"] {
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

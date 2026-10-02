use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;

use crate::auth::credential_health::CredentialsHealth;
use crate::downloads;
use crate::downloads::yt_dlp::DownloadProgress;
use crate::emit_terminal;
use crate::errors::MhResult;
use crate::ipc_contract;
use crate::services::common::download::{
    accept_download, log_sink, AcceptedDownload, DownloadContext, DownloadProvider,
};
use crate::EventEmitter;

fn yt_dlp_level(line: &str) -> &'static str {
    if line.contains("ERROR") || line.contains("error") {
        "error"
    } else {
        "info"
    }
}

fn progress_sink(
    emitter: &Arc<dyn EventEmitter>,
    download_id: u64,
) -> impl Fn(DownloadProgress) + Send + 'static {
    let emitter = emitter.clone();
    move |p: DownloadProgress| {
        emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
            download_id,
            percent: p.percent,
            speed: (!p.speed.is_empty()).then(|| p.speed.clone()),
            eta: (!p.eta.is_empty()).then(|| p.eta.clone()),
            status: "downloading".into(),
            item_index: p.item_index,
            item_total: p.item_total,
            quality: None,
        });
    }
}

async fn resolve_output_dir(req_dir: &str, create_subfolders: bool, sub: &str) -> String {
    let dir = if create_subfolders {
        std::path::Path::new(req_dir)
            .join(sub)
            .to_string_lossy()
            .to_string()
    } else {
        req_dir.to_string()
    };
    tokio::fs::create_dir_all(&dir).await.ok();
    dir
}

async fn finish(
    emitter: &Arc<dyn EventEmitter>,
    active: &Arc<DashMap<u64, Arc<AtomicBool>>>,
    credentials_health: &Arc<CredentialsHealth>,
    download_id: u64,
    result: MhResult<()>,
) {
    active.remove(&download_id);
    if let Err(e) = &result {
        credentials_health
            .note_auth_failure(
                crate::services::common::library::ServicePlatform::Youtube,
                e,
            )
            .await;
    }
    emit_terminal(emitter, download_id, &result);
}

pub struct YtMusicDownloader {
    credentials_health: Arc<CredentialsHealth>,
}

impl YtMusicDownloader {
    pub fn from_state(state: &crate::BackendState) -> Self {
        Self {
            credentials_health: state.credentials_health.clone(),
        }
    }
}

#[async_trait]
impl DownloadProvider for YtMusicDownloader {
    async fn start(
        &self,
        req: Value,
        ctx: &DownloadContext,
        download_id: u64,
    ) -> ipc_contract::StartDownloadResponse {
        let req: ipc_contract::StartYtMusicDownloadRequest =
            match crate::services::common::download::parse_request(req, download_id) {
                Ok(r) => r,
                Err(resp) => return resp,
            };

        let AcceptedDownload {
            settings,
            cancel_flag,
        } = match accept_download(ctx, download_id).await {
            Ok(a) => a,
            Err(resp) => return resp,
        };

        let emitter = ctx.emitter.clone();
        let active = ctx.active_downloads.clone();
        let credentials_health = self.credentials_health.clone();

        tokio::spawn(async move {
            use downloads::yt_dlp::{download_music, music_args_from_settings};
            let yt_dlp_cmd = downloads::yt_dlp::find_yt_dlp_command();
            let quality = req.quality.as_deref().unwrap_or("best").to_string();
            let mut args = music_args_from_settings(&req.url, &quality, &settings);
            args.download_path = resolve_output_dir(
                &req.output_dir,
                settings.create_platform_subfolders,
                "YouTube Music",
            )
            .await;

            let result = download_music(
                args,
                &yt_dlp_cmd,
                progress_sink(&emitter, download_id),
                log_sink(&emitter, "yt-dlp", "yt-dlp", yt_dlp_level),
                cancel_flag,
            )
            .await;

            finish(&emitter, &active, &credentials_health, download_id, result).await;
        });

        ipc_contract::StartDownloadResponse::ok(download_id)
    }
}

pub struct YtVideoDownloader {
    credentials_health: Arc<CredentialsHealth>,
}

impl YtVideoDownloader {
    pub fn from_state(state: &crate::BackendState) -> Self {
        Self {
            credentials_health: state.credentials_health.clone(),
        }
    }
}

#[async_trait]
impl DownloadProvider for YtVideoDownloader {
    async fn start(
        &self,
        req: Value,
        ctx: &DownloadContext,
        download_id: u64,
    ) -> ipc_contract::StartDownloadResponse {
        let req: ipc_contract::StartYtVideoDownloadRequest =
            match crate::services::common::download::parse_request(req, download_id) {
                Ok(r) => r,
                Err(resp) => return resp,
            };

        let AcceptedDownload {
            settings,
            cancel_flag,
        } = match accept_download(ctx, download_id).await {
            Ok(a) => a,
            Err(resp) => return resp,
        };

        let emitter = ctx.emitter.clone();
        let active = ctx.active_downloads.clone();
        let credentials_health = self.credentials_health.clone();

        tokio::spawn(async move {
            use downloads::yt_dlp::{download_video, video_args_from_settings};
            let yt_dlp_cmd = downloads::yt_dlp::find_yt_dlp_command();
            let quality = req
                .resolution
                .as_deref()
                .unwrap_or("bestvideo+bestaudio/best")
                .to_string();
            let mut args = video_args_from_settings(&req.url, &quality, &settings, req.is_generic);
            args.download_path = resolve_output_dir(
                &req.output_dir,
                settings.create_platform_subfolders,
                if req.is_generic { "Generic" } else { "YouTube" },
            )
            .await;
            if let Some(f) = req.format {
                args.merge_output_format = Some(f);
            }

            let result = download_video(
                args,
                &yt_dlp_cmd,
                progress_sink(&emitter, download_id),
                log_sink(&emitter, "yt-dlp", "yt-dlp", yt_dlp_level),
                cancel_flag,
            )
            .await;

            finish(&emitter, &active, &credentials_health, download_id, result).await;
        });

        ipc_contract::StartDownloadResponse::ok(download_id)
    }
}

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::downloads;
use crate::ipc_contract;
use crate::services::common::download::{
    accept_download, always_info, batch_progress_sink, log_sink, AcceptedDownload, DownloadContext,
    DownloadProvider,
};
use crate::services::spotify::rate_limit::LicenseRateLimiter;
use crate::services::spotify::session::LibrespotService;
use crate::venv_manager;

pub struct SpotifyDownloader {
    librespot: Arc<RwLock<LibrespotService>>,
    license_limiter: Arc<LicenseRateLimiter>,
}

impl SpotifyDownloader {
    pub fn from_state(state: &crate::BackendState) -> Self {
        Self {
            librespot: state.librespot.clone(),
            license_limiter: state.license_limiter.clone(),
        }
    }
}

#[async_trait]
impl DownloadProvider for SpotifyDownloader {
    async fn start(
        &self,
        req: Value,
        ctx: &DownloadContext,
        download_id: u64,
    ) -> ipc_contract::StartDownloadResponse {
        let req: ipc_contract::StartSpotifyDownloadRequest =
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
        let backend = settings.spotify_downloader_backend.clone();

        if backend == "native" {
            use crate::services::spotify::native_engine::{
                classify_spotify_url, SpotifyUrlSupport,
            };
            if let SpotifyUrlSupport::Unsupported(kind) = classify_spotify_url(&req.url) {
                active.remove(&download_id);
                return crate::services::common::download::emit_start_error(
                    &emitter,
                    download_id,
                    format!(
                        "Native Spotify downloader can't handle {kind} links. \
                         Open Settings → Spotify → Downloader and switch to Votify."
                    ),
                );
            }

            let librespot = self.librespot.clone();
            let license_limiter = self.license_limiter.clone();
            let venv_py = if venv_manager::is_venv_ready() {
                Some(venv_manager::get_venv_python())
            } else {
                None
            };
            let quality_override = req.meta.quality.clone();
            let user_data = ctx.user_data.clone();
            let skip_enabled =
                settings.native_skip_existing && !req.force_redownload.unwrap_or(false);
            let dedup = Arc::new(downloads::dedup::DedupLedger::new(
                ctx.library.db().clone(),
                skip_enabled,
            ));

            tokio::spawn(async move {
                let result = crate::services::spotify::native_engine::download_with_native_spotify(
                    &settings,
                    librespot,
                    license_limiter,
                    venv_py,
                    user_data,
                    &req.url,
                    quality_override.as_deref(),
                    batch_progress_sink(&emitter, download_id),
                    log_sink(&emitter, "spotify-native", "Spotify (native)", always_info),
                    cancel_flag,
                    Some(dedup.as_ref()),
                )
                .await;

                active.remove(&download_id);
                crate::emit_batch_terminal(&emitter, download_id, "Spotify", &result);
            });

            return ipc_contract::StartDownloadResponse::ok(download_id);
        }

        crate::services::common::cli::spawn_cli_download(
            Arc::new(crate::services::spotify::votify::Votify),
            ctx,
            download_id,
            settings,
            req.url,
            req.meta.quality,
            cancel_flag,
        )
    }
}

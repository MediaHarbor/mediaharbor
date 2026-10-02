use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::downloads;
use crate::ipc_contract;
use crate::services::common::download::{
    accept_download, always_info, batch_progress_sink, log_sink, AcceptedDownload, DownloadContext,
    DownloadProvider,
};

pub struct AppleMusicDownloader;

#[async_trait]
impl DownloadProvider for AppleMusicDownloader {
    async fn start(
        &self,
        req: Value,
        ctx: &DownloadContext,
        download_id: u64,
    ) -> ipc_contract::StartDownloadResponse {
        let req: ipc_contract::StartAppleDownloadRequest =
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
        let backend = settings.apple_downloader_backend.clone();

        if backend == "native" {
            use crate::services::apple_music::native_engine::{
                classify_apple_url, AppleUrlSupport,
            };
            if let AppleUrlSupport::Unsupported(kind) = classify_apple_url(&req.url) {
                active.remove(&download_id);
                return crate::services::common::download::emit_start_error(
                    &emitter,
                    download_id,
                    format!(
                        "Native Apple Music downloader can't handle {kind} links. \
                         Open Settings → Apple Music → Downloader and switch to Gamdl."
                    ),
                );
            }

            let service = Arc::new(
                crate::services::apple_music::playback::AppleMusicService::from_settings(&settings),
            );
            let api =
                match crate::services::apple_music::api::AppleMusicApiClient::unauthenticated() {
                    Ok(c) => c,
                    Err(e) => {
                        active.remove(&download_id);
                        return crate::services::common::download::emit_start_error(
                            &emitter,
                            download_id,
                            format!("Apple Music API init failed: {e}"),
                        );
                    }
                };
            let quality_override = req.meta.quality.clone();
            let skip_enabled =
                settings.native_skip_existing && !req.force_redownload.unwrap_or(false);
            let dedup = Arc::new(downloads::dedup::DedupLedger::new(
                ctx.library.db().clone(),
                skip_enabled,
            ));

            tokio::spawn(async move {
                let result =
                    crate::services::apple_music::native_engine::download_with_native_apple(
                        &settings,
                        service,
                        api,
                        &req.url,
                        quality_override.as_deref(),
                        batch_progress_sink(&emitter, download_id),
                        log_sink(
                            &emitter,
                            "apple-native",
                            "Apple Music (native)",
                            always_info,
                        ),
                        cancel_flag,
                        Some(dedup.as_ref()),
                    )
                    .await;

                active.remove(&download_id);
                crate::emit_batch_terminal(&emitter, download_id, "Apple Music", &result);
            });

            return ipc_contract::StartDownloadResponse::ok(download_id);
        }

        crate::services::common::cli::spawn_cli_download(
            Arc::new(crate::services::apple_music::gamdl::Gamdl),
            ctx,
            download_id,
            settings,
            req.url,
            req.meta.quality,
            cancel_flag,
        )
    }
}

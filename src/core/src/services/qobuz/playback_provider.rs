use crate::streaming_server;
use async_trait::async_trait;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::ipc_contract;
use crate::services::common::playback::PlaybackProvider;

pub struct QobuzPlayback;

#[async_trait]
impl PlaybackProvider for QobuzPlayback {
    async fn play(
        &self,
        req: &ipc_contract::PlayMediaRequest,
        settings: &Settings,
        state: &crate::BackendState,
    ) -> MhResult<ipc_contract::PlayMediaResponse> {
        let track_id = crate::services::common::pipeline::orchestrator::extract_platform_id(
            &req.url,
            crate::services::common::pipeline::orchestrator::Platform::Qobuz,
            crate::services::common::pipeline::orchestrator::ContentType::Track,
        )
        .ok_or_else(|| MhError::Parse("Could not extract Qobuz track ID".into()))?;

        let mut client = state.cached_qobuz_client(settings).await?;
        let cdn_url = match client.get_file_url(&track_id, 27).await {
            Ok(u) => u,
            Err(MhError::Auth(_)) => {
                *state.qobuz_client_cache.write().await = None;
                client =
                    crate::services::qobuz::client::QobuzClient::authenticate(settings).await?;
                *state.qobuz_client_cache.write().await = Some(client.clone());
                client.get_file_url(&track_id, 27).await?
            }
            Err(e) => return Err(e),
        };
        let auth_headers = client.api_headers()?;

        let server = state.streaming_server()?;
        let id = uuid::Uuid::new_v4().to_string();
        let stream_url = server.register(
            &id,
            streaming_server::StreamContent::Proxied {
                url: cdn_url,
                auth_headers,
            },
            "audio/flac",
        );

        Ok(ipc_contract::PlayMediaResponse::audio(stream_url, "qobuz"))
    }
}

use crate::streaming_server;
use async_trait::async_trait;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::ipc_contract;
use crate::services::common::playback::PlaybackProvider;

pub struct DeezerPlayback;

#[async_trait]
impl PlaybackProvider for DeezerPlayback {
    async fn play(
        &self,
        req: &ipc_contract::PlayMediaRequest,
        settings: &Settings,
        state: &crate::BackendState,
    ) -> MhResult<ipc_contract::PlayMediaResponse> {
        if settings.deezer_arl.is_empty() {
            return Err(MhError::Auth(
                "Deezer ARL not configured. Add it in Settings → Deezer → ARL Token.".into(),
            ));
        }
        let client = crate::services::deezer::client::DeezerClient::new(&settings.deezer_arl)?;
        client.authenticate().await?;
        let track_id = crate::services::common::pipeline::orchestrator::extract_platform_id(
            &req.url,
            crate::services::common::pipeline::orchestrator::Platform::Deezer,
            crate::services::common::pipeline::orchestrator::ContentType::Track,
        )
        .ok_or_else(|| MhError::Parse("Could not extract Deezer track ID".into()))?;
        let stream = client.get_stream_url(&track_id, 3).await?;
        let (url_str, ext, filesize, crypto_id) =
            (stream.url, stream.ext, stream.filesize, stream.crypto_id);
        let mime_type: &'static str = if ext == "flac" {
            "audio/flac"
        } else {
            "audio/mpeg"
        };

        let server = state.streaming_server()?;
        let id = uuid::Uuid::new_v4().to_string();

        let stream_url = server.register(
            &id,
            streaming_server::StreamContent::SeekableDeezer {
                cdn_url: url_str,
                crypto_id,
                filesize,
            },
            mime_type,
        );

        Ok(ipc_contract::PlayMediaResponse::audio(stream_url, "deezer"))
    }
}

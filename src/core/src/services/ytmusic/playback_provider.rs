use async_trait::async_trait;

use crate::defaults::Settings;
use crate::errors::MhResult;
use crate::ipc_contract;
use crate::services::common::playback::PlaybackProvider;

pub struct YtMusicPlayback;

#[async_trait]
impl PlaybackProvider for YtMusicPlayback {
    async fn play(
        &self,
        req: &ipc_contract::PlayMediaRequest,
        settings: &Settings,
        state: &crate::BackendState,
    ) -> MhResult<ipc_contract::PlayMediaResponse> {
        let platform = req.platform.as_str();
        let client = crate::http_client::build_stream_client()?;

        let (data, content_type) = match crate::services::youtube::stream::fetch_audio_stream(
            &req.url,
            &state.yt_stream_cache,
            None,
            settings,
            &client,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                state
                    .credentials_health
                    .note_auth_failure(
                        crate::services::common::library::ServicePlatform::YtMusic,
                        &e,
                    )
                    .await;
                return Err(e);
            }
        };

        let server = state.streaming_server()?;
        let id = uuid::Uuid::new_v4().to_string();
        let stream_url = server.register_stream(&id, data, &content_type);

        Ok(ipc_contract::PlayMediaResponse::audio(stream_url, platform))
    }
}

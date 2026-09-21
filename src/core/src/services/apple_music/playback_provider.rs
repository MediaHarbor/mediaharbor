use crate::{is_progressive_only_apple, streaming_server};
use async_trait::async_trait;
use std::sync::Arc;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::ipc_contract;
use crate::services::common::playback::PlaybackProvider;

pub struct AppleMusicPlayback;

#[async_trait]
impl PlaybackProvider for AppleMusicPlayback {
    async fn play(
        &self,
        req: &ipc_contract::PlayMediaRequest,
        _settings: &Settings,
        state: &crate::BackendState,
    ) -> MhResult<ipc_contract::PlayMediaResponse> {
        let apple = state.apple_music.read().await;
        if !apple.is_configured() {
            return Err(MhError::Auth(
                "Apple Music requires cookies. Configure in Settings → Apple.".into(),
            ));
        }

        let server = state.streaming_server()?;
        let id = uuid::Uuid::new_v4().to_string();

        let prep = tokio::time::timeout(
            std::time::Duration::from_secs(45),
            apple.prepare_seekable_audio(&req.url),
        )
        .await
        .map_err(|_| MhError::Other("Apple Music stream timed out".into()))?;
        let stream_url = match prep {
            Ok(seekable) => {
                let seekable = Arc::new(seekable);
                let ct = seekable.content_type.clone();
                server.register(
                    &id,
                    streaming_server::StreamContent::AppleSeekable(seekable),
                    &ct,
                )
            }
            Err(e) if is_progressive_only_apple(&e) => {
                let track = tokio::time::timeout(
                    std::time::Duration::from_secs(45),
                    apple.get_track_stream(&req.url, None),
                )
                .await
                .map_err(|_| MhError::Other("Apple Music stream timed out".into()))??;
                server.register_stream(&id, track.data, &track.content_type)
            }
            Err(e) => return Err(e),
        };
        drop(apple);

        Ok(ipc_contract::PlayMediaResponse::audio(
            stream_url,
            "applemusic",
        ))
    }
}

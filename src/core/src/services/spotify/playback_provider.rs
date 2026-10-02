use crate::{extract_spotify_id, venv_manager};
use async_trait::async_trait;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::ipc_contract;
use crate::services::common::playback::PlaybackProvider;

pub struct SpotifyPlayback;

#[async_trait]
impl PlaybackProvider for SpotifyPlayback {
    async fn play(
        &self,
        req: &ipc_contract::PlayMediaRequest,
        _settings: &Settings,
        state: &crate::BackendState,
    ) -> MhResult<ipc_contract::PlayMediaResponse> {
        let track_id = extract_spotify_id(&req.url).unwrap_or_else(|| req.url.clone());

        if let Some((id, url)) = state.spotify_stream_memo.get(&track_id) {
            if state
                .streaming_server
                .as_ref()
                .is_some_and(|s| s.has_stream(&id))
            {
                return Ok(ipc_contract::PlayMediaResponse::audio(url, "spotify"));
            }
        }

        state.license_limiter.acquire(None).await?;

        let mut librespot = state.librespot.write().await;
        if !librespot.is_logged_in() {
            return Err(MhError::Auth(
                "Spotify streaming requires login. Configure cookies in Settings.".into(),
            ));
        }
        let venv_py = if venv_manager::is_venv_ready() {
            Some(venv_manager::get_venv_python())
        } else {
            None
        };
        let is_episode = req.url.contains("/episode/") || req.url.contains(":episode:");
        let (data, content_type) = if is_episode {
            librespot.get_podcast_episode_stream(&track_id).await?
        } else {
            librespot
                .get_track_stream(&track_id, venv_py.as_deref())
                .await?
        };
        drop(librespot);

        let server = state.streaming_server()?;
        let id = uuid::Uuid::new_v4().to_string();
        let stream_url = server.register_stream(&id, data, &content_type);

        state
            .spotify_stream_memo
            .remember(track_id.clone(), id, stream_url.clone());

        Ok(ipc_contract::PlayMediaResponse::audio(
            stream_url, "spotify",
        ))
    }
}

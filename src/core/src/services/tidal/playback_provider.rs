use crate::http_client;
use crate::{extract_tidal_track_id, streaming_server};
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::ipc_contract;
use crate::services::common::playback::PlaybackProvider;

pub struct TidalPlayback;

#[async_trait]
impl PlaybackProvider for TidalPlayback {
    async fn play(
        &self,
        req: &ipc_contract::PlayMediaRequest,
        settings: &Settings,
        state: &crate::BackendState,
    ) -> MhResult<ipc_contract::PlayMediaResponse> {
        let client = state.authenticate_tidal(settings).await?;
        let track_id = extract_tidal_track_id(&req.url)
            .ok_or_else(|| MhError::Parse("Could not extract Tidal track ID".into()))?;

        let server = state.streaming_server()?;
        let id = uuid::Uuid::new_v4().to_string();

        let stream_url = match client.resolve_playback(&track_id).await? {
            crate::services::tidal::client::TidalPlayback::Direct {
                url,
                mime,
                access_token,
            } => {
                let mut auth_headers = reqwest::header::HeaderMap::new();
                if let Ok(v) =
                    reqwest::header::HeaderValue::from_str(&format!("Bearer {access_token}"))
                {
                    auth_headers.insert(reqwest::header::AUTHORIZATION, v);
                }
                server.register(
                    &id,
                    streaming_server::StreamContent::Proxied { url, auth_headers },
                    mime,
                )
            }
            crate::services::tidal::client::TidalPlayback::Buffered {
                mime,
                urls,
                access_token,
                needs_remux,
            } => {
                if needs_remux {
                    server.register(
                        &id,
                        streaming_server::StreamContent::TidalRemux { urls, access_token },
                        mime,
                    )
                } else {
                    let path = std::env::temp_dir().join(format!("mh_tidal_{id}.mp4"));
                    let done = Arc::new(AtomicBool::new(false));
                    let total = Arc::new(std::sync::atomic::AtomicU64::new(0));
                    let done_task = done.clone();
                    let total_task = total.clone();
                    let path_task = path.clone();
                    let http = http_client::build_client()?;
                    let logger = state.logger.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            crate::services::tidal::client::TidalClient::download_segments_to_file(
                                &http,
                                &access_token,
                                &urls,
                                &path_task,
                                &total_task,
                                false,
                            )
                            .await
                        {
                            logger.error("tidal", &format!("stream prepare failed: {e}"));
                        }
                        done_task.store(true, Ordering::Relaxed);
                    });
                    server.register(
                        &id,
                        streaming_server::StreamContent::TempFile { path, done, total },
                        mime,
                    )
                }
            }
        };

        Ok(ipc_contract::PlayMediaResponse::audio(stream_url, "tidal"))
    }
}

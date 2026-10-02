use crate::venv_manager;
use async_trait::async_trait;

use crate::defaults::Settings;
use crate::errors::MhResult;
use crate::ipc_contract;
use crate::services::common::playback::PlaybackProvider;

/// Pipe an ffmpeg render straight into a progressive stream channel.
async fn ffmpeg_pipe(
    args: Vec<String>,
    tx: tokio::sync::mpsc::Sender<Result<bytes::Bytes, String>>,
) {
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;

    let ffmpeg_bin = venv_manager::resolve_ffmpeg();
    let mut cmd = tokio::process::Command::new(&ffmpeg_bin);
    cmd.args(&args).stdout(Stdio::piped()).stderr(Stdio::null());
    crate::subprocess::apply_no_window(&mut cmd);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx
                .send(Err(format!("ffmpeg ({}): {}", ffmpeg_bin, e)))
                .await;
            return;
        }
    };

    if let Some(mut stdout) = child.stdout.take() {
        let mut buf = vec![0u8; 65_536];
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if tx
                        .send(Ok(bytes::Bytes::copy_from_slice(&buf[..n])))
                        .await
                        .is_err()
                    {
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
}

pub struct YoutubePlayback;

#[async_trait]
impl PlaybackProvider for YoutubePlayback {
    async fn play(
        &self,
        req: &ipc_contract::PlayMediaRequest,
        settings: &Settings,
        state: &crate::BackendState,
    ) -> MhResult<ipc_contract::PlayMediaResponse> {
        let platform = req.platform.as_str();

        let info = match crate::services::youtube::stream::get_video_stream_info_with_settings(
            &req.url, None, settings,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                state
                    .credentials_health
                    .note_auth_failure(
                        crate::services::common::library::ServicePlatform::Youtube,
                        &e,
                    )
                    .await;
                return Err(e);
            }
        };

        let server = state.streaming_server()?;
        let id = uuid::Uuid::new_v4().to_string();

        if info.is_live {
            let hls_url = info.video.url.clone();
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, String>>(32);
            tokio::spawn(ffmpeg_pipe(
                [
                    "-loglevel",
                    "error",
                    "-i",
                    &hls_url,
                    "-c",
                    "copy",
                    "-f",
                    "mpegts",
                    "pipe:1",
                ]
                .iter()
                .map(|a| a.to_string())
                .collect(),
                tx,
            ));
            let stream_url = server.register_stream_progressive(&id, rx, "video/mp2t");
            return Ok(ipc_contract::PlayMediaResponse::live_video(
                stream_url, platform,
            ));
        }

        // Picture and audio are served as two streams rather than muxed into one.
        // The player decodes every media type natively and the `<video>` element
        // only draws frames, muted.
        //
        // Both are registered as `Proxied`, not `Progressive`. A progressive
        // stream is *removed from the map when it is served* — it answers exactly
        // one request and 404s afterwards — so any reload, effect re-run or
        // StrictMode double-invoke kills playback, and the 404 body is what the
        // decoder then tries to probe. `Proxied` re-fetches per request and
        // forwards `Range`, which also gives video real seeking for the first time.
        let mut auth_headers = reqwest::header::HeaderMap::new();
        auth_headers.insert(
            reqwest::header::REFERER,
            reqwest::header::HeaderValue::from_static("https://www.youtube.com/"),
        );

        let stream_url = server.register(
            &id,
            crate::streaming_server::StreamContent::Proxied {
                url: info.video.url.clone(),
                auth_headers: auth_headers.clone(),
            },
            &info.video.mime_type,
        );
        let audio_url = server.register(
            &format!("{id}-audio"),
            crate::streaming_server::StreamContent::Proxied {
                url: info.audio.url.clone(),
                auth_headers,
            },
            &info.audio.mime_type,
        );

        Ok(ipc_contract::PlayMediaResponse::video_with_audio(
            stream_url, audio_url, platform,
        ))
    }
}

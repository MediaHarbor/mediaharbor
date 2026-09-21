use axum::http::{header, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use tokio::sync::mpsc;

use crate::streaming_server::{body_from_channel, or_500};

pub async fn serve_remux(
    http: reqwest::Client,
    urls: Vec<String>,
    access_token: String,
    content_type: String,
) -> Response {
    let http = http.clone();
    let ffmpeg_bin = crate::venv_manager::resolve_ffmpeg();
    let (tx, rx) = mpsc::channel::<Result<Bytes, String>>(32);

    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;

        let mut ffmpeg = Command::new(&ffmpeg_bin);
        ffmpeg
            .args([
                "-loglevel",
                "error",
                "-i",
                "pipe:0",
                "-vn",
                "-c:a",
                "copy",
                "-f",
                "flac",
                "pipe:1",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        crate::subprocess::apply_no_window(&mut ffmpeg);
        let mut child = match ffmpeg.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(Err(format!("ffmpeg spawn failed: {e}"))).await;
                return;
            }
        };
        let mut stdin = child.stdin.take().expect("stdin piped");
        let mut stdout = child.stdout.take().expect("stdout piped");

        let feed_http = http.clone();
        let feed_token = access_token.clone();
        let feeder = tokio::spawn(async move {
            for url in urls {
                let resp = match feed_http
                    .get(&url)
                    .header("Authorization", format!("Bearer {feed_token}"))
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(_) => break,
                };
                match resp.bytes().await {
                    Ok(b) => {
                        if stdin.write_all(&b).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = stdin.shutdown().await;
        });

        let mut buf = vec![0u8; 65_536];
        loop {
            use tokio::io::AsyncReadExt;
            match stdout.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if tx
                        .send(Ok(Bytes::copy_from_slice(&buf[..n])))
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
        let _ = feeder.await;
        let _ = child.wait().await;
    });

    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body_from_channel(rx))
        .unwrap_or_else(|_| or_500())
}

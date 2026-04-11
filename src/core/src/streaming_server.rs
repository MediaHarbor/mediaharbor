use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use bytes::Bytes;
use dashmap::DashMap;
use reqwest::{Client, header::HeaderMap as ReqwestHeaderMap};
use tokio::sync::{mpsc, oneshot, Mutex};
use tower_http::cors::{Any, CorsLayer};

use crate::errors::MhResult;

pub enum StreamContent {
    Full(Bytes),
    Progressive(Mutex<mpsc::Receiver<Result<Bytes, String>>>),
    Proxied { url: String, auth_headers: ReqwestHeaderMap },
    SeekableDeezer { cdn_url: String, track_id: String, filesize: Option<u64> },
    /// ffmpeg is writing an MP4 to `path`; `done` is set to `true` when muxing finishes.
    /// Supports Range requests for instant-start + seeking on dual-URL YouTube video.
    TempFile { path: PathBuf, done: Arc<AtomicBool> },
}

pub struct StreamEntry {
    pub content: StreamContent,
    pub content_type: String,
}

pub type StreamMap = Arc<DashMap<String, StreamEntry>>;

#[derive(Clone)]
struct AppState {
    streams: StreamMap,
    http: Client,
}

pub struct StreamingServer {
    pub port: u16,
    streams: StreamMap,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl StreamingServer {
    pub async fn start() -> MhResult<Self> {
        let streams: StreamMap = Arc::new(DashMap::new());
        let http = crate::http_client::build_client()?;
        let state = AppState { streams: streams.clone(), http };

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let app = Router::new()
            .route("/:id", get(serve_stream))
            .with_state(state)
            .layer(cors);

        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let port = listener.local_addr()?.port();

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();
        });

        Ok(Self {
            port,
            streams,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub fn register_stream(&self, id: &str, data: Bytes, content_type: &str) -> String {
        self.streams.insert(
            id.to_string(),
            StreamEntry {
                content: StreamContent::Full(data),
                content_type: content_type.to_string(),
            },
        );
        format!("http://127.0.0.1:{}/{}", self.port, id)
    }

    pub fn register_stream_progressive(
        &self,
        id: &str,
        rx: mpsc::Receiver<Result<Bytes, String>>,
        content_type: &str,
    ) -> String {
        self.streams.insert(
            id.to_string(),
            StreamEntry {
                content: StreamContent::Progressive(Mutex::new(rx)),
                content_type: content_type.to_string(),
            },
        );
        format!("http://127.0.0.1:{}/{}", self.port, id)
    }

    pub fn register_stream_deezer(
        &self,
        id: &str,
        cdn_url: String,
        track_id: String,
        filesize: Option<u64>,
        content_type: &str,
    ) -> String {
        self.streams.insert(
            id.to_string(),
            StreamEntry {
                content: StreamContent::SeekableDeezer { cdn_url, track_id, filesize },
                content_type: content_type.to_string(),
            },
        );
        format!("http://127.0.0.1:{}/{}", self.port, id)
    }

    pub fn register_stream_proxied(
        &self,
        id: &str,
        cdn_url: String,
        auth_headers: ReqwestHeaderMap,
        content_type: &str,
    ) -> String {
        self.streams.insert(
            id.to_string(),
            StreamEntry {
                content: StreamContent::Proxied { url: cdn_url, auth_headers },
                content_type: content_type.to_string(),
            },
        );
        format!("http://127.0.0.1:{}/{}", self.port, id)
    }

    pub fn register_stream_tempfile(
        &self,
        id: &str,
        path: PathBuf,
        done: Arc<AtomicBool>,
        content_type: &str,
    ) -> String {
        self.streams.insert(
            id.to_string(),
            StreamEntry {
                content: StreamContent::TempFile { path, done },
                content_type: content_type.to_string(),
            },
        );
        format!("http://127.0.0.1:{}/{}", self.port, id)
    }

    pub fn remove_stream(&self, id: &str) {
        if let Some((_, entry)) = self.streams.remove(id) {
            if let StreamContent::TempFile { path, .. } = entry.content {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    pub fn clear_streams(&self) {
        for r in self.streams.iter() {
            if let StreamContent::TempFile { path, .. } = &r.value().content {
                let _ = std::fs::remove_file(path);
            }
        }
        self.streams.clear();
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.clear_streams();
    }
}

fn parse_range_header_opt(headers: &HeaderMap) -> (u64, bool) {
    headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("bytes="))
        .and_then(|s| s.split('-').next())
        .and_then(|s| s.parse::<u64>().ok())
        .map(|n| (n, true))
        .unwrap_or((0, false))
}

enum ResolvedStream {
    Full { content_type: String, data: Bytes },
    Progressive(String),
    Proxied { content_type: String, url: String, auth_headers: ReqwestHeaderMap },
}

async fn serve_stream(
    Path(id): Path<String>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    {
        let entry = match state.streams.get(&id) {
            Some(e) => e,
            None => return (StatusCode::NOT_FOUND, "Stream not found").into_response(),
        };
        if let StreamContent::SeekableDeezer { cdn_url, track_id, filesize } = &entry.content {
            let cdn_url = cdn_url.clone();
            let track_id = track_id.clone();
            let filesize = *filesize;
            let content_type = entry.content_type.clone();
            drop(entry);

            const CHUNK: u64 = 6144;
            const ENC: usize = 2048;

            let (range_start, is_range) = parse_range_header_opt(&headers);
            let aligned = (range_start / CHUNK) * CHUNK;

            let (tx, rx) = mpsc::channel::<Result<Bytes, String>>(32);
            let http = state.http.clone();
            tokio::spawn(async move {
                use futures_util::StreamExt;
                use crate::crypto::deezer::{generate_blowfish_key, decrypt_chunk};

                let key = generate_blowfish_key(&track_id);
                let resp = http
                    .get(&cdn_url)
                    .header("Range", format!("bytes={}-", aligned))
                    .send()
                    .await;
                let resp = match resp {
                    Ok(r) => r,
                    Err(e) => { let _ = tx.send(Err(e.to_string())).await; return; }
                };

                let mut buf: Vec<u8> = Vec::with_capacity(CHUNK as usize * 2);
                let mut stream = resp.bytes_stream();

                while let Some(result) = stream.next().await {
                    match result {
                        Ok(incoming) => buf.extend_from_slice(&incoming),
                        Err(e) => { let _ = tx.send(Err(e.to_string())).await; return; }
                    }
                    while buf.len() >= CHUNK as usize {
                        let raw: Vec<u8> = buf.drain(..CHUNK as usize).collect();
                        let mut out = decrypt_chunk(&key, &raw[..ENC]);
                        out.extend_from_slice(&raw[ENC..]);
                        if tx.send(Ok(Bytes::from(out))).await.is_err() { return; }
                    }
                }

                if !buf.is_empty() {
                    let raw = buf;
                    let out = if raw.len() >= ENC {
                        let mut dec = decrypt_chunk(&key, &raw[..ENC]);
                        dec.extend_from_slice(&raw[ENC..]);
                        dec
                    } else {
                        raw
                    };
                    let _ = tx.send(Ok(Bytes::from(out))).await;
                }
            });

            let stream = futures_util::stream::unfold(rx, |mut rx| async move {
                match rx.recv().await {
                    Some(Ok(c)) => Some((Ok::<Bytes, std::io::Error>(c), rx)),
                    Some(Err(e)) => Some((Err(std::io::Error::new(std::io::ErrorKind::Other, e)), rx)),
                    None => None,
                }
            });

            let mut builder = axum::http::Response::builder()
                .header(header::CONTENT_TYPE, content_type)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CACHE_CONTROL, "no-cache");

            if is_range {
                if let Some(fs) = filesize {
                    builder = builder
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", aligned, fs - 1, fs));
                } else {
                    builder = builder.status(StatusCode::OK);
                }
            } else {
                builder = builder.status(StatusCode::OK);
                if let Some(fs) = filesize {
                    builder = builder.header(header::CONTENT_LENGTH, fs.to_string());
                }
            }

            return builder
                .body(Body::from_stream(stream))
                .unwrap_or_else(|_| {
                    axum::http::Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::empty())
                        .unwrap()
                });
        } else if let StreamContent::TempFile { path, done } = &entry.content {
            let path = path.clone();
            let done = done.clone();
            let content_type = entry.content_type.clone();
            drop(entry);

            while !done.load(Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }

            let file_size = match tokio::fs::metadata(&path).await {
                Ok(m) => m.len(),
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            };

            let (range_start, is_range) = parse_range_header_opt(&headers);

            let path_task = path.clone();
            let (tx, rx) = mpsc::channel::<Result<Bytes, String>>(32);
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncSeekExt};

                let mut file = match tokio::fs::File::open(&path_task).await {
                    Ok(f) => f,
                    Err(e) => { let _ = tx.send(Err(e.to_string())).await; return; }
                };
                if range_start > 0 {
                    if file.seek(std::io::SeekFrom::Start(range_start)).await.is_err() {
                        let _ = tx.send(Err("Seek failed".to_string())).await;
                        return;
                    }
                }
                let mut buf = vec![0u8; 65_536];
                loop {
                    match file.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if tx.send(Ok(Bytes::copy_from_slice(&buf[..n]))).await.is_err() {
                                return;
                            }
                        }
                        Err(e) => { let _ = tx.send(Err(e.to_string())).await; return; }
                    }
                }
            });

            let stream = futures_util::stream::unfold(rx, |mut rx| async move {
                match rx.recv().await {
                    Some(Ok(c)) => Some((Ok::<Bytes, std::io::Error>(c), rx)),
                    Some(Err(e)) => Some((Err(std::io::Error::new(std::io::ErrorKind::Other, e)), rx)),
                    None => None,
                }
            });

            let mut builder = axum::http::Response::builder()
                .header(header::CONTENT_TYPE, content_type)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CACHE_CONTROL, "no-cache");

            if is_range {
                let end = file_size.saturating_sub(1);
                let content_length = file_size.saturating_sub(range_start);
                builder = builder
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_RANGE, format!("bytes {}-{}/{}", range_start, end, file_size))
                    .header(header::CONTENT_LENGTH, content_length.to_string());
            } else {
                builder = builder
                    .status(StatusCode::OK)
                    .header(header::CONTENT_LENGTH, file_size.to_string());
            }

            return builder
                .body(Body::from_stream(stream))
                .unwrap_or_else(|_| {
                    axum::http::Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::empty())
                        .unwrap()
                });
        }
    }

    let resolved = {
        let entry = match state.streams.get(&id) {
            Some(e) => e,
            None => return (StatusCode::NOT_FOUND, "Stream not found").into_response(),
        };
        match &entry.content {
            StreamContent::Full(b) => ResolvedStream::Full {
                content_type: entry.content_type.clone(),
                data: b.clone(),
            },
            StreamContent::Progressive(_) => ResolvedStream::Progressive(entry.content_type.clone()),
            StreamContent::Proxied { url, auth_headers } => ResolvedStream::Proxied {
                content_type: entry.content_type.clone(),
                url: url.clone(),
                auth_headers: auth_headers.clone(),
            },
            StreamContent::SeekableDeezer { .. } | StreamContent::TempFile { .. } => unreachable!(),
        }
    };

    if let ResolvedStream::Proxied { content_type, url: cdn_url, auth_headers } = resolved {

        let mut upstream_req = state.http.get(&cdn_url).headers(auth_headers);
        if let Some(range) = headers.get(header::RANGE) {
            upstream_req = upstream_req.header(reqwest::header::RANGE, range.clone());
        }

        let upstream_resp = match upstream_req.send().await {
            Ok(r) => r,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, e.to_string()).into_response();
            }
        };

        let up_status = upstream_resp.status();
        let axum_status = StatusCode::from_u16(up_status.as_u16()).unwrap_or(StatusCode::OK);
        let up_headers = upstream_resp.headers().clone();

        let ct = up_headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(&content_type)
            .to_string();

        let body_stream = futures_util::stream::unfold(upstream_resp, |mut resp| async move {
            match resp.chunk().await {
                Ok(Some(chunk)) => Some((Ok::<Bytes, std::io::Error>(chunk), resp)),
                Ok(None) => None,
                Err(e) => Some((
                    Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
                    resp,
                )),
            }
        });

        let mut builder = axum::http::Response::builder()
            .status(axum_status)
            .header(header::CONTENT_TYPE, ct)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CACHE_CONTROL, "no-cache");

        if let Some(cl) = up_headers.get(reqwest::header::CONTENT_LENGTH) {
            builder = builder.header(header::CONTENT_LENGTH, cl.clone());
        }
        if let Some(cr) = up_headers.get(reqwest::header::CONTENT_RANGE) {
            builder = builder.header(header::CONTENT_RANGE, cr.clone());
        }

        return builder
            .body(Body::from_stream(body_stream))
            .unwrap_or_else(|_| {
                axum::http::Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .unwrap()
            });
    }

    if let ResolvedStream::Progressive(content_type) = resolved {
        let owned = match state.streams.remove(&id) {
            Some((_, e)) => e,
            None => return (StatusCode::NOT_FOUND, "Stream already consumed").into_response(),
        };
        let rx = match owned.content {
            StreamContent::Progressive(m) => m.into_inner(),
            StreamContent::Full(_) | StreamContent::Proxied { .. } | StreamContent::SeekableDeezer { .. } | StreamContent::TempFile { .. } => unreachable!(),
        };

        let stream = futures_util::stream::unfold(rx, |mut rx| async move {
            match rx.recv().await {
                Some(Ok(chunk)) => Some((Ok::<Bytes, std::io::Error>(chunk), rx)),
                Some(Err(e)) => Some((
                    Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
                    rx,
                )),
                None => None,
            }
        });

        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (header::CACHE_CONTROL, "no-cache".to_string()),
            ],
            Body::from_stream(stream),
        )
            .into_response();
    }

    let (content_type, data) = match resolved {
        ResolvedStream::Full { content_type, data } => (content_type, data),
        _ => unreachable!(),
    };

    let total = data.len();

    if let Some(range_val) = headers.get(header::RANGE) {
        if let Ok(range_str) = range_val.to_str() {
            if let Some(rest) = range_str.strip_prefix("bytes=") {
                let parts: Vec<&str> = rest.splitn(2, '-').collect();
                let start: usize = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
                let end: usize = parts
                    .get(1)
                    .and_then(|s| if s.is_empty() { None } else { s.parse().ok() })
                    .unwrap_or(total.saturating_sub(1));
                let end = end.min(total.saturating_sub(1));

                if start < total {
                    let slice = data.slice(start..=end);
                    let content_range = format!("bytes {}-{}/{}", start, end, total);
                    return (
                        StatusCode::PARTIAL_CONTENT,
                        [
                            (header::CONTENT_TYPE, content_type),
                            (header::CONTENT_RANGE, content_range),
                            (header::ACCEPT_RANGES, "bytes".to_string()),
                            (header::CACHE_CONTROL, "no-cache".to_string()),
                        ],
                        slice,
                    )
                        .into_response();
                }
            }
        }
    }

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_LENGTH, total.to_string()),
            (header::ACCEPT_RANGES, "bytes".to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        data,
    )
        .into_response()
}

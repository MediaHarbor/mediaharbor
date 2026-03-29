use std::{
    net::SocketAddr,
    sync::Arc,
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
use tokio::sync::{mpsc, oneshot, Mutex};
use tower_http::cors::{Any, CorsLayer};

use crate::errors::MhResult;

pub enum StreamContent {
    Full(Bytes),
    Progressive(Mutex<mpsc::Receiver<Result<Bytes, String>>>),
}

pub struct StreamEntry {
    pub content: StreamContent,
    pub content_type: String,
}

pub type StreamMap = Arc<DashMap<String, StreamEntry>>;

#[derive(Clone)]
struct AppState {
    streams: StreamMap,
}

pub struct StreamingServer {
    pub port: u16,
    streams: StreamMap,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl StreamingServer {
    pub async fn start() -> MhResult<Self> {
        let streams: StreamMap = Arc::new(DashMap::new());
        let state = AppState { streams: streams.clone() };

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

    pub fn remove_stream(&self, id: &str) {
        self.streams.remove(id);
    }

    pub fn clear_streams(&self) {
        self.streams.clear();
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.streams.clear();
    }
}

async fn serve_stream(
    Path(id): Path<String>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    let (content_type, is_progressive) = {
        let entry = match state.streams.get(&id) {
            Some(e) => e,
            None => return (StatusCode::NOT_FOUND, "Stream not found").into_response(),
        };
        (entry.content_type.clone(), matches!(entry.content, StreamContent::Progressive(_)))
    };

    if is_progressive {
        let owned = match state.streams.remove(&id) {
            Some((_, e)) => e,
            None => return (StatusCode::NOT_FOUND, "Stream already consumed").into_response(),
        };
        let rx = match owned.content {
            StreamContent::Progressive(m) => m.into_inner(),
            StreamContent::Full(_) => unreachable!(),
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

    let entry = match state.streams.get(&id) {
        Some(e) => e,
        None => return (StatusCode::NOT_FOUND, "Stream not found").into_response(),
    };

    let data = match &entry.content {
        StreamContent::Full(b) => b.clone(),
        StreamContent::Progressive(_) => unreachable!(),
    };
    drop(entry);

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

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::streaming_server::{or_500, parse_range_header, range_not_satisfiable};

pub async fn serve_seekable(
    stream: Arc<crate::services::apple_music::playback::AppleSeekableStream>,
    headers: &HeaderMap,
    content_type: String,
) -> Response {
    let total = stream.total_size;
    let requested = parse_range_header(headers);
    let is_range = requested.is_some();

    if total == 0 {
        if is_range {
            return range_not_satisfiable(0);
        }
        return axum::http::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_LENGTH, "0")
            .body(Body::empty())
            .unwrap();
    }

    const FIRST_WINDOW: u64 = 256 * 1024;
    const STREAM_WINDOW: u64 = 2 * 1024 * 1024;

    let (range_start_raw, range_end) = match requested {
        Some(range) => {
            let Some((start, end)) = range.resolve(total) else {
                return range_not_satisfiable(total);
            };
            // Only "the rest" is answered a window at a time — the element
            // comes back for more as it plays. An explicit end is honoured.
            let end = if range.is_open_ended() {
                let window = if start == 0 {
                    FIRST_WINDOW
                } else {
                    STREAM_WINDOW
                };
                (start + window - 1).min(end)
            } else {
                end
            };
            (start, end)
        }
        None => (0, total - 1),
    };

    let bytes = match stream.read_range(range_start_raw, range_end).await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let mut builder = axum::http::Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONTENT_LENGTH, bytes.len().to_string());

    if is_range {
        let actual_end = (range_start_raw + bytes.len() as u64).saturating_sub(1);
        builder = builder.status(StatusCode::PARTIAL_CONTENT).header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", range_start_raw, actual_end, total),
        );
    } else {
        builder = builder.status(StatusCode::OK);
    }

    builder.body(Body::from(bytes)).unwrap_or_else(|_| or_500())
}

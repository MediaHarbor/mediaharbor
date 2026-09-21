use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use tokio::sync::mpsc;

use crate::streaming_server::{
    body_from_channel, or_500, parse_range_header, range_not_satisfiable, ByteRange, RangeClip,
};

pub async fn serve_seekable(
    http: &reqwest::Client,
    headers: &HeaderMap,
    cdn_url: String,
    crypto_id: String,
    filesize: Option<u64>,
    content_type: String,
) -> Response {
    const CHUNK: u64 = 6144;
    const ENC: usize = 2048;

    let requested = parse_range_header(headers);
    let (range_start, range_end, is_range) = match (requested, filesize) {
        (Some(range), Some(fs)) => match range.resolve(fs) {
            Some((start, end)) => (start, Some(end), true),
            None => {
                return range_not_satisfiable(fs);
            }
        },
        // Without a known size a suffix range cannot be placed at all, so
        // the header is ignored and the whole body sent — see RFC 7233.
        (Some(ByteRange::FromStart { start, end }), None) => (start, end, true),
        _ => (0, None, false),
    };
    let aligned = (range_start / CHUNK) * CHUNK;
    let skip = (range_start - aligned) as usize;
    let mut clip = RangeClip {
        skip,
        remaining: range_end.map(|e| e - range_start + 1),
    };

    let (tx, rx) = mpsc::channel::<Result<Bytes, String>>(32);
    let http = http.clone();
    tokio::spawn(async move {
        use crate::services::deezer::crypto::{decrypt_chunk, generate_blowfish_key};
        use futures_util::StreamExt;

        let key = generate_blowfish_key(&crypto_id);
        let resp = http
            .get(&cdn_url)
            .header("Range", format!("bytes={}-", aligned))
            .send()
            .await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(Err(e.to_string())).await;
                return;
            }
        };

        let mut buf: Vec<u8> = Vec::with_capacity(CHUNK as usize * 2);
        let mut stream = resp.bytes_stream();

        while let Some(result) = stream.next().await {
            match result {
                Ok(incoming) => buf.extend_from_slice(&incoming),
                Err(e) => {
                    let _ = tx.send(Err(e.to_string())).await;
                    return;
                }
            }
            while buf.len() >= CHUNK as usize {
                let raw: Vec<u8> = buf.drain(..CHUNK as usize).collect();
                let mut out = decrypt_chunk(&key, &raw[..ENC]);
                out.extend_from_slice(&raw[ENC..]);
                if let Some(bytes) = clip.take(out) {
                    if tx.send(Ok(bytes)).await.is_err() {
                        return;
                    }
                }
            }
            if clip.exhausted() {
                return;
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
            if let Some(bytes) = clip.take(out) {
                let _ = tx.send(Ok(bytes)).await;
            }
        }
    });

    let mut builder = axum::http::Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "no-cache");

    if is_range {
        match (filesize, range_end) {
            (Some(fs), Some(end)) => {
                builder = builder
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {}-{}/{}", range_start, end, fs),
                    )
                    .header(header::CONTENT_LENGTH, (end - range_start + 1).to_string());
            }
            _ => builder = builder.status(StatusCode::OK),
        }
    } else {
        builder = builder.status(StatusCode::OK);
        if let Some(fs) = filesize {
            builder = builder.header(header::CONTENT_LENGTH, fs.to_string());
        }
    }

    builder
        .body(body_from_channel(rx))
        .unwrap_or_else(|_| or_500())
}

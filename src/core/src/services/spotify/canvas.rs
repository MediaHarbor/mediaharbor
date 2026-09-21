use reqwest::Client;

use crate::errors::{MhError, MhResult};

const CANVAZ_URL: &str = "https://spclient.wg.spotify.com/canvaz-cache/v0/canvases";

fn pb_len_delim(field: u32, payload: &[u8], out: &mut Vec<u8>) {
    out.push(((field << 3) | 2) as u8);
    let mut n = payload.len();
    loop {
        let mut b = (n & 0x7f) as u8;
        n >>= 7;
        if n != 0 {
            b |= 0x80;
        }
        out.push(b);
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(payload);
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut shift = 0u32;
    let mut val = 0u64;
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        val |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(val);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

fn collect_strings(buf: &[u8], out: &mut Vec<String>) {
    let mut pos = 0usize;
    while pos < buf.len() {
        let key = match read_varint(buf, &mut pos) {
            Some(k) => k,
            None => return,
        };
        let wire = (key & 0x7) as u32;
        match wire {
            0 => {
                if read_varint(buf, &mut pos).is_none() {
                    return;
                }
            }
            2 => {
                let len = match read_varint(buf, &mut pos) {
                    Some(l) => l as usize,
                    None => return,
                };
                let end = match pos.checked_add(len) {
                    Some(e) if e <= buf.len() => e,
                    _ => return,
                };
                let slice = &buf[pos..end];
                if let Ok(s) = std::str::from_utf8(slice) {
                    if s.chars().all(|c| !c.is_control()) && !s.is_empty() {
                        out.push(s.to_string());
                    }
                }
                collect_strings(slice, out);
                pos = end;
            }
            5 => pos += 4,
            1 => pos += 8,
            _ => return,
        }
    }
}

pub async fn canvas_url(
    client: &Client,
    access_token: &str,
    track_id: &str,
) -> MhResult<Option<String>> {
    let entity_uri = format!("spotify:track:{track_id}");
    let mut entity = Vec::new();
    pb_len_delim(1, entity_uri.as_bytes(), &mut entity);
    let mut req = Vec::new();
    pb_len_delim(1, &entity, &mut req);

    let resp = client
        .post(CANVAZ_URL)
        .header("authorization", format!("Bearer {access_token}"))
        .header("accept", "application/protobuf")
        .header("content-type", "application/protobuf")
        .header("app-platform", "WebPlayer")
        .header("origin", "https://open.spotify.com")
        .header("referer", "https://open.spotify.com/")
        .body(req)
        .send()
        .await
        .map_err(MhError::Network)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(MhError::Other(format!("Spotify canvas {status}: {body}")));
    }

    let bytes = resp.bytes().await.map_err(MhError::Network)?;
    let mut strings = Vec::new();
    collect_strings(&bytes, &mut strings);
    let url = strings
        .into_iter()
        .find(|u| u.starts_with("http") && u.contains(".cnvs.mp4"));
    Ok(url)
}

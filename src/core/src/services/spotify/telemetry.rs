use std::net::ToSocketAddrs;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use crate::services::common::ids::{now_millis, rand_hex};

const DEALER_HOST: &str = "gew4-dealer.g2.spotify.com";
const DEALER: &str = "wss://gew4-dealer.g2.spotify.com/?access_token=";

const CLIENT_VERSION: &str = "harmony:4.76.0-380da12a4";
const THRESHOLD_MS: u64 = 30_000;

#[derive(Default)]
pub struct SpotifyPlayContext {
    pub context_uri: Option<String>,
    pub track_index: Option<u64>,
}

type DealerWs =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn device_id() -> String {
    static DEVICE_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DEVICE_ID.get_or_init(|| rand_hex(40)).clone()
}

async fn dealer_tcp() -> Option<TcpStream> {
    let host = DEALER_HOST.to_string();
    let mut addrs = tokio::task::spawn_blocking(move || {
        (host.as_str(), 443)
            .to_socket_addrs()
            .map(|it| it.collect::<Vec<_>>())
            .unwrap_or_default()
    })
    .await
    .ok()?;
    addrs.sort_by_key(|a| a.is_ipv6());
    for addr in addrs {
        if let Ok(Ok(s)) = timeout(Duration::from_secs(4), TcpStream::connect(addr)).await {
            return Some(s);
        }
    }
    None
}

async fn dealer_connect(access_token: &str) -> Option<(DealerWs, String)> {
    let tcp = dealer_tcp().await?;
    let request = format!("{DEALER}{access_token}")
        .into_client_request()
        .ok()?;
    let connector =
        tokio_tungstenite::Connector::NativeTls(native_tls::TlsConnector::builder().build().ok()?);
    let (mut ws, _) = timeout(
        Duration::from_secs(10),
        tokio_tungstenite::client_async_tls_with_config(request, tcp, None, Some(connector)),
    )
    .await
    .ok()?
    .ok()?;
    for _ in 0..8 {
        let msg = match timeout(Duration::from_secs(10), ws.next()).await {
            Ok(Some(Ok(m))) => m,
            _ => break,
        };
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Ping(p) => {
                let _ = ws.send(Message::Pong(p)).await;
                continue;
            }
            _ => continue,
        };
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(id) = v
                .pointer("/headers/Spotify-Connection-Id")
                .and_then(|x| x.as_str())
            {
                return Some((ws, id.to_string()));
            }
        }
    }
    let _ = ws.close(None).await;
    None
}

async fn await_replace_state(ws: &mut DealerWs) -> Option<(Value, usize)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while tokio::time::Instant::now() < deadline {
        match timeout(Duration::from_secs(3), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                let txt = t.to_string();
                if !txt.contains("replace_state") {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                    if let Some(payload) = v.pointer("/payloads/0") {
                        let machine = payload.get("state_machine").cloned().unwrap_or(Value::Null);
                        let index = payload
                            .pointer("/state_ref/state_index")
                            .and_then(|x| x.as_u64())
                            .unwrap_or(0) as usize;
                        if machine
                            .pointer("/state_machine_id")
                            .and_then(|x| x.as_str())
                            .is_some_and(|s| !s.is_empty())
                        {
                            return Some((machine, index));
                        }
                    }
                }
            }
            Ok(Some(Ok(Message::Ping(p)))) => {
                let _ = ws.send(Message::Pong(p)).await;
            }
            _ => break,
        }
    }
    None
}

fn elapsed_ms(started_at: tokio::time::Instant) -> u64 {
    started_at.elapsed().as_millis() as u64
}

fn resolved_track_uri(machine: &Value, index: usize) -> Option<String> {
    let track = machine
        .pointer(&format!("/states/{index}/track"))
        .and_then(|x| x.as_u64())?;
    machine
        .pointer(&format!("/tracks/{track}/metadata/uri"))
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

fn track_duration_ms(machine: &Value, index: usize) -> Option<u64> {
    let track = machine
        .pointer(&format!("/states/{index}/track"))
        .and_then(|x| x.as_u64())?;
    machine
        .pointer(&format!("/tracks/{track}/metadata/duration"))
        .and_then(|x| x.as_u64())
}

fn playback_stats(played_ms: u64) -> Value {
    let local_time_ms = now_millis() as u64;
    json!({
        "ms_total_est": played_ms,
        "ms_metadata_duration": 0,
        "ms_manifest_latency": 328,
        "ms_latency": 1038,
        "ms_first_bytes_latency": 1090,
        "start_offset_ms": 5,
        "ms_initial_buffering": 681,
        "ms_initial_rebuffer": 681,
        "ms_seek_rebuffering": 0,
        "ms_stalled": 0,
        "max_ms_seek_rebuffering": 0,
        "max_ms_stalled": 0,
        "n_stalls": 0,
        "n_rendition_upgrade": 0,
        "n_rendition_downgrade": 0,
        "bps_bandwidth_max": 0,
        "bps_bandwidth_min": 0,
        "bps_bandwidth_avg": 0,
        "audiocodec": "mp4",
        "audio_start_bitrate": 256_000,
        "video_start_bitrate": Value::Null,
        "start_bitrate": 256_000,
        "time_weighted_bitrate": 0,
        "key_system": "widevine",
        "ms_key_latency": 791,
        "total_bytes": played_ms.saturating_mul(32_000) / 1000,
        "local_time_ms": local_time_ms,
        "n_dropped_video_frames": 0,
        "n_total_video_frames": 0,
        "resolution_max": 0,
        "resolution_min": 0,
        "strategy": "MSE",
        "ms_played_per_surface": {},
        "ms_played_visible": 0,
        "ms_played_per_audio_format": { "mp4a.40.2;256000": played_ms },
        "ms_played_per_video_format": {},
    })
}

fn feature_identifier(context_uri: &str) -> &'static str {
    match context_uri.split(':').nth(1) {
        Some("album") => "album",
        Some("playlist") => "playlist",
        Some("artist") => "artist",
        Some("show") => "show",
        _ => "auto_play_on_load",
    }
}

fn play_command(context_uri: &str, track_uri: &str, track_index: Option<u64>) -> Value {
    let skip_to = match track_index {
        Some(index) => json!({ "track_index": index, "track_uri": track_uri }),
        None => json!({}),
    };
    json!({
        "command": {
            "context": {
                "uri": context_uri,
                "url": format!("context://{context_uri}"),
                "metadata": {}
            },
            "play_origin": {
                "feature_identifier": feature_identifier(context_uri),
                "feature_version": CLIENT_VERSION,
                "referrer_identifier": "mediaharbor"
            },
            "options": {
                "license": "tft",
                "skip_to": skip_to,
                "player_options_override": {}
            },
            "logging_params": { "command_id": uuid::Uuid::new_v4().simple().to_string() },
            "endpoint": "play"
        }
    })
}

fn register_body(dev_id: &str, connection_id: &str) -> Value {
    json!({
        "device": {
            "brand": "spotify",
            "capabilities": {
                "change_volume": true,
                "enable_play_token": true,
                "supports_file_media_type": true,
                "play_token_lost_behavior": "pause",
                "disable_connect": true,
                "audio_podcasts": true,
                "video_playback": false,
                "manifest_formats": ["file_ids_mp4", "file_ids_mp4_dual"],
                "supports_preferred_media_type": true,
                "supports_playback_offsets": true,
                "supports_playback_speed": true
            },
            "device_id": dev_id,
            "device_type": "computer",
            "metadata": {},
            "model": "web_player",
            "name": "Web Player (MediaHarbor)",
            "platform_identifier": "web_player linux undefined;mediaharbor;desktop",
            "is_group": false,
            "correlation_id": uuid::Uuid::new_v4().to_string(),
            "client_version": CLIENT_VERSION
        },
        "outro_endcontent_snooping": false,
        "connection_id": connection_id,
        "client_version": CLIENT_VERSION,
        "volume": 65535
    })
}

async fn post_json(
    client: &Client,
    url: &str,
    token: &str,
    client_token: Option<&str>,
    body: &Value,
    put: bool,
) -> Result<(u16, Value), String> {
    let mut req = if put {
        client.put(url)
    } else {
        client.post(url)
    };
    req = req
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("origin", "https://open.spotify.com")
        .header("referer", "https://open.spotify.com/")
        .json(body);
    if let Some(ct) = client_token {
        req = req.header("client-token", ct);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let parsed = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
    Ok((status, parsed))
}

fn body_excerpt(body: &Value) -> String {
    serde_json::to_string(body).unwrap_or_default()
}

pub async fn report_playback(
    client: Client,
    access_token: String,
    client_token: Option<String>,
    track_id: String,
    duration_secs: u64,
    context: SpotifyPlayContext,
    history: bool,
    telemetry: bool,
) -> Result<String, String> {
    let mut summary = String::new();
    let effective_license;
    let track_uri = format!("spotify:track:{track_id}");
    let (mut ws, connection_id) = dealer_connect(&access_token)
        .await
        .ok_or_else(|| "dealer websocket did not deliver a connection id".to_string())?;

    let dev_id = device_id();
    let reg = register_body(&dev_id, &connection_id);
    let spclient = crate::services::spotify::endpoints::spclient_base(&client).await;
    let reg_url = format!("{spclient}/track-playback/v1/devices");
    let mut seq = match post_json(
        &client,
        &reg_url,
        &access_token,
        client_token.as_deref(),
        &reg,
        false,
    )
    .await
    {
        Ok((s, body)) if (200..300).contains(&s) => {
            effective_license = body
                .get("effective_license")
                .and_then(|x| x.as_str())
                .unwrap_or("?")
                .to_string();
            body.get("initial_seq_num")
                .and_then(|x| x.as_u64())
                .unwrap_or(1)
        }
        Ok((s, body)) => {
            return Err(format!(
                "track-playback device register returned {s}: {}",
                body_excerpt(&body)
            ))
        }
        Err(e) => {
            return Err(format!(
                "track-playback device register failed to send: {e}"
            ))
        }
    };

    if history {
        let container = context.context_uri.clone();
        let ctx_uri = container.clone().unwrap_or_else(|| track_uri.clone());
        let skip_to = container.map(|_| context.track_index.unwrap_or(0));
        let cmd = play_command(&ctx_uri, &track_uri, skip_to);
        let cmd_url =
            format!("{spclient}/connect-state/v1/player/command/from/{dev_id}/to/{dev_id}");
        match post_json(
            &client,
            &cmd_url,
            &access_token,
            client_token.as_deref(),
            &cmd,
            false,
        )
        .await
        {
            Ok((s, body)) if !(200..300).contains(&s) => {
                return Err(format!(
                    "connect-state play command returned {s}: {}",
                    body_excerpt(&body)
                ))
            }
            Err(e) => return Err(format!("connect-state play command failed to send: {e}")),
            _ => {}
        }

        let (mut machine, index) = await_replace_state(&mut ws).await.ok_or_else(|| {
            "dealer did not deliver a replace_state for the play command".to_string()
        })?;

        let duration_ms = track_duration_ms(&machine, index)
            .unwrap_or_else(|| duration_secs.saturating_mul(1000).max(THRESHOLD_MS + 1));
        let played_ms = duration_secs
            .saturating_mul(1000)
            .clamp(THRESHOLD_MS + 1, duration_ms);
        let state_url = format!("{spclient}/track-playback/v1/devices/{dev_id}/state");

        let resolved = resolved_track_uri(&machine, index).unwrap_or_default();
        summary = format!(
            "context={ctx_uri} skip_to={} resolved={} match={} played={}s/{}s license={effective_license}",
            skip_to.map(|i| i.to_string()).unwrap_or_else(|| "none".into()),
            if resolved.is_empty() { "?" } else { &resolved },
            resolved == track_uri,
            played_ms / 1000,
            duration_ms / 1000,
        );

        let state_id = machine
            .pointer(&format!("/states/{index}/state_id"))
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let started_at = tokio::time::Instant::now();

        let transitions: [(&str, u64, u64, u64, bool, bool); 6] = [
            ("before_track_load", 0, 0, 0, false, false),
            ("speed_changed", 1_000, 1, 0, false, true),
            ("resume", 100, 1, 22, false, true),
            ("pause", 40, 0, 22, true, true),
            ("started_playing", 1_000, 0, 1_027, true, true),
            ("played_threshold_reached", 29_000, 0, 30_053, true, true),
        ];
        for (src, delay_ms, speed, position, paused, prev) in transitions {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            seq += 1;
            let mut body = json!({
                "seq_num": seq,
                "state_ref": {
                    "state_machine_id": machine
                        .pointer("/state_machine_id")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default(),
                    "state_id": state_id,
                    "paused": paused,
                },
                "sub_state": {
                    "playback_speed": speed,
                    "position": position,
                    "duration": duration_ms,
                    "media_type": "AUDIO",
                    "bitrate": 256000,
                    "audio_quality": "VERY_HIGH",
                    "format": 11,
                    "is_video_on": false
                },
                "debug_source": src
            });
            if prev {
                body["previous_position"] = json!(position);
            }
            let (status, resp) = post_json(
                &client,
                &state_url,
                &access_token,
                client_token.as_deref(),
                &body,
                true,
            )
            .await
            .map_err(|e| format!("track-playback state '{src}' failed to send: {e}"))?;
            if !(200..300).contains(&status) {
                return Err(format!(
                    "track-playback state '{src}' returned {status}: {}",
                    body_excerpt(&resp)
                ));
            }
            if let Some(m) = resp.get("state_machine") {
                machine = m.clone();
            }
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
        seq += 1;
        let final_position = played_ms.min(elapsed_ms(started_at));
        let mut body = json!({
            "seq_num": seq,
            "state_ref": {
                "state_machine_id": machine
                    .pointer("/state_machine_id")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default(),
                "state_id": state_id,
                "paused": true,
            },
            "sub_state": {
                "playback_speed": 0,
                "position": final_position,
                "duration": duration_ms,
                "media_type": "AUDIO",
                "bitrate": 256000,
                "audio_quality": "VERY_HIGH",
                "format": 11,
                "is_video_on": false
            },
            "debug_source": "track_data_finalized"
        });
        body["previous_position"] = json!(final_position);
        body["playback_stats"] = playback_stats(final_position);
        let (status, resp) = post_json(
            &client,
            &state_url,
            &access_token,
            client_token.as_deref(),
            &body,
            true,
        )
        .await
        .map_err(|e| format!("track-playback finalize failed to send: {e}"))?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "track-playback finalize returned {status}: {}",
                body_excerpt(&resp)
            ));
        }
        summary.push_str(&format!(" finalized_at={}s", final_position / 1000));
    }

    if telemetry {
        let _ = post_melody(&client, &access_token, client_token.as_deref(), &dev_id).await;
    }

    let _ = ws.close(None).await;
    if summary.is_empty() {
        summary = format!("telemetry only, license={effective_license}");
    }
    Ok(summary)
}

async fn post_melody(
    client: &Client,
    token: &str,
    client_token: Option<&str>,
    dev_id: &str,
) -> Option<u16> {
    let body = json!({
        "messages": [{
            "type": "jssdk_connect_command",
            "message": {
                "ms_ack_duration": 1000,
                "ms_request_latency": 300,
                "command_id": uuid::Uuid::new_v4().simple().to_string(),
                "command_type": "play",
                "target_device_brand": "spotify",
                "target_device_model": "web_player",
                "target_device_id": dev_id,
                "interaction_ids": uuid::Uuid::new_v4().to_string(),
                "result": "success",
                "http_response": "",
                "http_status_code": 200
            }
        }],
        "sdk_id": CLIENT_VERSION,
        "platform": "web_player linux undefined;mediaharbor;desktop"
    });
    let spclient = crate::services::spotify::endpoints::spclient_base(client).await;
    let mut req = client
        .post(format!("{spclient}/melody/v1/msg/batch"))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "text/plain;charset=UTF-8")
        .header("accept", "*/*")
        .body(serde_json::to_string(&body).ok()?);
    if let Some(ct) = client_token {
        req = req.header("client-token", ct);
    }
    Some(req.send().await.ok()?.status().as_u16())
}

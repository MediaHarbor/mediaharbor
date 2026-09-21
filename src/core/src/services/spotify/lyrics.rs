use serde_json::Value;

use crate::errors::MhResult;
use crate::services::common::library::string_at;
use crate::services::common::lyrics::{WordLyrics, WordSyncedLine, WordTiming};

const COLOR_LYRICS_URL: &str = "https://spclient.wg.spotify.com/color-lyrics/v2/track";

/// Spotify reports timings as either a numeric or a stringified millisecond count
/// depending on the field and the client that minted the response.
fn ms(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
}

/// Turns a `color-lyrics/v2` payload into the shared timed-lyrics shape. Syllable
/// timings are kept when Spotify publishes them, which is what makes word-synced
/// TTML possible for this service.
pub fn parse_color_lyrics(body: &Value) -> Option<WordLyrics> {
    let lines = body["lyrics"]["lines"].as_array()?;
    let parsed: Vec<WordSyncedLine> = lines
        .iter()
        .map(|l| {
            let start = ms(&l["startTimeMs"]).unwrap_or(0.0) / 1000.0;
            let words: Vec<WordTiming> = l["syllables"]
                .as_array()
                .map(|syls| {
                    syls.iter()
                        .filter_map(|s| {
                            let text = s["chars"]
                                .as_str()
                                .or_else(|| s["text"].as_str())
                                .or_else(|| s["words"].as_str())
                                .unwrap_or("")
                                .trim();
                            if text.is_empty() {
                                return None;
                            }
                            let ws = ms(&s["startTimeMs"]).unwrap_or(0.0) / 1000.0;
                            let we = ms(&s["endTimeMs"]).unwrap_or(ws * 1000.0) / 1000.0;
                            Some(WordTiming {
                                start: ws,
                                end: we.max(ws),
                                text: text.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let end = match ms(&l["endTimeMs"]).filter(|v| *v > 0.0) {
                Some(v) => v / 1000.0,
                None => words.last().map(|w| w.end).unwrap_or(start),
            };
            WordSyncedLine {
                start_time: start,
                end_time: end,
                text: string_at(l, &["/words"]),
                words,
            }
        })
        .collect();

    (!parsed.is_empty()).then_some(WordLyrics { lines: parsed })
}

/// `Ok(None)` means Spotify has no lyrics for this track; `Err` carries the real
/// upstream response so a bad token is distinguishable from an instrumental.
pub async fn fetch_color_lyrics(
    access_token: &str,
    track_id: &str,
) -> MhResult<Option<WordLyrics>> {
    let http = crate::http_client::build_client()?;
    let resp = http
        .get(format!("{COLOR_LYRICS_URL}/{track_id}"))
        .header("Authorization", format!("Bearer {access_token}"))
        .header("App-Platform", "WebPlayer")
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(crate::services::common::http::api_error(
            &format!("Spotify color-lyrics for {track_id}"),
            status,
            &body,
        ));
    }
    let body: Value = resp.json().await?;
    Ok(parse_color_lyrics(&body))
}

pub struct SpotifyLyrics;

#[async_trait::async_trait]
impl crate::services::common::lyrics::LyricsProvider for SpotifyLyrics {
    async fn fetch(
        &self,
        req: &crate::ipc_contract::GetLyricsRequest,
        _settings: &crate::defaults::Settings,
        state: &crate::BackendState,
    ) -> crate::ipc_contract::GetLyricsResponse {
        use crate::services::common::lyrics::empty_response;

        let track_id = crate::extract_spotify_id(&req.url).unwrap_or_default();
        let token = state.librespot.read().await.cached_access_token();
        let (false, Some(token)) = (track_id.is_empty(), token) else {
            return empty_response();
        };
        match fetch_color_lyrics(&token, &track_id).await {
            Ok(Some(lyrics)) => crate::ipc_contract::GetLyricsResponse {
                synced: lyrics.to_lrc(),
                plain: lyrics.to_plain(),
                word_synced: lyrics
                    .has_word_timings()
                    .then(|| lyrics.to_json())
                    .flatten(),
            },
            Ok(None) => empty_response(),
            Err(e) => {
                state
                    .emitter
                    .emit_log(&crate::ipc_contract::BackendLogEvent::new(
                        "warning",
                        "spotify",
                        "Lyrics",
                        e.to_string(),
                    ));
                empty_response()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syllables_become_word_timings() {
        let body = serde_json::json!({
            "lyrics": {
                "lines": [
                    {
                        "startTimeMs": "1500",
                        "endTimeMs": "3000",
                        "words": "Hello world",
                        "syllables": [
                            {"startTimeMs": "1500", "endTimeMs": "2000", "chars": "Hello"},
                            {"startTimeMs": "2100", "endTimeMs": "3000", "chars": "world"}
                        ]
                    }
                ]
            }
        });
        let parsed = parse_color_lyrics(&body).expect("parsed");
        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].start_time, 1.5);
        assert_eq!(parsed.lines[0].words.len(), 2);
        assert_eq!(parsed.lines[0].words[1].text, "world");
        assert!(parsed.has_word_timings());
    }

    #[test]
    fn line_only_payloads_still_parse() {
        let body = serde_json::json!({
            "lyrics": {
                "lines": [
                    {"startTimeMs": 1000, "endTimeMs": 0, "words": "Just a line"}
                ]
            }
        });
        let parsed = parse_color_lyrics(&body).expect("parsed");
        assert!(!parsed.has_word_timings());
        assert_eq!(parsed.lines[0].end_time, 1.0);
        assert_eq!(parsed.to_lrc().unwrap(), "[00:01.00]Just a line");
    }

    #[test]
    fn a_payload_without_lines_is_not_lyrics() {
        assert!(parse_color_lyrics(&serde_json::json!({"lyrics": {}})).is_none());
    }
}

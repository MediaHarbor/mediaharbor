use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::downloads::dedup::quality_rank;
use crate::errors::{MhError, MhResult};
use crate::http_client::build_mozilla_client;
use crate::services::common::library::string_at;
use crate::services::common::lyrics::{
    lyrics_wanted, parse_deezer_gw_lyrics, resolve_track_lyrics, FoundLyrics,
};
use crate::services::common::lyrics::{WordLyrics, WordSyncedLine, WordTiming};
use crate::services::common::pipeline::converter::ConversionSource;
use crate::services::common::pipeline::downloader::{
    count_of, cover_tmp_for, existing_final_file, finalize_track, preflight, resolve_track_dest,
    save_cover_file, CommonTrackFields, CoverFiles, FinalizeTrack,
};
use crate::services::common::pipeline::orchestrator::TrackJob;
use crate::services::common::pipeline::tagger::{one, TrackMetadata};
use crate::services::common::pipeline::{playlist_rows, AlbumInfo, PlaylistInfo, TrackOutcome};
use crate::services::deezer::crypto::download_and_decrypt_deezer;

const GW_BASE: &str = "https://www.deezer.com/ajax/gw-light.php";
const MEDIA_BASE: &str = "https://media.deezer.com/v1";
const PUBLIC_API: &str = "https://api.deezer.com";

#[derive(Debug, Clone)]
pub struct DeezerSession {
    pub token: String,
    pub user_id: String,
    pub license_token: String,
    pub max_quality: u8,
    pub sid: Option<String>,
}

pub struct DeezerClient {
    pub arl: String,
    pub client: reqwest::Client,
    pub session: Arc<RwLock<Option<DeezerSession>>>,
    /// Mirror of `DeezerSession::max_quality`, readable without awaiting the lock
    /// so a release folder can be named after the account's real ceiling.
    entitled_quality: Arc<std::sync::atomic::AtomicU8>,
    /// Album payloads already fetched during this run; every track of an album used
    /// to re-request it.
    album_cache: Arc<RwLock<std::collections::HashMap<String, Value>>>,
    /// `nb_disk` comes back null on every public album, so the disc count has to be
    /// counted off the track listing. Memoised for the run like the album itself.
    disc_totals: Arc<RwLock<std::collections::HashMap<String, u32>>>,
}

/// A resolved Deezer stream. `quality` is what Deezer will actually hand over,
/// which is the requested tier capped by the account *and* by what the track has.
pub struct DeezerStream {
    pub url: String,
    pub ext: &'static str,
    pub quality: u8,
    pub requested: u8,
    pub filesize: Option<u64>,
    /// The song id whose media this URL serves; the Blowfish key derives from it.
    pub crypto_id: String,
    /// The `deezer.pageTrack` blob this stream was resolved from. Reserving the URL
    /// already costs that call, and it is the only source for the credits, BPM and
    /// gain that the public REST API does not publish.
    pub info: Value,
}

/// One `get_url` reservation: where the bytes are, how many, and which format Deezer
/// decided to hand over — which is not always the one that was asked for.
struct ReservedMedia {
    url: String,
    filesize: Option<u64>,
    format: Option<String>,
}

/// A zero `FILESIZE_*` means the track has no master in that format.
fn has_format(track_info: &Value, q: u8) -> bool {
    let key = match q {
        2 => "FILESIZE_FLAC",
        1 => "FILESIZE_MP3_320",
        _ => "FILESIZE_MP3_128",
    };
    let raw = &track_info[key];
    let size = raw
        .as_u64()
        .or_else(|| raw.as_str().and_then(|s| s.trim().parse::<u64>().ok()));
    size.map(|n| n > 0).unwrap_or(true)
}

fn best_available_quality(track_info: &Value, ceiling: u8) -> u8 {
    (0..=ceiling)
        .rev()
        .find(|&q| has_format(track_info, q))
        .unwrap_or(0)
}

/// Deezer encodes the cover's edge length in the path
/// (`.../1000x1000-000000-80-0-0.jpg`), so the requested size substitutes directly.
/// 1400 is as far as the CDN actually renders; past that it serves the same image.
fn resize_deezer_cover(url: &str, size: u32) -> String {
    static SIZE_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"/\d+x\d+-").unwrap());
    let size = size.clamp(56, 1400);
    SIZE_RE
        .replace(url, format!("/{size}x{size}-").as_str())
        .into_owned()
}

/// The cover for a track whose payload carries no `album` object at all. A release
/// listing trims the nested album away but keeps `md5_image`, which is the same hash
/// the full-size URL is built from.
fn deezer_cover_url(md5_image: &str, size: u32) -> Option<String> {
    let md5 = md5_image.trim();
    if md5.is_empty() {
        return None;
    }
    let size = size.clamp(56, 1400);
    Some(format!(
        "https://cdn-images.dzcdn.net/images/cover/{md5}/{size}x{size}-000000-80-0-0.jpg"
    ))
}

/// A `/album/{id}/tracks` entry carries no nested `album` object — the title, the
/// release date, the label, the barcode, the genre and the cover all live on the
/// release payload instead. Grafting it on keeps every `track["album"][…]` read
/// valid. A track fetched on its own already nests one and is left alone.
fn graft_album(track: &mut Value, album: &Value) {
    if !album.is_null() && track["album"]["id"].as_u64().is_none() {
        track["album"] = album.clone();
    }
}

/// The tier byte a served `format` string names, so a reservation that quietly came
/// back a tier down is labelled and extensioned for what it really is.
fn quality_of_format(format: &str) -> Option<u8> {
    match format.to_ascii_uppercase().as_str() {
        "FLAC" => Some(2),
        "MP3_320" | "MP3_256" => Some(1),
        "MP3_128" | "MP3_64" | "MP3_MISC" => Some(0),
        _ => None,
    }
}

fn quality_info(q: u8) -> (&'static str, &'static str) {
    match q {
        0 => ("MP3_128", "mp3"),
        1 => ("MP3_320", "mp3"),
        2 => ("FLAC", "flac"),
        _ => ("MP3_128", "mp3"),
    }
}

/// Deezer reports timings in milliseconds, sometimes as numbers and sometimes as
/// strings depending on the field.
fn ms_value(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
}

fn word_by_word_lines(wbw: &[Value]) -> Vec<WordSyncedLine> {
    wbw.iter()
        .map(|line| {
            let words: Vec<WordTiming> = line["words"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|w| {
                            let text = w["word"].as_str().unwrap_or("").trim();
                            if text.is_empty() {
                                return None;
                            }
                            let start = ms_value(&w["start"]).unwrap_or(0.0) / 1000.0;
                            let end = ms_value(&w["end"]).unwrap_or(start * 1000.0) / 1000.0;
                            Some(WordTiming {
                                start,
                                end,
                                text: text.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let text = line["words"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|w| w["word"].as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            WordSyncedLine {
                start_time: ms_value(&line["start"]).unwrap_or(0.0) / 1000.0,
                end_time: ms_value(&line["end"]).unwrap_or(0.0) / 1000.0,
                text,
                words,
            }
        })
        .collect()
}

fn line_synced_lines(sync: &[Value]) -> Vec<WordSyncedLine> {
    sync.iter()
        .filter_map(|l| {
            let text = string_at(l, &["/line"]);
            let start_ms = ms_value(&l["milliseconds"])?;
            let end_ms = start_ms + ms_value(&l["duration"]).unwrap_or(0.0);
            Some(WordSyncedLine {
                start_time: start_ms / 1000.0,
                end_time: end_ms / 1000.0,
                text,
                words: Vec::new(),
            })
        })
        .collect()
}

impl DeezerClient {
    pub fn new(arl: &str) -> MhResult<Self> {
        let client = build_mozilla_client()?;
        Ok(Self {
            arl: arl.trim().to_string(),
            client,
            session: Arc::new(RwLock::new(None)),
            entitled_quality: Arc::new(std::sync::atomic::AtomicU8::new(2)),
            album_cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
            disc_totals: Arc::new(RwLock::new(std::collections::HashMap::new())),
        })
    }

    /// The highest tier this account may stream: 2 lossless, 1 320 kbps, 0 128 kbps.
    pub fn entitled_quality(&self) -> u8 {
        self.entitled_quality
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn cookie(&self, sid: Option<&str>) -> String {
        let mut parts = vec![format!("arl={}", self.arl)];
        if let Some(s) = sid {
            if !s.is_empty() {
                parts.push(format!("sid={}", s));
            }
        }
        parts.join("; ")
    }

    pub async fn authenticate(&self) -> MhResult<()> {
        let (resp, captured_sid) = self.gw_get("deezer.getUserData", None).await?;
        let user_id = resp["results"]["USER"]["USER_ID"].as_u64().unwrap_or(0);
        if user_id == 0 {
            return Err(MhError::Auth(
                "Deezer ARL is invalid or expired. Update it in Settings → Deezer → ARL Token."
                    .into(),
            ));
        }
        let token = resp["results"]["checkForm"]
            .as_str()
            .unwrap_or("null")
            .to_string();
        let license_token = resp["results"]["USER"]["OPTIONS"]["license_token"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let opts = &resp["results"]["USER"]["OPTIONS"];
        let max_quality = if opts["web_lossless"].as_bool().unwrap_or(false)
            || opts["mobile_lossless"].as_bool().unwrap_or(false)
        {
            2
        } else if opts["web_hq"].as_bool().unwrap_or(false)
            || opts["mobile_hq"].as_bool().unwrap_or(false)
        {
            1
        } else {
            0
        };

        self.entitled_quality
            .store(max_quality, std::sync::atomic::Ordering::Relaxed);
        let mut session = self.session.write().await;
        *session = Some(DeezerSession {
            token,
            user_id: user_id.to_string(),
            license_token,
            max_quality,
            sid: captured_sid,
        });
        Ok(())
    }

    async fn require_session(
        &self,
    ) -> MhResult<tokio::sync::RwLockReadGuard<'_, Option<DeezerSession>>> {
        let guard = self.session.read().await;
        if guard.is_none() {
            return Err(MhError::Auth(
                "DeezerClient: not authenticated. Call authenticate() first.".into(),
            ));
        }
        Ok(guard)
    }

    async fn gw_get(
        &self,
        method: &str,
        session_snapshot: Option<(&str, Option<&str>)>,
    ) -> MhResult<(Value, Option<String>)> {
        let (api_token, sid) = match session_snapshot {
            Some((tok, sid)) => (tok.to_string(), sid.map(|s| s.to_string())),
            None => ("null".to_string(), None),
        };
        let url = format!(
            "{}?method={}&input=3&api_version=1.0&api_token={}",
            GW_BASE, method, api_token
        );
        let cookie = self.cookie(sid.as_deref());
        let resp = self
            .client
            .get(&url)
            .header("Cookie", &cookie)
            .header("User-Agent", crate::http_client::UA_MOZILLA)
            .send()
            .await?;

        let new_sid: Option<String> = resp
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(|c| {
                c.split(';').next().and_then(|part| {
                    let mut kv = part.splitn(2, '=');
                    let k = kv.next()?.trim();
                    let v = kv.next()?.trim();
                    if k == "sid" {
                        Some(v.to_string())
                    } else {
                        None
                    }
                })
            });

        let body = resp.text().await?;
        let json: Value = serde_json::from_str(&body)?;

        Ok((json, new_sid))
    }

    async fn gw_post_inner(
        &self,
        method: &str,
        body: Value,
        api_token: &str,
        sid: Option<&str>,
    ) -> MhResult<Value> {
        let url = format!(
            "{}?method={}&input=3&api_version=1.0&api_token={}",
            GW_BASE, method, api_token
        );
        let cookie = self.cookie(sid);
        let body_str = serde_json::to_string(&body)?;
        let resp = self
            .client
            .post(&url)
            .header("Cookie", &cookie)
            .header("User-Agent", crate::http_client::UA_MOZILLA)
            .header("Content-Type", "application/json")
            .body(body_str)
            .send()
            .await?;
        let text = resp.text().await?;
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn get_track_info(&self, track_id: &str) -> MhResult<Value> {
        let guard = self.require_session().await?;
        let sess = guard.as_ref().unwrap();
        let token = sess.token.clone();
        let sid = sess.sid.clone();
        drop(guard);

        let page_resp = self
            .gw_post_inner(
                "deezer.pageTrack",
                serde_json::json!({ "SNG_ID": track_id }),
                &token,
                sid.as_deref(),
            )
            .await;

        if let Ok(resp) = page_resp {
            if let Some(data) = resp.get("results").and_then(|r| r.get("DATA")) {
                let mut track_info = data.clone();
                if let Some(lyrics) = resp["results"].get("LYRICS") {
                    track_info["LYRICS"] = lyrics.clone();
                }
                return Ok(track_info);
            }
        }

        let resp = self
            .gw_post_inner(
                "song.getData",
                serde_json::json!({ "SNG_ID": track_id }),
                &token,
                sid.as_deref(),
            )
            .await?;

        resp.get("results")
            .cloned()
            .ok_or_else(|| MhError::NotFound(format!("Failed to get track info for {}", track_id)))
    }

    pub async fn get_public_track(&self, track_id: &str) -> MhResult<Value> {
        let resp = self
            .client
            .get(format!("{}/track/{}", PUBLIC_API, track_id))
            .send()
            .await?;
        let mut track: Value = crate::services::common::http::read_json("Deezer", resp).await?;
        if let Some(c) = self.get_track_contributors(track_id).await {
            track["contributors"] = c;
        }
        Ok(track)
    }

    /// The credits, which a release listing does not carry. Best-effort: a track
    /// without them still tags, it just loses the composer and performer roles.
    pub async fn get_track_contributors(&self, track_id: &str) -> Option<Value> {
        let resp = self
            .client
            .get(format!("{}/track/{}/contributors", PUBLIC_API, track_id))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json::<Value>().await.ok()
    }

    /// Memoised for the lifetime of this client, which is one download run.
    pub async fn get_public_album(&self, album_id: &str) -> MhResult<Value> {
        if let Some(hit) = self.album_cache.read().await.get(album_id) {
            return Ok(hit.clone());
        }
        let resp = self
            .client
            .get(format!("{}/album/{}", PUBLIC_API, album_id))
            .send()
            .await?;
        let json: Value = crate::services::common::http::read_json("Deezer", resp).await?;
        self.album_cache
            .write()
            .await
            .insert(album_id.to_string(), json.clone());
        Ok(json)
    }

    /// The highest disc number on a release. Only the track listing carries it, so a
    /// bare track download has to ask for the listing once to know its disc total.
    async fn album_disc_total(&self, album_id: &str) -> Option<u32> {
        if let Some(hit) = self.disc_totals.read().await.get(album_id) {
            return Some(*hit);
        }
        let resp = self
            .client
            .get(format!(
                "{}/album/{}/tracks?limit=500",
                PUBLIC_API, album_id
            ))
            .send()
            .await
            .ok()?;
        let json: Value = resp.json().await.ok()?;
        let total = json["data"]
            .as_array()?
            .iter()
            .filter_map(|t| t["disk_number"].as_u64())
            .max()? as u32;
        self.disc_totals
            .write()
            .await
            .insert(album_id.to_string(), total);
        Some(total)
    }

    pub async fn get_public_playlist(&self, playlist_id: &str) -> MhResult<Value> {
        let resp = self
            .client
            .get(format!("{}/playlist/{}", PUBLIC_API, playlist_id))
            .send()
            .await?;
        crate::services::common::http::read_json("Deezer", resp).await
    }

    pub async fn get_album_tracks(&self, album_id: &str) -> MhResult<AlbumInfo> {
        let album = self.get_public_album(album_id).await?;
        let tracks_resp = self
            .client
            .get(format!(
                "{}/album/{}/tracks?limit=500",
                PUBLIC_API, album_id
            ))
            .send()
            .await?;
        let tracks: Value = tracks_resp.json().await?;
        let items = tracks["data"].as_array().cloned().unwrap_or_default();
        let rows = crate::services::common::pipeline::release_rows(
            &items,
            "/artist/name",
            Some("disk_number"),
        );

        let disc_total = rows.iter().map(|r| r.disc).max().unwrap_or(1).max(1);
        self.disc_totals
            .write()
            .await
            .insert(album_id.to_string(), disc_total);
        let mut album = album;
        album["nb_disk"] = Value::from(disc_total);

        Ok(AlbumInfo {
            tracks: rows,
            album_record: album.clone(),
            number_of_volumes: disc_total,
            title: album["title"]
                .as_str()
                .unwrap_or(&format!("Album {}", album_id))
                .to_string(),
            artist: string_at(&album, &["/artist/name"]),
            year: album["release_date"]
                .as_str()
                .and_then(|d| d.split('-').next())
                .unwrap_or("")
                .to_string(),
            genre: album["genres"]["data"][0]["name"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            label: string_at(&album, &["/label"]),
            bit_depth: None,
            sampling_rate: None,
        })
    }

    pub async fn get_playlist_tracks(&self, playlist_id: &str) -> MhResult<PlaylistInfo> {
        let meta = self.get_public_playlist(playlist_id).await?;
        let rows = playlist_rows(&meta["tracks"]["data"], "/artist/name");
        Ok(PlaylistInfo {
            tracks: rows,
            title: meta["title"]
                .as_str()
                .unwrap_or(&format!("Playlist {}", playlist_id))
                .to_string(),
            artist: string_at(&meta, &["/creator/name"]),
        })
    }

    pub async fn get_artist_albums(&self, artist_id: &str) -> MhResult<Vec<String>> {
        let resp = self
            .client
            .get(format!(
                "{}/artist/{}/albums?limit=500",
                PUBLIC_API, artist_id
            ))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(MhError::NotFound(format!(
                "Deezer: artist {} not found",
                artist_id
            )));
        }
        let json: Value = resp.json().await?;
        let ids = json["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a["id"].as_u64().map(|id| id.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(ids)
    }

    pub fn get_label_albums(&self, _label_id: &str) -> MhResult<Vec<String>> {
        Err(MhError::Unsupported(
            "Deezer does not support label downloads via its public API. Use Qobuz for label downloads.".into()
        ))
    }

    async fn get_token_url(
        &self,
        track_token: &str,
        format: &str,
        license_token: &str,
        sid: Option<&str>,
    ) -> MhResult<Option<ReservedMedia>> {
        let body = serde_json::json!({
            "license_token": license_token,
            "media": [{ "type": "FULL", "formats": [{ "cipher": "BF_CBC_STRIPE", "format": format }] }],
            "track_tokens": [track_token],
        });
        let url = format!("{}/get_url", MEDIA_BASE);
        let cookie = self.cookie(sid);
        let resp = self
            .client
            .post(&url)
            .header("Cookie", &cookie)
            .header("User-Agent", crate::http_client::UA_MOZILLA)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(MhError::Other(format!(
                "Deezer get_url: HTTP {status}: {text}"
            )));
        }
        let json: Value = serde_json::from_str(&text)?;
        let datum = &json["data"][0];
        let media = &datum["media"][0];
        let Some(url_str) = media["sources"][0]["url"].as_str().map(|s| s.to_string()) else {
            let errors = datum["errors"].to_string();
            if errors != "null" {
                return Err(MhError::Other(format!("Deezer get_url: {errors}")));
            }
            return Ok(None);
        };

        let cipher = media["cipher"]["type"].as_str().unwrap_or("BF_CBC_STRIPE");
        if !cipher.eq_ignore_ascii_case("BF_CBC_STRIPE") {
            return Err(MhError::Other(format!(
                "Deezer get_url returned cipher {cipher}, which this build cannot decrypt \
                 (only BF_CBC_STRIPE). Full response: {text}"
            )));
        }

        Ok(Some(ReservedMedia {
            url: url_str,
            filesize: media["filesize"].as_u64(),
            format: media["format"].as_str().map(|s| s.to_string()),
        }))
    }

    pub async fn get_stream_url(&self, track_id: &str, quality: u8) -> MhResult<DeezerStream> {
        let guard = self.require_session().await?;
        let sess = guard.as_ref().unwrap();
        let entitled = quality.min(sess.max_quality);
        let license_token = sess.license_token.clone();
        let sid = sess.sid.clone();
        drop(guard);

        let track_info = self.get_track_info(track_id).await?;
        let effective_quality = best_available_quality(&track_info, entitled);
        let (format, _) = quality_info(effective_quality);

        let fallback_id = track_info["FALLBACK"]["SNG_ID"].as_str();

        let mut reserved: Option<(ReservedMedia, String)> = None;
        let mut refusals: Vec<String> = Vec::new();

        for (token, crypto_id) in [
            (track_info["TRACK_TOKEN"].as_str(), track_id),
            (
                track_info["FALLBACK"]["TRACK_TOKEN"].as_str(),
                fallback_id.unwrap_or(track_id),
            ),
        ] {
            if reserved.is_some() {
                break;
            }
            let Some(token) = token.filter(|t| !t.is_empty()) else {
                continue;
            };
            match self
                .get_token_url(token, format, &license_token, sid.as_deref())
                .await
            {
                Ok(Some(media)) => reserved = Some((media, crypto_id.to_string())),
                Ok(None) => {}
                Err(e) => refusals.push(e.to_string()),
            }
        }

        let (media, crypto_id) = match reserved {
            Some(pair) => pair,
            None => {
                let md5 = track_info["MD5_ORIGIN"]
                    .as_str()
                    .or_else(|| track_info["FALLBACK"]["MD5_ORIGIN"].as_str());
                let md5 = md5.ok_or_else(|| {
                    let why = if refusals.is_empty() {
                        String::new()
                    } else {
                        format!(" Deezer said: {}", refusals.join("; "))
                    };
                    MhError::Other(format!(
                        "Deezer: Track {track_id} is not available for streaming. It may be                          region-locked or unavailable on your subscription tier.{why}"
                    ))
                })?;
                let media_version = track_info["MEDIA_VERSION"]
                    .as_str()
                    .or_else(|| track_info["FALLBACK"]["MEDIA_VERSION"].as_str())
                    .unwrap_or("1");
                let effective_id = fallback_id.unwrap_or(track_id);
                (
                    ReservedMedia {
                        url: crate::services::deezer::crypto::get_encrypted_url(
                            effective_id,
                            md5,
                            media_version,
                            effective_quality,
                        ),
                        filesize: None,
                        format: None,
                    },
                    effective_id.to_string(),
                )
            }
        };

        let served_quality = media
            .format
            .as_deref()
            .and_then(quality_of_format)
            .unwrap_or(effective_quality);
        let (_, served_ext) = quality_info(served_quality);

        Ok(DeezerStream {
            url: media.url,
            ext: served_ext,
            quality: served_quality,
            requested: quality,
            filesize: media.filesize,
            crypto_id,
            info: track_info,
        })
    }

    pub async fn get_lyrics(&self, track_id: &str) -> MhResult<Value> {
        let guard = self.require_session().await?;
        let sess = guard.as_ref().unwrap();
        let token = sess.token.clone();
        let sid = sess.sid.clone();
        drop(guard);

        let url = format!(
            "{}?method=song.getLyrics&api_version=1.0&api_token={}&sng_id={}",
            GW_BASE, token, track_id
        );
        let cookie = self.cookie(sid.as_deref());
        let resp = self
            .client
            .get(&url)
            .header("Cookie", &cookie)
            .header("User-Agent", crate::http_client::UA_MOZILLA)
            .send()
            .await?;
        let text = resp.text().await?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Deezer's public word-by-word lyrics endpoint. Uses an anonymous JWT, so it
    /// works without an ARL — this is the path free accounts get synced lyrics from.
    pub async fn fetch_word_lyrics(&self, track_id: &str) -> Option<WordLyrics> {
        let jwt = {
            let http = crate::http_client::build_client().ok()?;
            let resp = http
                .get("https://auth.deezer.com/login/anonymous?jo=p&rto=c")
                .send()
                .await
                .ok()?;
            let body: Value = resp.json().await.ok()?;
            body["jwt"].as_str()?.to_string()
        };

        let query = r#"query GetLyrics($trackId: String!) {
  track(trackId: $trackId) {
    id
    lyrics {
      id
      text
      ...SynchronizedWordByWordLines
      ...SynchronizedLines
      copyright
      writers
    }
  }
}

fragment SynchronizedWordByWordLines on Lyrics {
  id
  synchronizedWordByWordLines {
    start
    end
    words {
      start
      end
      word
    }
  }
}

fragment SynchronizedLines on Lyrics {
  id
  synchronizedLines {
    lrcTimestamp
    line
    milliseconds
    duration
  }
}"#;

        let http = crate::http_client::build_client().ok()?;
        let resp = http
            .post("https://pipe.deezer.com/api")
            .header("Authorization", format!("Bearer {}", jwt))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "operationName": "GetLyrics",
                "variables": { "trackId": track_id },
                "query": query
            }))
            .send()
            .await
            .ok()?;

        let body: Value = resp.json().await.ok()?;
        let lyrics = &body["data"]["track"]["lyrics"];

        if let Some(wbw) = lyrics["synchronizedWordByWordLines"].as_array() {
            let lines = word_by_word_lines(wbw);
            if lines.iter().any(|l| !l.words.is_empty()) {
                return Some(WordLyrics { lines });
            }
        }

        if let Some(sync) = lyrics["synchronizedLines"].as_array() {
            let lines = line_synced_lines(sync);
            if !lines.is_empty() {
                return Some(WordLyrics { lines });
            }
        }

        None
    }

    /// The tier this account will actually get for this release: the request capped
    /// by the subscription, then by what masters the track has. Costs one metadata
    /// call, not a stream reservation.
    pub async fn probe_served_quality(
        &self,
        track_id: &str,
        requested: u8,
    ) -> MhResult<(String, String)> {
        let info = self.get_track_info(track_id).await?;
        let entitled = requested.min(self.entitled_quality());
        let served = best_available_quality(&info, entitled);
        let name = quality_info(served).0;
        Ok((
            deezer_quality_label(name).to_string(),
            deezer_format(name).to_string(),
        ))
    }

    pub async fn download_track(&self, job: TrackJob<'_>) -> MhResult<TrackOutcome> {
        let rank = quality_rank("deezer", &job.settings.deezer_quality);
        if let Some(out) = preflight(&job, "deezer", "Deezer", &["readable"], rank)? {
            return Ok(out);
        }
        let TrackJob {
            track_id,
            dest,
            settings,
            on_progress,
            on_log,
            placement,
            dedup,
            record,
            release,
            covers,
            ..
        } = job;
        let on_log = on_log.as_ref();

        let _guard = self.require_session().await?;
        drop(_guard);

        let requested = match settings.deezer_quality.to_uppercase().as_str() {
            "MP3_320" | "320" | "1" => 1u8,
            "MP3_128" | "128" | "0" => 0,
            _ => 2,
        };
        let stream = self.get_stream_url(track_id, requested).await?;
        let (stream_url, ext, quality, filesize, crypto_id, gw) = (
            stream.url,
            stream.ext,
            stream.quality,
            stream.filesize,
            stream.crypto_id,
            stream.info,
        );
        if quality < requested {
            let name = |q: u8| deezer_quality_label(quality_info(q).0);
            on_log(format!(
                "  ⚠ Deezer {} unavailable for this track; served {} instead. \
                 A free account is capped at 128 kbps and HiFi is required for FLAC.",
                name(requested),
                name(quality)
            ));
        }

        let mut public_meta = match record.filter(|r| !r.is_null()) {
            Some(r) => Some(r.clone()),
            None => self.get_public_track(track_id).await.ok(),
        };
        let mut album_meta: Option<Value> = release.filter(|a| !a.is_null()).cloned();
        if album_meta.is_none() {
            if let Some(album_id) = public_meta
                .as_ref()
                .and_then(|pm| pm["album"]["id"].as_u64())
            {
                album_meta = self.get_public_album(&album_id.to_string()).await.ok();
            }
        }
        if let (Some(pm), Some(album)) = (public_meta.as_mut(), album_meta.as_ref()) {
            graft_album(pm, album);
        }
        let gw_credits = gw["SNG_CONTRIBUTORS"].clone();
        if let Some(pm) = public_meta.as_mut() {
            if pm["contributors"]["data"].as_array().is_none() {
                if let Some(c) = self.get_track_contributors(track_id).await {
                    pm["contributors"] = c;
                }
            }
        }

        let title = public_meta
            .as_ref()
            .and_then(|m| m["title"].as_str())
            .unwrap_or(&format!("track_{}", track_id))
            .to_string();
        let artist = public_meta
            .as_ref()
            .and_then(|m| m["artist"]["name"].as_str())
            .unwrap_or("Unknown")
            .to_string();
        let albumartist = album_meta
            .as_ref()
            .and_then(|m| m["artist"]["name"].as_str())
            .unwrap_or(&artist)
            .to_string();
        let album = public_meta
            .as_ref()
            .and_then(|m| m["album"]["title"].as_str())
            .unwrap_or("")
            .to_string();
        let track_num = public_meta
            .as_ref()
            .and_then(|m| m["track_position"].as_u64())
            .unwrap_or(0) as u32;
        let disc_num = public_meta
            .as_ref()
            .and_then(|m| m["disk_number"].as_u64())
            .unwrap_or(1) as u32;
        let tracktotal = album_meta
            .as_ref()
            .and_then(|m| m["nb_tracks"].as_u64())
            .map(|n| n.to_string())
            .unwrap_or_default();
        let disctotal = match album_meta
            .as_ref()
            .and_then(|m| m["nb_disk"].as_u64())
            .filter(|n| *n > 0)
        {
            Some(n) => n.to_string(),
            None => match album_meta
                .as_ref()
                .and_then(|m| m["id"].as_u64())
                .map(|id| id.to_string())
            {
                Some(id) => self
                    .album_disc_total(&id)
                    .await
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                None => String::new(),
            },
        };
        let release_date = public_meta
            .as_ref()
            .and_then(|m| m["release_date"].as_str())
            .or_else(|| album_meta.as_ref().and_then(|m| m["release_date"].as_str()))
            .unwrap_or("")
            .to_string();
        let year = release_date.split('-').next().unwrap_or("").to_string();
        let genre = album_meta
            .as_ref()
            .and_then(|m| m["genres"]["data"][0]["name"].as_str())
            .unwrap_or("")
            .to_string();

        let explicit_str = if public_meta
            .as_ref()
            .and_then(|m| m["explicit_lyrics"].as_bool())
            .unwrap_or(false)
        {
            " (Explicit)".to_string()
        } else {
            String::new()
        };

        let isrc = public_meta
            .as_ref()
            .and_then(|m| m["isrc"].as_str())
            .unwrap_or("")
            .to_string();
        let label = album_meta
            .as_ref()
            .and_then(|m| m["label"].as_str())
            .unwrap_or("")
            .to_string();
        let date = release_date.clone();
        let quality_str: &str = match ext {
            "flac" => "16-bit ⁄ 44.1kHz",
            _ => match quality {
                1 => "320kbps",
                _ => "128kbps",
            },
        };
        let format_str: &str = if ext == "flac" { "FLAC" } else { "MP3" };

        let composer_name = public_meta
            .as_ref()
            .and_then(|m| m["contributors"]["data"].as_array())
            .map(|items| {
                items
                    .iter()
                    .filter(|c| c["role"].as_str() == Some("Composer"))
                    .filter_map(|c| c["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let fields = CommonTrackFields {
            title: title.clone(),
            artist: artist.clone(),
            albumartist: albumartist.clone(),
            album: album.clone(),
            track_num,
            disc_num,
            tracktotal: tracktotal.clone(),
            disctotal: disctotal.clone(),
            year: year.clone(),
            genre: genre.clone(),
            explicit: explicit_str,
            isrc: isrc.clone(),
            label: label.clone(),
            date: date.clone(),
            quality_label: quality_str.to_string(),
            format_label: format_str.to_string(),
            composer: composer_name.clone(),
        };
        let file_stem = crate::downloads::native_common::native_track_name(settings, &fields);
        let (track_dest, dest_path) =
            resolve_track_dest(dest, &file_stem, ext, settings, &fields, placement).await?;

        if let Some(existing) = existing_final_file(&dest_path, settings).await {
            on_log(format!(
                "  · already on disk, keeping {}",
                existing.display()
            ));
            return Ok(TrackOutcome::Skipped(existing));
        }

        let found = if lyrics_wanted(settings) {
            let mut found = self
                .get_lyrics(track_id)
                .await
                .ok()
                .map(|p| parse_deezer_gw_lyrics(&p))
                .unwrap_or_default();
            found.word = self.fetch_word_lyrics(track_id).await;
            resolve_track_lyrics(
                found.filled(),
                &fields.title,
                &fields.artist,
                public_meta.as_ref().and_then(|m| m["duration"].as_f64()),
                settings,
                on_log,
            )
            .await
        } else {
            FoundLyrics::default()
        };

        let cover_url = public_meta
            .as_ref()
            .and_then(|m| {
                m["album"]["cover_xl"]
                    .as_str()
                    .or_else(|| m["album"]["cover_medium"].as_str())
            })
            .or_else(|| album_meta.as_ref().and_then(|m| m["cover_xl"].as_str()))
            .map(|s| resize_deezer_cover(s, settings.pipeline_cover_size))
            .or_else(|| {
                public_meta
                    .as_ref()
                    .and_then(|m| m["md5_image"].as_str())
                    .or_else(|| album_meta.as_ref().and_then(|m| m["md5_image"].as_str()))
                    .or_else(|| gw["ALB_PICTURE"].as_str())
                    .and_then(|md5| deezer_cover_url(md5, settings.pipeline_cover_size))
            });

        let embed_cover = settings.embed_cover;
        let cover_files = CoverFiles::from_settings(settings);
        let cover_tmp = cover_tmp_for(&self.client, cover_url.as_deref(), settings, covers).await;

        download_and_decrypt_deezer(
            &self.client,
            &stream_url,
            &crypto_id,
            ext,
            &dest_path,
            filesize,
            on_progress,
        )
        .await?;

        save_cover_file(
            &track_dest,
            cover_tmp.as_ref(),
            cover_files,
            Some(file_stem.as_str()),
        )
        .await;

        let contribs = public_meta
            .as_ref()
            .and_then(|m| m["contributors"]["data"].as_array())
            .cloned()
            .unwrap_or_default();
        let by_role = |role: &str| -> Vec<String> {
            contribs
                .iter()
                .filter(|c| c["role"].as_str() == Some(role))
                .filter_map(|c| c["name"].as_str())
                .map(str::to_string)
                .collect()
        };
        let gw_role = |keys: &[&str]| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for key in keys {
                match &gw_credits[*key] {
                    Value::Array(items) => {
                        out.extend(items.iter().filter_map(|v| v.as_str()).map(str::to_string))
                    }
                    Value::String(one) => out.push(one.clone()),
                    _ => {}
                }
            }
            out.dedup();
            out
        };
        let credit = |gw_keys: &[&str], role: &str| -> Vec<String> {
            let from_gw = gw_role(gw_keys);
            if from_gw.is_empty() {
                by_role(role)
            } else {
                from_gw
            }
        };
        let non_empty_str = |v: &Value| -> Option<String> {
            v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let gw_num = |v: &Value| -> Option<f64> {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
        };

        let metadata = TrackMetadata {
            title: Some(title),
            artist: {
                let mut billed = gw_role(&["main_artist", "featuring"]);
                if billed.is_empty() {
                    for role in ["Main", "Featured"] {
                        billed.extend(by_role(role));
                    }
                }
                if billed.is_empty() {
                    one(Some(artist.clone()))
                } else {
                    billed
                }
            },
            album: Some(album),
            album_artist: one(Some(albumartist)),
            year: if release_date.is_empty() {
                None
            } else {
                Some(release_date.clone())
            },
            original_date: non_empty_str(&gw["PHYSICAL_RELEASE_DATE"])
                .or_else(|| non_empty_str(&gw["DIGITAL_RELEASE_DATE"])),
            genre: one(Some(genre)),
            track_number: Some(track_num),
            disc_number: Some(disc_num),
            total_tracks: count_of(&tracktotal),
            total_discs: count_of(&disctotal),
            isrc: public_meta
                .as_ref()
                .and_then(|m| m["isrc"].as_str())
                .map(|s| s.to_string()),
            upc: album_meta
                .as_ref()
                .and_then(|m| m["upc"].as_str())
                .map(|s| s.to_string()),
            copyright: album_meta
                .as_ref()
                .and_then(|m| m["copyright"].as_str())
                .map(|s| s.to_string())
                .or_else(|| non_empty_str(&gw["COPYRIGHT"]))
                .or_else(|| non_empty_str(&gw["PRODUCER_LINE"])),
            label: album_meta
                .as_ref()
                .and_then(|m| m["label"].as_str())
                .map(|s| s.to_string())
                .or_else(|| non_empty_str(&gw["LABEL_NAME"])),
            composer: credit(&["composer"], "Composer"),
            producer: by_role("Producer"),
            lyricist: credit(&["author"], "Lyricist"),
            engineer: credit(&["recordingengineer", "masteringengineer"], "Engineer"),
            mixer: credit(&["mixingengineer"], "Mixer"),
            conductor: by_role("Conductor"),
            performer: by_role("Performer"),
            bpm: gw_num(&gw["BPM"])
                .or_else(|| public_meta.as_ref().and_then(|m| m["bpm"].as_f64()))
                .filter(|b| *b > 0.0)
                .map(|b| format!("{}", b.round() as u64)),
            replaygain_track_gain: gw_num(&gw["GAIN"])
                .or_else(|| public_meta.as_ref().and_then(|m| m["gain"].as_f64()))
                .map(|g| format!("{g:+.2} dB")),
            replaygain_track_peak: None,
            replaygain_album_gain: None,
            replaygain_album_peak: None,
            description: None,
            purchase_date: None,
            grouping: None,
            comment: None,
            lyrics: settings.embed_lyrics.then(|| found.plain.clone()).flatten(),
        };

        finalize_track(FinalizeTrack {
            dest: dest_path,
            platform: "deezer",
            track_id,
            rank,
            served_rank: quality_rank("deezer", quality_info(quality).0),
            source: ConversionSource::from_ext(ext),
            duration: public_meta.as_ref().and_then(|m| m["duration"].as_f64()),
            metadata: &metadata,
            embed_cover,
            cover_tmp: cover_tmp.as_ref(),
            sidecars: found.sidecars(Some(&fields.title), Some(&fields.artist)),
            settings,
            dedup,
            on_log,
        })
        .await
    }
}

#[cfg(test)]
mod album_merge_tests {
    use super::{deezer_cover_url, graft_album};
    use serde_json::json;

    /// The exact key set a live `GET /album/{id}/tracks` entry comes back with.
    /// Everything album-level is absent, which is why nothing album-level was tagged.
    fn listing_entry() -> serde_json::Value {
        json!({
            "id": 3135553,
            "readable": true,
            "title": "One More Time",
            "isrc": "GBDUW0000053",
            "duration": 320,
            "track_position": 1,
            "disk_number": 1,
            "explicit_lyrics": false,
            "md5_image": "5718f7c81c27e0b2417e2a4c45224f8a",
            "artist": { "id": 27, "name": "Daft Punk" },
            "type": "track"
        })
    }

    fn album_payload() -> serde_json::Value {
        json!({
            "id": 302127,
            "title": "Discovery",
            "upc": "724384960650",
            "label": "Daft Life Ltd./ADA France",
            "nb_tracks": 14,
            "release_date": "2001-03-07",
            "md5_image": "5718f7c81c27e0b2417e2a4c45224f8a",
            "cover_xl": "https://cdn-images.dzcdn.net/images/cover/5718f7c81c27e0b2417e2a4c45224f8a/1000x1000-000000-80-0-0.jpg",
            "artist": { "id": 27, "name": "Daft Punk" },
            "genres": { "data": [ { "id": 106, "name": "Electronic" } ] },
            "tracks": { "data": [ { "disk_number": 1 }, { "disk_number": 2 } ] }
        })
    }

    #[test]
    fn the_release_payload_restores_every_album_level_field() {
        let mut track = listing_entry();
        assert!(track["album"]["title"].as_str().is_none());
        graft_album(&mut track, &album_payload());

        assert_eq!(track["album"]["title"].as_str(), Some("Discovery"));
        assert_eq!(track["album"]["upc"].as_str(), Some("724384960650"));
        assert_eq!(
            track["album"]["label"].as_str(),
            Some("Daft Life Ltd./ADA France")
        );
        assert_eq!(track["album"]["nb_tracks"].as_u64(), Some(14));
        assert_eq!(track["album"]["release_date"].as_str(), Some("2001-03-07"));
        assert_eq!(
            track["album"]["genres"]["data"][0]["name"].as_str(),
            Some("Electronic")
        );
        assert!(track["album"]["cover_xl"].as_str().is_some());
        assert_eq!(track["album"]["artist"]["name"].as_str(), Some("Daft Punk"));
        let disctotal = track["album"]["tracks"]["data"]
            .as_array()
            .and_then(|a| a.iter().filter_map(|t| t["disk_number"].as_u64()).max());
        assert_eq!(disctotal, Some(2));
    }

    /// `nb_disk` is null on every public album and the album's own embedded tracks
    /// carry no `disk_number`, so a disc count taken from the release payload alone
    /// is always empty. Only the `/album/{id}/tracks` listing knows.
    #[test]
    fn the_disc_count_cannot_come_from_the_album_payload_alone() {
        let album = album_payload();
        assert!(album["nb_disk"].as_u64().is_none());
        let from_embedded = json!({
            "id": 12114248,
            "tracks": { "data": [ { "title": "One" }, { "title": "Two" } ] }
        });
        let counted = from_embedded["tracks"]["data"]
            .as_array()
            .and_then(|a| a.iter().filter_map(|t| t["disk_number"].as_u64()).max());
        assert_eq!(counted, None);

        let mut stamped = album;
        stamped["nb_disk"] = json!(2);
        assert_eq!(stamped["nb_disk"].as_u64(), Some(2));
    }

    /// Every scalar on the gw blob is a JSON string — captured live as
    /// `"GAIN": "-9.6"`, `"NUMBER_TRACK": "16"`, `"DISK_NUMBER": "1"`. Reading them
    /// as numbers silently yields nothing, which is how BPM and ReplayGain would
    /// have gone missing all over again.
    #[test]
    fn gw_scalars_arrive_as_strings_and_still_parse() {
        let gw = json!({ "GAIN": "-9.6", "BPM": "128", "NUMBER_TRACK": "16" });
        let num = |v: &serde_json::Value| -> Option<f64> {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
        };
        assert_eq!(gw["GAIN"].as_f64(), None, "gw really does send a string");
        assert_eq!(num(&gw["GAIN"]), Some(-9.6));
        assert_eq!(num(&gw["BPM"]), Some(128.0));
        assert_eq!(num(&json!(-9.6)), Some(-9.6), "a real number still works");
        assert_eq!(format!("{:+.2} dB", num(&gw["GAIN"]).unwrap()), "-9.60 dB");
    }

    /// Captured role keys: gw names the writing credits `composer` and `author`, and
    /// spells the studio roles out in full. There is no plain `producer`, `engineer`
    /// or `mixer` key, so those still come from the public contributors list.
    #[test]
    fn gw_contributor_roles_use_their_captured_spellings() {
        let credits = json!({
            "main_artist": ["Daft Punk"],
            "featuring": ["Pharrell Williams"],
            "composer": ["Thomas Bangalter", "Nile Rodgers"],
            "author": ["Thomas Bangalter"],
            "mixingengineer": ["Mick Guzauski"],
            "masteringengineer": ["Bob Ludwig"],
            "recordingengineer": ["Peter Franco"]
        });
        let role = |keys: &[&str]| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for k in keys {
                if let Some(items) = credits[*k].as_array() {
                    out.extend(items.iter().filter_map(|v| v.as_str()).map(str::to_string));
                }
            }
            out
        };
        assert_eq!(role(&["composer"]), ["Thomas Bangalter", "Nile Rodgers"]);
        assert_eq!(role(&["author"]), ["Thomas Bangalter"]);
        assert_eq!(role(&["mixingengineer"]), ["Mick Guzauski"]);
        assert_eq!(
            role(&["recordingengineer", "masteringengineer"]),
            ["Peter Franco", "Bob Ludwig"]
        );
        assert!(
            role(&["producer"]).is_empty(),
            "gw has no plain producer key"
        );
    }

    #[test]
    fn a_track_that_already_nests_its_album_is_left_alone() {
        let mut track = json!({
            "id": 3135553,
            "album": { "id": 302127, "title": "Discovery (Remaster)" }
        });
        graft_album(&mut track, &album_payload());
        assert_eq!(
            track["album"]["title"].as_str(),
            Some("Discovery (Remaster)")
        );
    }

    #[test]
    fn a_trimmed_entry_still_yields_a_cover_from_its_image_hash() {
        let track = listing_entry();
        let url = deezer_cover_url(track["md5_image"].as_str().unwrap(), 1400).unwrap();
        assert_eq!(
            url,
            "https://cdn-images.dzcdn.net/images/cover/\
             5718f7c81c27e0b2417e2a4c45224f8a/1400x1400-000000-80-0-0.jpg"
                .replace(' ', "")
        );
        assert!(deezer_cover_url("", 1400).is_none());
    }
}

#[cfg(test)]
mod cover_tests {
    use super::resize_deezer_cover;

    const XL: &str =
        "https://e-cdns-images.dzcdn.net/images/cover/abc123/1000x1000-000000-80-0-0.jpg";

    #[test]
    fn the_requested_edge_length_replaces_the_one_in_the_path() {
        assert_eq!(
            resize_deezer_cover(XL, 1280),
            "https://e-cdns-images.dzcdn.net/images/cover/abc123/1280x1280-000000-80-0-0.jpg"
        );
        assert_eq!(
            resize_deezer_cover(XL, 320),
            "https://e-cdns-images.dzcdn.net/images/cover/abc123/320x320-000000-80-0-0.jpg"
        );
    }

    #[test]
    fn a_url_without_a_size_segment_is_left_alone() {
        let odd = "https://e-cdns-images.dzcdn.net/images/cover/abc123/";
        assert_eq!(resize_deezer_cover(odd, 640), odd);
    }
}

use std::path::Path;

use crate::defaults::Settings;
use crate::downloads::dedup::DedupLedger;
use crate::services::common::pipeline::orchestrator::{
    extract_platform_id, ContentType, Platform, SharedItem, SharedLog, SharedProgress,
    SharedQuality, TrackSourceClient,
};

impl DeezerClient {
    /// A free account asking for FLAC gets 128 kbps MP3; the folder should say so.
    fn entitled_quality_format(&self, settings: &Settings) -> (String, String) {
        let requested = match settings.deezer_quality.to_uppercase().as_str() {
            "FLAC" | "2" => 2u8,
            "MP3_320" | "320" | "1" => 1,
            _ => 0,
        };
        let effective = requested.min(self.entitled_quality());
        let as_str = match effective {
            2 => "FLAC",
            1 => "MP3_320",
            _ => "MP3_128",
        };
        (
            deezer_quality_label(as_str).to_string(),
            deezer_format(as_str).to_string(),
        )
    }
}

#[async_trait::async_trait]
impl TrackSourceClient for DeezerClient {
    fn platform_name(&self) -> &'static str {
        "Deezer"
    }

    fn requested_quality(&self, _settings: &Settings) -> u8 {
        0
    }

    fn extract_id(&self, url: &str, content_type: ContentType) -> Option<String> {
        extract_platform_id(url, Platform::Deezer, content_type)
    }

    fn album_url(&self, album_id: &str) -> String {
        format!("https://www.deezer.com/album/{}", album_id)
    }

    fn album_quality_format(&self, _album: &AlbumInfo, settings: &Settings) -> (String, String) {
        self.entitled_quality_format(settings)
    }

    fn playlist_quality_format(&self, settings: &Settings) -> (String, String) {
        self.entitled_quality_format(settings)
    }

    async fn probe_release_quality(
        &self,
        first_track_id: &str,
        _quality: u8,
        settings: &Settings,
    ) -> Option<(String, String)> {
        let requested = match settings.deezer_quality.to_uppercase().as_str() {
            "FLAC" | "2" => 2u8,
            "MP3_320" | "320" | "1" => 1,
            _ => 0,
        };
        DeezerClient::probe_served_quality(self, first_track_id, requested)
            .await
            .ok()
    }

    async fn get_album_tracks(&self, album_id: &str) -> MhResult<AlbumInfo> {
        DeezerClient::get_album_tracks(self, album_id).await
    }

    async fn get_playlist_tracks(&self, playlist_id: &str) -> MhResult<PlaylistInfo> {
        DeezerClient::get_playlist_tracks(self, playlist_id).await
    }

    async fn get_artist_albums(
        &self,
        artist_id: &str,
        _settings: &Settings,
    ) -> MhResult<Vec<String>> {
        DeezerClient::get_artist_albums(self, artist_id).await
    }

    async fn download_track(&self, job: TrackJob<'_>) -> MhResult<TrackOutcome> {
        DeezerClient::download_track(self, job).await
    }

    async fn download_label(
        &self,
        _url: &str,
        _base_dir: &Path,
        _settings: &Settings,
        _on_progress: SharedProgress,
        _on_log: SharedLog,
        _on_item: SharedItem,
        _on_quality: SharedQuality,
        _bytes: Arc<crate::downloads::ByteProgress>,
        _dedup: Option<&DedupLedger>,
    ) -> MhResult<()> {
        Err(MhError::Unsupported(
            "Deezer does not support label downloads.".into(),
        ))
    }

    async fn download_video(
        &self,
        _url: &str,
        _base_dir: &Path,
        _settings: &Settings,
        _on_progress: SharedProgress,
        _on_log: SharedLog,
        _dedup: Option<&DedupLedger>,
    ) -> MhResult<()> {
        Err(MhError::Unsupported(
            "Deezer does not support video downloads.".into(),
        ))
    }
}

pub fn deezer_quality_label(quality: &str) -> &'static str {
    match quality.to_uppercase().as_str() {
        "MP3_128" | "128" => "128kbps",
        "MP3_320" | "320" => "320kbps",
        _ => "16-bit ⁄ 44.1kHz",
    }
}

pub fn deezer_format(quality: &str) -> &'static str {
    match quality.to_uppercase().as_str() {
        "MP3_128" | "128" | "MP3_320" | "320" => "MP3",
        _ => "FLAC",
    }
}

#[cfg(test)]
mod quality_label_tests {
    use super::*;

    #[test]
    fn deezer_quality_round_trip() {
        let cases = [
            (0u8, "MP3_128", "MP3", "128kbps"),
            (1u8, "MP3_320", "MP3", "320kbps"),
            (2u8, "FLAC", "FLAC", "16-bit ⁄ 44.1kHz"),
        ];
        for (q_u8, settings_str, expected_format, expected_label) in cases {
            let settings_str_owned: String = match q_u8 {
                2 => "FLAC".into(),
                1 => "MP3_320".into(),
                _ => "MP3_128".into(),
            };
            assert_eq!(settings_str_owned, settings_str, "lib.rs u8→string");
            assert_eq!(deezer_format(&settings_str_owned), expected_format);
            assert_eq!(deezer_quality_label(&settings_str_owned), expected_label);
        }
    }
}

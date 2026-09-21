use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::defaults::Settings;
use crate::downloads::dedup::{quality_rank, DedupLedger};
use crate::errors::{MhError, MhResult};
use crate::http_client::build_tidal_client;
use crate::services::common::ids::now_secs_f64;
use crate::services::common::library::string_at;
use crate::services::common::lyrics::{lyrics_wanted, resolve_track_lyrics, FoundLyrics};
use crate::services::common::pipeline::converter::{AudioCodec, ConversionSource};
use crate::services::common::pipeline::downloader::{
    count_of, cover_tmp_for, download_file, excluded_tags, existing_final_file, finalize_track,
    preflight, resolve_track_dest, save_cover_file, track_vars, CommonTrackFields, CoverFiles,
    FinalizeTrack,
};
use crate::services::common::pipeline::orchestrator::TrackJob;
use crate::services::common::pipeline::tagger::{one, tag_file, TrackMetadata};
use crate::services::common::pipeline::{AlbumInfo, PlaylistInfo, TrackOutcome};
use crate::services::common::video::{parse_height, run_ffmpeg_with_progress};

const BASE: &str = "https://api.tidalhifi.com/v1";
const AUTH_URL: &str = "https://auth.tidal.com/v1/oauth2";

pub const TIDAL_CLIENT_ID: &str = "lw3vR6GE1vtNBsjv";
const TIDAL_CLIENT_SECRET: &str = "Y8tIpqKJxs9BEIwYr0I9bSbMWDsogXJx9LaN3mCHwD4=";

const QUALITY_MAP: &[(u8, &str)] = &[
    (0, "LOW"),
    (1, "HIGH"),
    (2, "LOSSLESS"),
    (3, "HI_RES_LOSSLESS"),
];

fn first_non_empty<'a>(candidates: &[Option<&'a str>]) -> Option<&'a str> {
    candidates
        .iter()
        .flatten()
        .copied()
        .find(|s| !s.trim().is_empty())
}

/// The tier byte a served `audioQuality` corresponds to, for dedup ranking.
fn tier_of(audio_quality: &str) -> u8 {
    QUALITY_MAP
        .iter()
        .find(|(_, v)| v.eq_ignore_ascii_case(audio_quality))
        .map(|(k, _)| *k)
        .unwrap_or(if audio_quality.eq_ignore_ascii_case("HI_RES") {
            3
        } else {
            2
        })
}

fn quality_str(q: u8) -> &'static str {
    QUALITY_MAP
        .iter()
        .find(|(k, _)| *k == q)
        .map(|(_, v)| *v)
        .unwrap_or("LOSSLESS")
}

/// Whether an `albums/{id}` payload advertises hi-res. Tidal reports the ceiling in
/// `audioQuality` and repeats it in `mediaMetadata.tags`; either is enough.
fn album_tier_is_hi_res(album: &Value) -> bool {
    let quality = album["audioQuality"].as_str().unwrap_or("");
    if quality.eq_ignore_ascii_case("HI_RES") || quality.eq_ignore_ascii_case("HI_RES_LOSSLESS") {
        return true;
    }
    album["mediaMetadata"]["tags"]
        .as_array()
        .map(|tags| {
            tags.iter()
                .filter_map(|t| t.as_str())
                .any(|t| t.eq_ignore_ascii_case("HIRES_LOSSLESS"))
        })
        .unwrap_or(false)
}

/// The `<Representation …>` tag a manifest actually serves. A manifest for one
/// requested tier normally holds a single one, but reading only the first match —
/// which is what a bare `captures()` does — silently picks the wrong track the
/// moment Tidal lists more than one.
fn best_representation(mpd: &str) -> Option<String> {
    let re = regex::Regex::new(r"<Representation\b[^>]*>").ok()?;
    re.find_iter(mpd)
        .map(|m| m.as_str().to_string())
        .max_by_key(|tag| {
            attr_of(tag, "bandwidth")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0)
        })
}

/// Read `name="…"` out of a DASH tag.
///
/// Hand-rolled rather than `Regex::new(&format!(…))`: this runs several times
/// per track download, and building the pattern from the attribute name meant
/// compiling a fresh DFA on every call. The leading-boundary check is what the
/// `\b` in the old pattern did — it stops `id=` matching inside `uid=`.
fn attr_of(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let mut from = 0;
    while let Some(rel) = tag[from..].find(&needle) {
        let at = from + rel;
        let value_start = at + needle.len();
        let preceded_by_word_char = tag[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !preceded_by_word_char {
            let end = tag[value_start..].find('"')? + value_start;
            return Some(tag[value_start..end].to_string());
        }
        from = value_start;
    }
    None
}

/// A Tidal representation names itself `FORMAT,RATE,DEPTH` — `FLAC_HIRES,44100,24`.
/// For hi-res that id is the only place the bit depth is published, so without it
/// the label falls back to a guess.
fn representation_id_spec(id: &str) -> (Option<u32>, Option<u32>) {
    let mut parts = id.split(',').skip(1);
    let rate = parts.next().and_then(|p| p.trim().parse::<u32>().ok());
    let depth = parts.next().and_then(|p| p.trim().parse::<u32>().ok());
    (rate, depth)
}

#[derive(Debug, Clone)]
pub struct TidalStreamInfo {
    pub url: Option<String>,
    pub manifest: Option<String>,
    pub manifest_mime: String,
    pub audio_quality: String,
    pub encryption_key: Option<String>,
}

/// What Tidal actually served. The requested tier is only a preference: a track
/// that exists solely in `LOSSLESS` answers a `HI_RES_LOSSLESS` request with
/// `LOSSLESS`, so naming a file after the request labels a 16-bit rip hi-res.
#[derive(Debug, Clone, Default)]
pub struct ServedSpec {
    pub quality: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<u32>,
    /// From the DASH `Representation@bandwidth`. `playbackinfo` never carries it,
    /// and it is the only honest source for what a lossy tier actually weighs.
    pub bitrate_bps: Option<u32>,
}

#[derive(Debug)]
enum Downloadable {
    Dash {
        init_url: String,
        segment_urls: Vec<String>,
        ext: String,
        needs_remux: bool,
        served: ServedSpec,
    },
    Direct {
        url: String,
        ext: String,
        enc_key: Option<String>,
        served: ServedSpec,
    },
}

impl Downloadable {
    fn ext(&self) -> &str {
        match self {
            Downloadable::Dash { ext, .. } => ext,
            Downloadable::Direct { ext, .. } => ext,
        }
    }

    fn served(&self) -> &ServedSpec {
        match self {
            Downloadable::Dash { served, .. } => served,
            Downloadable::Direct { served, .. } => served,
        }
    }
}

pub enum TidalPlayback {
    Direct {
        url: String,
        mime: &'static str,
        access_token: String,
    },
    Buffered {
        mime: &'static str,
        urls: Vec<String>,
        access_token: String,
        needs_remux: bool,
    },
}

pub struct TidalClient {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
    pub country_code: String,
    pub token_expiry: f64,
    pub client: reqwest::Client,
    /// Album payloads already fetched during this run.
    album_cache: Arc<tokio::sync::Mutex<HashMap<String, Value>>>,
}

impl TidalClient {
    /// Authenticate, and write the token back if Tidal rotated it.
    ///
    /// Tidal issues a fresh access token on refresh; dropping it means the next
    /// call re-authenticates from scratch, so the new token and its expiry are
    /// persisted before the client is handed back.
    pub async fn authenticate_and_persist(
        settings: &Settings,
        store: &tokio::sync::RwLock<Settings>,
        user_data: &Path,
    ) -> MhResult<Self> {
        let old_token = settings.tidal_access_token.clone();
        let client = Self::authenticate(settings).await?;
        if client.access_token != old_token {
            let mut s = store.write().await;
            s.tidal_access_token = client.access_token.clone();
            s.tidal_token_expiry = client.token_expiry.to_string();
            crate::settings::save_settings(&s, user_data).await.ok();
        }
        Ok(client)
    }

    pub async fn authenticate(settings: &Settings) -> MhResult<Self> {
        if settings.tidal_access_token.is_empty() {
            return Err(MhError::Auth(
                "Tidal access token not set. Go to Settings → Tidal and follow the instructions to get your token.".into()
            ));
        }

        let client = build_tidal_client()?;
        let access_token = settings.tidal_access_token.clone();
        let refresh_token = settings.tidal_refresh_token.clone();
        let token_expiry: f64 = settings.tidal_token_expiry.parse().unwrap_or(0.0);
        let user_id = settings.tidal_user_id.clone();
        let country_code = if settings.tidal_country_code.is_empty() {
            "US".to_string()
        } else {
            settings.tidal_country_code.clone()
        };

        let mut client_obj = TidalClient {
            album_cache: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            access_token,
            refresh_token,
            user_id,
            country_code,
            token_expiry,
            client,
        };

        let now = now_secs_f64();

        if client_obj.token_expiry - now < 86400.0 {
            if !client_obj.refresh_token.is_empty() {
                client_obj.refresh_access_token().await?;
            }
        } else {
            client_obj.verify_token().await?;
        }

        Ok(client_obj)
    }

    pub async fn refresh_access_token(&mut self) -> MhResult<()> {
        let auth = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", TIDAL_CLIENT_ID, TIDAL_CLIENT_SECRET))
        };

        let refresh_token_encoded: String =
            url::form_urlencoded::byte_serialize(self.refresh_token.as_bytes()).collect();
        let body = format!(
            "client_id={}&refresh_token={}&grant_type=refresh_token&scope=r_usr%2Bw_usr%2Bw_sub",
            TIDAL_CLIENT_ID, refresh_token_encoded
        );

        let resp = self
            .client
            .post(format!("{}/token", AUTH_URL))
            .header("Authorization", format!("Basic {}", auth))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(crate::services::common::http::auth_error("Tidal", resp).await);
        }

        let json: Value = resp.json().await?;
        self.access_token = json["access_token"]
            .as_str()
            .ok_or_else(|| MhError::Auth("Tidal: no access_token in refresh response".into()))?
            .to_string();

        let now = now_secs_f64();
        self.token_expiry = now + json["expires_in"].as_f64().unwrap_or(0.0);

        Ok(())
    }

    async fn verify_token(&mut self) -> MhResult<()> {
        let resp = self
            .client
            .get("https://api.tidal.com/v1/sessions")
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(crate::services::common::http::auth_error("Tidal", resp).await);
        }
        let json: Value = resp.json().await?;
        if let Some(uid) = json["userId"].as_u64() {
            self.user_id = uid.to_string();
        }
        if let Some(cc) = json["countryCode"].as_str() {
            self.country_code = cc.to_string();
        }
        Ok(())
    }

    async fn api_get(&self, path: &str, extra_params: &[(&str, &str)]) -> MhResult<Value> {
        let url = format!("{}/{}", BASE, path);
        let mut params: Vec<(&str, &str)> =
            vec![("countryCode", &self.country_code), ("limit", "100")];
        params.extend_from_slice(extra_params);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .query(&params)
            .send()
            .await?;
        crate::services::common::http::read_json("Tidal", resp).await
    }

    async fn api_get_raw(
        &self,
        path: &str,
        extra_params: &[(&str, &str)],
    ) -> MhResult<(u16, String)> {
        let url = format!("{}/{}", BASE, path);
        let mut params: Vec<(&str, &str)> =
            vec![("countryCode", &self.country_code), ("limit", "100")];
        params.extend_from_slice(extra_params);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .query(&params)
            .send()
            .await?;
        let status = resp.status().as_u16();
        let text = resp.text().await?;
        Ok((status, text))
    }

    async fn get_downloadable(&self, track_id: &str, quality: u8) -> MhResult<Downloadable> {
        if quality > 3 {
            return Err(MhError::Other(format!(
                "No streamable format for track {}",
                track_id
            )));
        }

        let q_str = quality_str(quality);
        let (status, text) = self
            .api_get_raw(
                &format!("tracks/{}/playbackinfopostpaywall", track_id),
                &[
                    ("audioquality", q_str),
                    ("playbackmode", "STREAM"),
                    ("assetpresentation", "FULL"),
                ],
            )
            .await?;

        if status == 401 || status == 403 {
            if quality > 0 {
                return Box::pin(self.get_downloadable(track_id, quality - 1)).await;
            }
            return Err(MhError::Other(format!(
                "Tidal: no accessible quality for track {}",
                track_id
            )));
        }
        if status == 404 {
            return Err(MhError::NotFound(format!(
                "Tidal: track {} not found",
                track_id
            )));
        }
        if status != 200 {
            return Err(MhError::Other(format!(
                "Tidal: HTTP {} for track {}",
                status, track_id
            )));
        }

        let json: Value = serde_json::from_str(&text)?;
        let manifest_mime = string_at(&json, &["/manifestMimeType"]);
        let served = ServedSpec {
            quality: json["audioQuality"].as_str().unwrap_or(q_str).to_string(),
            bit_depth: json["bitDepth"].as_u64().map(|n| n as u32),
            sample_rate: json["sampleRate"].as_u64().map(|n| n as u32),
            bitrate_bps: None,
        };

        use base64::Engine;
        let raw_manifest = json["manifest"]
            .as_str()
            .and_then(|m| base64::engine::general_purpose::STANDARD.decode(m).ok())
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();

        if manifest_mime == "application/dash+xml" {
            return self.parse_dash_manifest_to_downloadable(&raw_manifest, served);
        }

        let manifest: Value = match serde_json::from_str(&raw_manifest) {
            Ok(m) => m,
            Err(e) => {
                if quality > 0 {
                    return Box::pin(self.get_downloadable(track_id, quality - 1)).await;
                }
                return Err(MhError::Parse(format!(
                    "Tidal: failed to parse manifest for {}: {}. Manifest was: {}",
                    track_id, e, raw_manifest
                )));
            }
        };

        let enc_key = if manifest["encryptionType"].as_str() == Some("NONE") {
            None
        } else {
            manifest["keyId"].as_str().map(|s| s.to_string())
        };

        let codec = manifest["codecs"].as_str().unwrap_or("").to_lowercase();
        let ext = if codec == "flac" { "flac" } else { "m4a" };

        let url = match manifest["urls"][0].as_str() {
            Some(u) => u,
            None => {
                if quality > 0 {
                    return Box::pin(self.get_downloadable(track_id, quality - 1)).await;
                }
                return Err(MhError::Other(format!(
                    "Tidal: no URL in manifest for {}. Manifest was: {}",
                    track_id, raw_manifest
                )));
            }
        };

        Ok(Downloadable::Direct {
            url: url.to_string(),
            ext: ext.to_string(),
            enc_key,
            served,
        })
    }

    fn parse_dash_manifest_to_downloadable(
        &self,
        mpd: &str,
        mut served: ServedSpec,
    ) -> MhResult<Downloadable> {
        let segments = self.parse_dash_manifest_inner(mpd)?;
        let (init_url, segment_urls) = if segments.is_empty() {
            (String::new(), vec![])
        } else {
            (segments[0].clone(), segments[1..].to_vec())
        };

        let rep = best_representation(mpd);
        let codec = rep
            .as_ref()
            .and_then(|r| attr_of(r, "codecs"))
            .map(|c| c.to_lowercase())
            .unwrap_or_else(|| "flac".to_string());

        if let Some(r) = rep.as_deref() {
            let (id_rate, id_depth) = attr_of(r, "id")
                .as_deref()
                .map(representation_id_spec)
                .unwrap_or((None, None));
            if served.sample_rate.is_none() {
                served.sample_rate = attr_of(r, "audioSamplingRate")
                    .and_then(|v| v.parse::<u32>().ok())
                    .or(id_rate);
            }
            if served.bit_depth.is_none() {
                served.bit_depth = id_depth;
            }
            served.bitrate_bps = attr_of(r, "bandwidth").and_then(|v| v.parse::<u32>().ok());
        }

        let ext = if codec == "flac" { "flac" } else { "m4a" };
        let needs_remux = codec == "flac";

        Ok(Downloadable::Dash {
            init_url,
            segment_urls,
            ext: ext.to_string(),
            needs_remux,
            served,
        })
    }

    fn parse_dash_manifest_inner(&self, mpd: &str) -> MhResult<Vec<String>> {
        let get_attr = |tag: &str, attr: &str| -> Option<String> {
            let re = regex::Regex::new(&format!(
                r#"<{tag}[^>]+{attr}="([^"]+)""#,
                tag = tag,
                attr = attr
            ))
            .ok()?;
            re.captures(mpd)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
        };

        let init_url = get_attr("SegmentTemplate", "initialization").unwrap_or_default();
        let media_template = get_attr("SegmentTemplate", "media").unwrap_or_default();
        let start_number: u64 = get_attr("SegmentTemplate", "startNumber")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        let s_re = regex::Regex::new(r"<S\s[^>]*>").unwrap();
        let r_re = regex::Regex::new(r#"r="(\d+)""#).ok();
        let mut total_segments: u64 = 0;
        for m in s_re.find_iter(mpd) {
            let tag = m.as_str();
            let r_val: u64 = r_re
                .as_ref()
                .and_then(|re| re.captures(tag))
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            total_segments += r_val + 1;
        }

        let mut urls = vec![init_url];
        for i in 0..total_segments {
            let seg_url = media_template.replace("$Number$", &(start_number + i).to_string());
            urls.push(seg_url);
        }

        Ok(urls)
    }

    async fn download_dash(
        &self,
        init_url: &str,
        segment_urls: &[String],
        needs_remux: bool,
        dest_path: &Path,
        on_progress: impl Fn(u64, u64),
    ) -> MhResult<()> {
        let mut all_urls = vec![init_url.to_string()];
        all_urls.extend_from_slice(segment_urls);

        let tmp = dest_path.with_extension("tmp.m4a");

        let headers = crate::http_client::build_headers(&[(
            "Authorization",
            &format!("Bearer {}", self.access_token),
        )])?;

        let total = all_urls.len() as u64;
        {
            use tokio::io::AsyncWriteExt;
            let mut out = tokio::fs::File::create(&tmp).await?;
            for (i, url) in all_urls.iter().enumerate() {
                let resp = self.client.get(url).headers(headers.clone()).send().await?;
                if !resp.status().is_success() {
                    return Err(MhError::Other(format!(
                        "Tidal segment HTTP {}",
                        resp.status().as_u16()
                    )));
                }
                let bytes = resp.bytes().await?;
                out.write_all(&bytes).await?;
                on_progress((i + 1) as u64, total);
            }
        }

        if needs_remux {
            let ffmpeg_bin = crate::venv_manager::resolve_ffmpeg();
            let mut ffmpeg_cmd = tokio::process::Command::new(&ffmpeg_bin);
            ffmpeg_cmd
                .args(["-y", "-loglevel", "error", "-i"])
                .arg(&tmp)
                .args(["-vn", "-c:a", "copy", "-map_metadata", "-1"])
                .arg(dest_path);
            crate::subprocess::apply_no_window(&mut ffmpeg_cmd);
            let output = ffmpeg_cmd
                .output()
                .await
                .map_err(|e| MhError::Subprocess(e.to_string()))?;
            if !output.status.success() {
                let mut retry = tokio::process::Command::new(&ffmpeg_bin);
                retry
                    .args(["-y", "-loglevel", "error", "-i"])
                    .arg(&tmp)
                    .args(["-vn", "-c:a", "flac", "-map_metadata", "-1"])
                    .arg(dest_path);
                crate::subprocess::apply_no_window(&mut retry);
                let second = retry
                    .output()
                    .await
                    .map_err(|e| MhError::Subprocess(e.to_string()))?;
                if !second.status.success() {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return Err(MhError::Subprocess(format!(
                        "ffmpeg remux failed: {}",
                        String::from_utf8_lossy(&second.stderr)
                    )));
                }
            }
            let _ = tokio::fs::remove_file(&tmp).await;
        } else {
            tokio::fs::rename(&tmp, dest_path).await?;
        }

        Ok(())
    }

    /// Tidal publishes a fixed ladder of cover sizes; anything else 404s.
    const COVER_SIZES: &'static [u32] = &[80, 160, 320, 640, 750, 1080, 1280];

    fn get_album_art_url(album_meta: &Value, settings: &Settings) -> Option<String> {
        let size = crate::services::common::pipeline::nearest_cover_size(
            settings.pipeline_cover_size,
            Self::COVER_SIZES,
        );
        album_meta["cover"].as_str().map(|uuid| {
            let path = uuid.replace('-', "/");
            format!(
                "https://resources.tidal.com/images/{}/{}x{}.jpg",
                path, size, size
            )
        })
    }

    pub async fn resolve_playback(&self, track_id: &str) -> MhResult<TidalPlayback> {
        let downloadable = self.get_downloadable(track_id, 3).await?;
        let mime: &'static str = if downloadable.ext() == "flac" {
            "audio/flac"
        } else {
            "audio/mp4"
        };
        match downloadable {
            Downloadable::Direct {
                url, enc_key: None, ..
            } => Ok(TidalPlayback::Direct {
                url,
                mime,
                access_token: self.access_token.clone(),
            }),
            Downloadable::Direct { .. } => Err(MhError::Other(
                "Encrypted Tidal track not supported for streaming".into(),
            )),
            Downloadable::Dash {
                init_url,
                segment_urls,
                needs_remux,
                ..
            } => {
                let urls: Vec<String> = std::iter::once(init_url).chain(segment_urls).collect();
                Ok(TidalPlayback::Buffered {
                    mime,
                    urls,
                    access_token: self.access_token.clone(),
                    needs_remux,
                })
            }
        }
    }

    pub async fn download_segments_to_file(
        client: &reqwest::Client,
        access_token: &str,
        urls: &[String],
        path: &std::path::Path,
        total: &std::sync::atomic::AtomicU64,
        _needs_remux: bool,
    ) -> MhResult<()> {
        use futures_util::StreamExt;
        use std::sync::atomic::Ordering;
        use tokio::io::AsyncWriteExt;

        let sizes = futures_util::future::join_all(urls.iter().map(|url| {
            let client = client.clone();
            let token = access_token.to_string();
            let url = url.clone();
            async move {
                client
                    .head(&url)
                    .header("Authorization", format!("Bearer {token}"))
                    .send()
                    .await
                    .ok()
                    .and_then(|r| r.content_length())
            }
        }))
        .await;
        if sizes.iter().all(Option::is_some) {
            total.store(sizes.into_iter().flatten().sum(), Ordering::Relaxed);
        }

        let mut file = tokio::fs::File::create(path).await.map_err(MhError::Io)?;
        for url in urls {
            let resp = client
                .get(url)
                .header("Authorization", format!("Bearer {access_token}"))
                .send()
                .await
                .map_err(MhError::Network)?;
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(MhError::Network)?;
                file.write_all(&chunk).await.map_err(MhError::Io)?;
            }
            file.flush().await.map_err(MhError::Io)?;
        }

        if let Ok(written) = file.metadata().await.map(|m| m.len()) {
            total.store(written, Ordering::Relaxed);
        }
        Ok(())
    }

    /// `albums/{id}`, memoised for the lifetime of this client — which is one
    /// download run, so a cached album can never go stale mid-download.
    async fn album_meta_cached(&self, album_id: &str) -> Option<Value> {
        if let Some(hit) = self.album_cache.lock().await.get(album_id) {
            return Some(hit.clone());
        }
        let fetched = self
            .api_get(&format!("albums/{}", album_id), &[])
            .await
            .ok()?;
        self.album_cache
            .lock()
            .await
            .insert(album_id.to_string(), fetched.clone());
        Some(fetched)
    }

    /// Walks a `v1` collection endpoint to the end. Without an explicit `limit` Tidal
    /// returns one default-sized page.
    async fn paged_items(&self, path: &str) -> MhResult<Vec<Value>> {
        let mut items: Vec<Value> = Vec::new();
        let mut offset = 0usize;
        loop {
            let page = self
                .api_get(path, &[("limit", "100"), ("offset", &offset.to_string())])
                .await?;
            let batch = page["items"].as_array().cloned().unwrap_or_default();
            let got = batch.len();
            items.extend(batch);
            offset += got;
            let total = page["totalNumberOfItems"].as_u64().unwrap_or(0) as usize;
            if got == 0 || offset >= total {
                break;
            }
        }
        Ok(items)
    }

    pub async fn get_album_tracks(&self, album_id: &str) -> MhResult<AlbumInfo> {
        let album_path = format!("albums/{}", album_id);
        let tracks_path = format!("albums/{}/tracks", album_id);
        let (tracks_resp, album_resp) = tokio::join!(
            self.paged_items(&tracks_path),
            self.api_get(&album_path, &[]),
        );
        let tracks = tracks_resp?;
        let album = album_resp.map_err(|e| {
            MhError::Other(format!(
                "Tidal album {} metadata lookup failed (folder name and album tags depend on it): {}",
                album_id, e
            ))
        })?;

        let rows = crate::services::common::pipeline::release_rows(
            &tracks,
            "/artist/name",
            Some("volumeNumber"),
        );

        Ok(AlbumInfo {
            tracks: rows,
            album_record: album.clone(),
            number_of_volumes: album["numberOfVolumes"].as_u64().unwrap_or(1) as u32,
            title: album["title"]
                .as_str()
                .unwrap_or(&format!("Album {}", album_id))
                .to_string(),
            artist: string_at(&album, &["/artist/name"]),
            year: first_non_empty(&[
                album["releaseDate"].as_str(),
                album["streamStartDate"].as_str(),
            ])
            .and_then(|d| d.split('-').next())
            .unwrap_or("")
            .trim()
            .to_string(),
            genre: string_at(&album, &["/genre"]),
            label: string_at(&album, &["/label/name"]),
            bit_depth: Some(if album_tier_is_hi_res(&album) { 24 } else { 16 }),
            sampling_rate: None,
        })
    }

    pub async fn get_playlist_tracks(&self, playlist_id: &str) -> MhResult<PlaylistInfo> {
        let meta = self
            .api_get(&format!("playlists/{}", playlist_id), &[])
            .await
            .unwrap_or(Value::Null);

        let items = self
            .paged_items(&format!("playlists/{}/tracks", playlist_id))
            .await?;
        let rows = crate::services::common::pipeline::release_rows(&items, "/artist/name", None);

        Ok(PlaylistInfo {
            tracks: rows,
            title: meta["title"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(&format!("Playlist {}", playlist_id))
                .to_string(),
            artist: string_at(&meta, &["/creator/name"]),
        })
    }

    pub async fn get_artist_albums(&self, artist_id: &str) -> MhResult<Vec<String>> {
        let path = format!("artists/{}/albums", artist_id);
        let (albums_resp, eps_resp) = tokio::join!(
            self.api_get(&path, &[("limit", "500")]),
            self.api_get(&path, &[("filter", "EPSANDSINGLES"), ("limit", "500")]),
        );

        let mut ids = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for resp in [albums_resp, eps_resp].into_iter().flatten() {
            for item in resp["items"].as_array().unwrap_or(&vec![]) {
                if let Some(id) = item["id"].as_u64() {
                    let id = id.to_string();
                    if seen.insert(id.clone()) {
                        ids.push(id);
                    }
                }
            }
        }
        Ok(ids)
    }

    pub async fn fetch_lyrics(&self, track_id: &str) -> Option<Value> {
        let qs = format!("countryCode={}", self.country_code);
        let hosts = [
            format!("https://api.tidal.com/v1/tracks/{}/lyrics?{}", track_id, qs),
            format!(
                "https://api.tidalhifi.com/v1/tracks/{}/lyrics?{}",
                track_id, qs
            ),
        ];
        for url in &hosts {
            if let Ok(resp) = self
                .client
                .get(url)
                .header("Authorization", format!("Bearer {}", self.access_token))
                .header("X-Tidal-Token", TIDAL_CLIENT_ID)
                .send()
                .await
            {
                if resp.status().is_success() {
                    if let Ok(json) = resp.json::<Value>().await {
                        return Some(json);
                    }
                }
            }
        }
        None
    }

    /// What Tidal will actually serve for this release. Asked once, before the folder
    /// is named — the requested tier is only a preference, and a release that exists
    /// solely in 16-bit answers a hi-res request with `LOSSLESS`, which used to leave
    /// a `24-bit` folder holding 16-bit files.
    pub async fn probe_served_quality(
        &self,
        track_id: &str,
        quality: u8,
    ) -> MhResult<(String, String)> {
        let served = self
            .get_downloadable(track_id, quality)
            .await?
            .served()
            .clone();
        let (label, format) = tidal_served_label(
            &served.quality,
            served.bit_depth,
            served.sample_rate,
            served.bitrate_bps,
        );
        Ok((label, format.to_string()))
    }

    pub async fn download_track(&self, job: TrackJob<'_>) -> MhResult<TrackOutcome> {
        let rank = quality_rank("tidal", &job.quality.to_string());
        if let Some(out) = preflight(
            &job,
            "tidal",
            "Tidal",
            &["allowStreaming", "streamReady"],
            rank,
        )? {
            return Ok(out);
        }
        let TrackJob {
            track_id,
            quality,
            dest,
            settings,
            on_progress,
            on_log,
            placement,
            dedup,
            record,
            release,
            covers,
        } = job;
        let on_log = on_log.as_ref();

        let downloadable = self.get_downloadable(track_id, quality).await?;

        let requested_tier = quality_str(quality);
        if !downloadable
            .served()
            .quality
            .eq_ignore_ascii_case(requested_tier)
        {
            let (served_label, _) = tidal_served_label(
                &downloadable.served().quality,
                downloadable.served().bit_depth,
                downloadable.served().sample_rate,
                downloadable.served().bitrate_bps,
            );
            on_log(format!(
                "  ⚠ Tidal {} unavailable for this track; served {} instead",
                requested_tier, served_label
            ));
        }

        let fetched = match record {
            Some(r) if !r.is_null() => None,
            _ => self
                .api_get(&format!("tracks/{}", track_id), &[])
                .await
                .ok(),
        };
        let meta: &Value = record
            .filter(|r| !r.is_null())
            .or(fetched.as_ref())
            .unwrap_or(&Value::Null);
        let album_id = meta["album"]["id"].as_u64().map(|id: u64| id.to_string());
        let album_meta: Option<Value> = match release.filter(|a| !a.is_null()) {
            Some(a) => Some(a.clone()),
            None => match album_id {
                Some(ref aid) => self.album_meta_cached(aid).await,
                None => None,
            },
        };
        let contributors: Option<Value> = self
            .api_get(&format!("tracks/{}/contributors", track_id), &[])
            .await
            .ok();

        let title = meta["title"]
            .as_str()
            .unwrap_or(&format!("track_{}", track_id))
            .to_string();
        let artists: Vec<String> = meta["artists"]
            .as_array()
            .map(|arr: &Vec<Value>| {
                arr.iter()
                    .filter_map(|a| a["name"].as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let artist = if artists.is_empty() {
            "Unknown".to_string()
        } else {
            artists.join(", ")
        };
        let albumartist = album_meta
            .as_ref()
            .and_then(|m| m["artist"]["name"].as_str())
            .unwrap_or(&artist)
            .to_string();
        let album = string_at(meta, &["/album/title"]);
        let track_num = meta["trackNumber"].as_u64().unwrap_or(0) as u32;
        let disc_num = meta["volumeNumber"].as_u64().unwrap_or(1) as u32;
        let tracktotal = album_meta
            .as_ref()
            .and_then(|m| m["numberOfTracks"].as_u64())
            .map(|n| n.to_string())
            .unwrap_or_default();
        let disctotal = album_meta
            .as_ref()
            .and_then(|m| m["numberOfVolumes"].as_u64())
            .map(|n| n.to_string())
            .unwrap_or_default();
        let release_date = first_non_empty(&[
            album_meta.as_ref().and_then(|m| m["releaseDate"].as_str()),
            album_meta
                .as_ref()
                .and_then(|m| m["streamStartDate"].as_str()),
            meta["album"]["releaseDate"].as_str(),
            meta["streamStartDate"].as_str(),
        ])
        .unwrap_or("")
        .to_string();
        let year = release_date
            .split('-')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        let genre = album_meta
            .as_ref()
            .and_then(|m| m["genre"].as_str())
            .unwrap_or("")
            .to_string();

        let explicit_str = if meta["explicit"].as_bool().unwrap_or(false) {
            " (Explicit)".to_string()
        } else {
            String::new()
        };

        let isrc = string_at(meta, &["/isrc"]);
        let label = album_meta
            .as_ref()
            .and_then(|m| m["label"]["name"].as_str())
            .unwrap_or("")
            .to_string();
        let date = release_date.clone();

        let ext = downloadable.ext().to_string();
        let served = downloadable.served().clone();
        let (quality_label, format_label) = tidal_served_label(
            &served.quality,
            served.bit_depth,
            served.sample_rate,
            served.bitrate_bps,
        );

        let composer_name = contributors
            .as_ref()
            .and_then(|c| c["items"].as_array())
            .map(|items| {
                items
                    .iter()
                    .filter(|c| {
                        c["role"]
                            .as_str()
                            .map(|r| r.eq_ignore_ascii_case("Composer"))
                            .unwrap_or(false)
                    })
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
            quality_label: quality_label.clone(),
            format_label: format_label.to_string(),
            composer: composer_name.clone(),
        };
        let file_stem = crate::downloads::native_common::native_track_name(settings, &fields);
        let (track_dest, dest_path) =
            resolve_track_dest(dest, &file_stem, &ext, settings, &fields, placement).await?;

        if let Some(existing) = existing_final_file(&dest_path, settings).await {
            on_log(format!(
                "  · already on disk, keeping {}",
                existing.display()
            ));
            return Ok(TrackOutcome::Skipped(existing));
        }

        let found = if lyrics_wanted(settings) {
            let native = match self.fetch_lyrics(track_id).await {
                Some(lyr) => FoundLyrics {
                    synced: lyr["subtitles"].as_str().map(|s| s.to_string()),
                    plain: lyr["lyrics"].as_str().map(|s| s.to_string()),
                    word: None,
                }
                .filled(),
                None => FoundLyrics::default(),
            };
            resolve_track_lyrics(
                native,
                &fields.title,
                &fields.artist,
                meta["duration"].as_f64(),
                settings,
                on_log,
            )
            .await
        } else {
            FoundLyrics::default()
        };

        let cover_url = album_meta
            .as_ref()
            .and_then(|m| Self::get_album_art_url(m, settings));
        let embed_cover = settings.embed_cover;
        let cover_files = CoverFiles::from_settings(settings);
        let cover_tmp = cover_tmp_for(&self.client, cover_url.as_deref(), settings, covers).await;

        match &downloadable {
            Downloadable::Dash {
                init_url,
                segment_urls,
                needs_remux,
                ..
            } => {
                self.download_dash(
                    init_url,
                    segment_urls,
                    *needs_remux,
                    &dest_path,
                    on_progress,
                )
                .await?;
            }
            Downloadable::Direct { url, enc_key, .. } => {
                let tmp_path = dest_path.with_extension(format!("{}.tmp", ext));
                let headers = crate::http_client::build_headers(&[(
                    "Authorization",
                    &format!("Bearer {}", self.access_token),
                )])?;
                download_file(&self.client, url, &tmp_path, Some(&headers), on_progress).await?;
                if let Some(ref key) = enc_key {
                    let encrypted = tokio::fs::read(&tmp_path).await?;
                    let decrypted = crate::services::tidal::crypto::decrypt_track(&encrypted, key)?;
                    tokio::fs::write(&dest_path, &decrypted).await?;
                    let _ = tokio::fs::remove_file(&tmp_path).await;
                } else {
                    tokio::fs::rename(&tmp_path, &dest_path).await?;
                }
            }
        }

        save_cover_file(
            &track_dest,
            cover_tmp.as_ref(),
            cover_files,
            Some(file_stem.as_str()),
        )
        .await;

        let contribs = contributors
            .as_ref()
            .and_then(|c| c["items"].as_array())
            .cloned()
            .unwrap_or_default();
        let gain = |scope: &Value, key: &str| -> Option<String> {
            scope[key].as_f64().map(|v| format!("{v:+.2} dB"))
        };
        let peak = |scope: &Value, key: &str| -> Option<String> {
            scope[key].as_f64().map(|v| format!("{v:.6}"))
        };
        let by_role = |role: &str| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for name in contribs
                .iter()
                .filter(|c| {
                    c["role"]
                        .as_str()
                        .map(|r| r.eq_ignore_ascii_case(role))
                        .unwrap_or(false)
                })
                .filter_map(|c| c["name"].as_str())
            {
                if !out.iter().any(|seen| seen.eq_ignore_ascii_case(name)) {
                    out.push(name.to_string());
                }
            }
            out
        };

        let metadata = TrackMetadata {
            title: Some(title),
            artist: if artists.is_empty() {
                one(Some(artist.clone()))
            } else {
                artists.clone()
            },
            album: Some(album),
            album_artist: one(Some(albumartist)),
            year: if release_date.is_empty() {
                None
            } else {
                Some(release_date.clone())
            },
            original_date: None,
            genre: one(Some(genre)),
            track_number: Some(track_num),
            disc_number: Some(disc_num),
            total_tracks: count_of(&tracktotal),
            total_discs: count_of(&disctotal),
            isrc: meta["isrc"].as_str().map(|s: &str| s.to_string()),
            upc: album_meta
                .as_ref()
                .and_then(|m| m["upc"].as_str())
                .map(|s| s.to_string()),
            copyright: meta["copyright"]
                .as_str()
                .or_else(|| album_meta.as_ref().and_then(|m| m["copyright"].as_str()))
                .map(|s| s.to_string()),
            label: album_meta
                .as_ref()
                .and_then(|m| m["label"]["name"].as_str())
                .map(|s| s.to_string()),
            composer: by_role("Composer"),
            conductor: by_role("Conductor"),
            performer: by_role("Performer"),
            producer: by_role("Producer"),
            lyricist: by_role("Lyricist"),
            engineer: by_role("Engineer"),
            mixer: by_role("Mixer"),
            bpm: meta["bpm"]
                .as_f64()
                .filter(|b| *b > 0.0)
                .map(|b| format!("{}", b.round() as u64)),
            replaygain_track_gain: gain(meta, "replayGain"),
            replaygain_track_peak: peak(meta, "peak"),
            replaygain_album_gain: album_meta.as_ref().and_then(|m| gain(m, "replayGain")),
            replaygain_album_peak: album_meta.as_ref().and_then(|m| peak(m, "peak")),
            description: None,
            purchase_date: None,
            grouping: None,
            comment: None,
            lyrics: settings.embed_lyrics.then(|| found.plain.clone()).flatten(),
        };

        finalize_track(FinalizeTrack {
            dest: dest_path,
            platform: "tidal",
            track_id,
            rank,
            served_rank: quality_rank("tidal", &tier_of(&served.quality).to_string()),
            source: if ext == "flac" {
                ConversionSource::with_codec(AudioCodec::Flac)
            } else {
                ConversionSource::with_codec(AudioCodec::Aac)
            },
            duration: meta["duration"].as_f64(),
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

    /// Picks the highest-bandwidth variant that still fits under `max_height`.
    /// Variants without a RESOLUTION attribute are only used when nothing fits.
    fn parse_best_m3u8_stream(m3u8: &str, max_height: u32) -> Option<String> {
        let lines: Vec<&str> = m3u8.lines().collect();
        let bw_re = regex::Regex::new(r"BANDWIDTH=(\d+)").ok()?;
        let res_re = regex::Regex::new(r"RESOLUTION=(\d+)x(\d+)").ok()?;

        let mut fitting: Option<(i64, String)> = None;
        let mut smallest: Option<(u32, i64, String)> = None;
        let mut unknown: Option<(i64, String)> = None;

        for (i, line) in lines.iter().enumerate() {
            let line = line.trim();
            if !line.starts_with("#EXT-X-STREAM-INF:") {
                continue;
            }
            let Some(url) = lines
                .get(i + 1)
                .map(|u| u.trim())
                .filter(|u| !u.is_empty() && !u.starts_with('#'))
            else {
                continue;
            };
            let bw = bw_re
                .captures(line)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<i64>().ok())
                .unwrap_or(0);
            match res_re
                .captures(line)
                .and_then(|c| c.get(2))
                .and_then(|m| m.as_str().parse::<u32>().ok())
            {
                Some(height) => {
                    if height <= max_height
                        && fitting.as_ref().map(|(b, _)| bw > *b).unwrap_or(true)
                    {
                        fitting = Some((bw, url.to_string()));
                    }
                    if smallest
                        .as_ref()
                        .map(|(h, _, _)| height < *h)
                        .unwrap_or(true)
                    {
                        smallest = Some((height, bw, url.to_string()));
                    }
                }
                None => {
                    if unknown.as_ref().map(|(b, _)| bw > *b).unwrap_or(true) {
                        unknown = Some((bw, url.to_string()));
                    }
                }
            }
        }

        fitting
            .map(|(_, u)| u)
            .or_else(|| smallest.map(|(_, _, u)| u))
            .or_else(|| unknown.map(|(_, u)| u))
    }

    /// Maps a requested output height onto Tidal's coarse `videoquality` tiers.
    fn video_quality_tier(max_height: u32) -> &'static str {
        match max_height {
            0..=480 => "LOW",
            481..=720 => "MEDIUM",
            _ => "HIGH",
        }
    }

    pub async fn download_video(
        &self,
        video_id: &str,
        dest: &Path,
        settings: &Settings,
        on_progress: impl Fn(u64, u64),
        dedup: Option<&DedupLedger>,
    ) -> MhResult<PathBuf> {
        let max_height = parse_height(&settings.tidal_video_quality).unwrap_or(1080);
        let rank = quality_rank("tidal-video", &settings.tidal_video_quality);
        if let Some(d) = dedup {
            if let Some(existing) = d.existing_path("tidal-video", video_id, rank) {
                return Ok(PathBuf::from(existing));
            }
        }

        let meta_resp = self.api_get(&format!("videos/{}", video_id), &[]).await;
        if let Err(e) = &meta_resp {
            if e.to_string().contains("404") {
                return Err(MhError::NotFound(format!(
                    "Tidal: video {} not found",
                    video_id
                )));
            }
        }
        let meta = meta_resp?;

        let title = meta["title"]
            .as_str()
            .unwrap_or(&format!("video_{}", video_id))
            .to_string();
        let artist = meta["artists"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a["name"].as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Unknown".to_string());
        let album = string_at(&meta, &["/album/title"]);
        let release_date = first_non_empty(&[
            meta["releaseDate"].as_str(),
            meta["streamStartDate"].as_str(),
        ])
        .unwrap_or("")
        .to_string();
        let year = release_date
            .split('-')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();

        let stream_resp = self
            .api_get_raw(
                &format!("videos/{}/playbackinfopostpaywall", video_id),
                &[
                    ("videoquality", Self::video_quality_tier(max_height)),
                    ("playbackmode", "STREAM"),
                    ("assetpresentation", "FULL"),
                ],
            )
            .await?;

        if stream_resp.0 != 200 {
            return Err(MhError::Other(format!(
                "Tidal: could not get video stream for {} (HTTP {}): {}",
                video_id, stream_resp.0, stream_resp.1
            )));
        }

        let stream_json: Value = serde_json::from_str(&stream_resp.1)?;
        use base64::Engine;
        let raw_manifest = stream_json["manifest"]
            .as_str()
            .and_then(|m| base64::engine::general_purpose::STANDARD.decode(m).ok())
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();

        let video_url = if raw_manifest.starts_with("#EXTM3U") {
            Self::parse_best_m3u8_stream(&raw_manifest, max_height).ok_or_else(|| {
                MhError::Other(format!(
                    "Tidal: no streams in HLS manifest for {}",
                    video_id
                ))
            })?
        } else {
            let mf: Value = serde_json::from_str(&raw_manifest).unwrap_or(Value::Null);
            mf["urls"][0]
                .as_str()
                .ok_or_else(|| {
                    MhError::Other(format!("Tidal: no video URL available for {}", video_id))
                })?
                .to_string()
        };

        let fields = CommonTrackFields {
            title: title.clone(),
            artist: artist.clone(),
            albumartist: artist.clone(),
            album: if album.is_empty() {
                title.clone()
            } else {
                album.clone()
            },
            track_num: meta["trackNumber"].as_u64().unwrap_or(0) as u32,
            disc_num: 1,
            tracktotal: String::new(),
            disctotal: String::new(),
            year: year.clone(),
            genre: String::new(),
            explicit: if meta["explicit"].as_bool().unwrap_or(false) {
                " (Explicit)".to_string()
            } else {
                String::new()
            },
            isrc: string_at(&meta, &["/isrc"]),
            label: String::new(),
            date: release_date.clone(),
            quality_label: format!("{}p", max_height),
            format_label: "MP4".to_string(),
            composer: String::new(),
        };
        let track_template = if settings.filepaths_track_format.trim().is_empty() {
            "{artist} - {title}".to_string()
        } else {
            settings.filepaths_track_format.clone()
        };
        let file_stem = crate::services::common::pipeline::build_file_name(
            &track_template,
            &track_vars(&fields),
            settings.filepaths_restrict_characters,
            settings.filepaths_truncate_to as usize,
        );
        let file_stem = if file_stem.is_empty() {
            crate::services::common::pipeline::safe_name(&title)
        } else {
            file_stem
        };
        tokio::fs::create_dir_all(dest).await?;
        let dest_path = dest.join(format!("{file_stem}.mp4"));

        let total_secs = meta["duration"].as_u64().unwrap_or(0);
        run_ffmpeg_with_progress(&video_url, &dest_path, total_secs, &on_progress).await?;

        const VIDEO_WIDE_SIZES: &[u32] = &[160, 320, 640, 750, 1280];
        const VIDEO_SQUARE_SIZES: &[u32] = &[80, 160, 320, 640, 750, 1080];
        let requested = settings.pipeline_cover_size;
        let cover_url = meta["imageId"]
            .as_str()
            .map(|uuid| {
                let w = crate::services::common::pipeline::nearest_cover_size(
                    requested,
                    VIDEO_WIDE_SIZES,
                );
                format!(
                    "https://resources.tidal.com/images/{}/{}x{}.jpg",
                    uuid.replace('-', "/"),
                    w,
                    w * 9 / 16
                )
            })
            .or_else(|| {
                meta["squareImage"].as_str().map(|uuid| {
                    let e = crate::services::common::pipeline::nearest_cover_size(
                        requested,
                        VIDEO_SQUARE_SIZES,
                    );
                    format!(
                        "https://resources.tidal.com/images/{}/{}x{}.jpg",
                        uuid.replace('-', "/"),
                        e,
                        e
                    )
                })
            });
        let covers = crate::services::common::pipeline::CoverCache::default();
        let cover_tmp = cover_tmp_for(&self.client, cover_url.as_deref(), settings, &covers).await;

        let video_artists: Vec<String> = meta["artists"]
            .as_array()
            .map(|arr: &Vec<Value>| {
                arr.iter()
                    .filter_map(|a| a["name"].as_str())
                    .map(str::to_string)
                    .collect()
            })
            .filter(|v: &Vec<String>| !v.is_empty())
            .unwrap_or_else(|| one(Some(artist.clone())));
        let metadata = TrackMetadata {
            title: Some(title.clone()),
            artist: video_artists,
            album_artist: one(Some(artist.clone())),
            album: (!album.is_empty()).then_some(album),
            year: (!release_date.is_empty()).then_some(release_date),
            isrc: meta["isrc"].as_str().map(|s| s.to_string()),
            track_number: meta["trackNumber"].as_u64().map(|n| n as u32),
            disc_number: meta["volumeNumber"].as_u64().map(|n| n as u32),
            copyright: meta["copyright"].as_str().map(|s| s.to_string()),
            ..Default::default()
        };
        tag_file(
            &dest_path,
            &metadata,
            settings
                .embed_cover
                .then(|| cover_tmp.as_ref().map(|t| t.path()))
                .flatten(),
            &excluded_tags(settings),
        )
        .await?;

        save_cover_file(
            dest,
            cover_tmp.as_ref(),
            CoverFiles::from_settings(settings),
            Some(&file_stem),
        )
        .await;

        if let Some(d) = dedup {
            d.record("tidal-video", video_id, &dest_path.to_string_lossy(), rank);
        }
        Ok(dest_path)
    }
}

#[cfg(test)]
mod manifest_tests {
    use super::{attr_of, best_representation, representation_id_spec};

    /// The bit depth is published nowhere else: `playbackinfo` omits it for hi-res,
    /// so without the id the label fell back to a guess of 24.
    #[test]
    fn a_representation_id_carries_the_rate_and_depth() {
        assert_eq!(
            representation_id_spec("FLAC_HIRES,44100,24"),
            (Some(44_100), Some(24))
        );
        assert_eq!(representation_id_spec("AACLC,44100"), (Some(44_100), None));
        assert_eq!(representation_id_spec("mp4a.40.2"), (None, None));
    }

    /// Reading only the first match — which a bare `captures()` does — labels the
    /// file after whichever representation Tidal happened to list first.
    #[test]
    fn attr_of_respects_the_leading_word_boundary() {
        let tag = r#"<R uid="nope" id="yes" empty="" />"#;
        assert_eq!(attr_of(tag, "id").as_deref(), Some("yes"));
        assert_eq!(attr_of(tag, "uid").as_deref(), Some("nope"));
        assert_eq!(attr_of(tag, "empty").as_deref(), Some(""));
        assert_eq!(attr_of(tag, "missing"), None);
    }

    #[test]
    fn the_served_representation_wins_over_the_first_one_listed() {
        let mpd = r#"<MPD>
            <Representation id="AACLC,44100" codecs="mp4a.40.2" bandwidth="322000" audioSamplingRate="44100"/>
            <Representation id="FLAC_HIRES,44100,24" codecs="flac" bandwidth="1461000" audioSamplingRate="44100"/>
        </MPD>"#;
        let rep = best_representation(mpd).expect("a representation");
        assert_eq!(attr_of(&rep, "codecs").as_deref(), Some("flac"));
        assert_eq!(attr_of(&rep, "bandwidth").as_deref(), Some("1461000"));
        assert_eq!(
            representation_id_spec(&attr_of(&rep, "id").unwrap()),
            (Some(44_100), Some(24))
        );
    }
}

use crate::services::common::pipeline::orchestrator::{
    extract_or_err, extract_platform_id, progress_to_fn, ContentType, Platform, SharedItem,
    SharedLog, SharedProgress, SharedQuality, TrackSourceClient,
};

#[async_trait::async_trait]
impl TrackSourceClient for TidalClient {
    fn platform_name(&self) -> &'static str {
        "Tidal"
    }

    fn requested_quality(&self, settings: &Settings) -> u8 {
        settings.tidal_quality
    }

    fn extract_id(&self, url: &str, content_type: ContentType) -> Option<String> {
        extract_platform_id(url, Platform::Tidal, content_type)
    }

    fn album_url(&self, album_id: &str) -> String {
        format!("https://tidal.com/browse/album/{}", album_id)
    }

    fn album_quality_format(&self, album: &AlbumInfo, settings: &Settings) -> (String, String) {
        let requested = settings.tidal_quality;
        let effective = match album.bit_depth {
            Some(bd) if bd < 24 && requested > 2 => 2,
            _ => requested,
        };
        (
            tidal_quality_label(effective).to_string(),
            tidal_format(effective).to_string(),
        )
    }

    fn playlist_quality_format(&self, settings: &Settings) -> (String, String) {
        let quality = settings.tidal_quality;
        (
            tidal_quality_label(quality).to_string(),
            tidal_format(quality).to_string(),
        )
    }

    async fn probe_release_quality(
        &self,
        first_track_id: &str,
        quality: u8,
        _settings: &Settings,
    ) -> Option<(String, String)> {
        TidalClient::probe_served_quality(self, first_track_id, quality)
            .await
            .ok()
    }

    async fn get_album_tracks(&self, album_id: &str) -> MhResult<AlbumInfo> {
        TidalClient::get_album_tracks(self, album_id).await
    }

    async fn get_playlist_tracks(&self, playlist_id: &str) -> MhResult<PlaylistInfo> {
        TidalClient::get_playlist_tracks(self, playlist_id).await
    }

    async fn get_artist_albums(
        &self,
        artist_id: &str,
        _settings: &Settings,
    ) -> MhResult<Vec<String>> {
        TidalClient::get_artist_albums(self, artist_id).await
    }

    async fn download_track(&self, job: TrackJob<'_>) -> MhResult<TrackOutcome> {
        TidalClient::download_track(self, job).await
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
            "Tidal does not support label downloads.".into(),
        ))
    }

    async fn download_video(
        &self,
        url: &str,
        base_dir: &Path,
        settings: &Settings,
        on_progress: SharedProgress,
        on_log: SharedLog,
        dedup: Option<&DedupLedger>,
    ) -> MhResult<()> {
        if !settings.tidal_download_videos {
            return Err(MhError::Unsupported(
                "Tidal video downloads are turned off. Enable them in Settings → Tidal → Videos."
                    .into(),
            ));
        }
        let id = extract_or_err(self, url, ContentType::Video)?;
        on_log(format!("Video: {}", id));
        tokio::fs::create_dir_all(base_dir).await?;
        let path = TidalClient::download_video(
            self,
            &id,
            base_dir,
            settings,
            progress_to_fn(on_progress),
            dedup,
        )
        .await?;
        on_log(format!("  ✓ {}", path.display()));
        Ok(())
    }
}

/// What Tidal actually served, named after the tier it answered with rather than
/// the one that was asked for. Tidal retired MQA in 2024 — `HI_RES` is now an
/// alias for `HI_RES_LOSSLESS`, and both mean plain hi-res FLAC.
pub fn tidal_served_label(
    audio_quality: &str,
    bit_depth: Option<u32>,
    sample_rate: Option<u32>,
    bitrate_bps: Option<u32>,
) -> (String, &'static str) {
    let lossy = |nominal: u32| match bitrate_bps {
        Some(bps) if bps > 0 => format!("{}kbps", (bps as f64 / 1000.0).round() as u32),
        _ => format!("{nominal}kbps"),
    };
    match audio_quality.to_ascii_uppercase().as_str() {
        "LOW" => (lossy(96), "AAC"),
        "HIGH" => (lossy(320), "AAC"),
        "LOSSLESS" => (
            crate::services::common::pipeline::depth_rate_label(
                bit_depth.unwrap_or(16),
                sample_rate.unwrap_or(44_100),
            ),
            "FLAC",
        ),
        "HI_RES" | "HI_RES_LOSSLESS" => (
            crate::services::common::pipeline::depth_rate_label(
                bit_depth.unwrap_or(24),
                sample_rate.unwrap_or(44_100),
            ),
            "FLAC",
        ),
        _ => match (bit_depth, sample_rate) {
            (Some(bd), Some(sr)) => (
                crate::services::common::pipeline::depth_rate_label(bd, sr),
                "FLAC",
            ),
            _ => (audio_quality.to_string(), "FLAC"),
        },
    }
}

/// Fallback for the moment before the first track reveals what was served — a
/// collection folder needs a name up front. Deliberately carries no sample rate
/// for the hi-res tier, because that varies per track.
pub fn tidal_quality_label(quality: u8) -> &'static str {
    match quality {
        0 => "96kbps",
        1 => "320kbps",
        2 => "16-bit ⁄ 44.1kHz",
        _ => "24-bit",
    }
}

pub fn tidal_format(quality: u8) -> &'static str {
    match quality {
        0 | 1 => "AAC",
        _ => "FLAC",
    }
}

#[cfg(test)]
mod quality_label_tests {
    use super::*;

    /// Every row of a live `playbackinfopostpaywall` probe. Tidal answers a
    /// `HI_RES` request with `HI_RES_LOSSLESS`; neither is MQA, which left the
    /// catalogue in 2024.
    #[test]
    fn a_tidal_track_is_named_after_what_tidal_served() {
        assert_eq!(
            tidal_served_label("LOW", None, None, None),
            ("96kbps".to_string(), "AAC")
        );
        assert_eq!(
            tidal_served_label("HIGH", None, None, None),
            ("320kbps".to_string(), "AAC")
        );
        assert_eq!(
            tidal_served_label("LOSSLESS", Some(16), Some(44_100), None),
            ("16-bit ⁄ 44.1kHz".to_string(), "FLAC")
        );
        assert_eq!(
            tidal_served_label("HI_RES_LOSSLESS", Some(24), Some(44_100), None),
            ("24-bit ⁄ 44.1kHz".to_string(), "FLAC")
        );
        assert_eq!(
            tidal_served_label("HI_RES_LOSSLESS", Some(24), Some(192_000), None),
            ("24-bit ⁄ 192kHz".to_string(), "FLAC")
        );
    }

    #[test]
    fn a_downgraded_tidal_track_is_not_labelled_as_the_request() {
        let (label, format) = tidal_served_label("LOSSLESS", Some(16), Some(44_100), None);
        assert_eq!(label, "16-bit ⁄ 44.1kHz");
        assert_eq!(format, "FLAC");
        assert_ne!(label, tidal_quality_label(3));
    }

    /// The manifest reports the real rate; the tier names round it off.
    #[test]
    fn a_lossy_tier_reports_the_bitrate_the_manifest_gave() {
        assert_eq!(
            tidal_served_label("LOW", None, None, Some(97_000)),
            ("97kbps".to_string(), "AAC")
        );
        assert_eq!(
            tidal_served_label("HIGH", None, None, Some(322_000)),
            ("322kbps".to_string(), "AAC")
        );
    }

    /// An unrecognised tier used to be echoed into the filename verbatim, which is
    /// how the literal string `HI_RES_LOSSLESS` ended up in folder names.
    #[test]
    fn an_unknown_tier_still_reports_a_readable_spec() {
        assert_eq!(
            tidal_served_label("SOMETHING_NEW", Some(24), Some(96_000), None),
            ("24-bit ⁄ 96kHz".to_string(), "FLAC")
        );
    }

    #[test]
    fn the_pre_first_track_fallback_never_claims_a_sample_rate_it_cannot_know() {
        assert_eq!(tidal_quality_label(0), "96kbps");
        assert_eq!(tidal_quality_label(1), "320kbps");
        assert_eq!(tidal_quality_label(2), "16-bit ⁄ 44.1kHz");
        assert_eq!(tidal_quality_label(3), "24-bit");
    }
}

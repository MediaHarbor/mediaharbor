use std::path::{Path, PathBuf};
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_tidal_client;
use crate::defaults::Settings;
use crate::streamrip::tagger::{TrackMetadata, tag_file, download_cover_art};
use crate::streamrip::downloader::download_file;

const BASE: &str = "https://api.tidalhifi.com/v1";
const AUTH_URL: &str = "https://auth.tidal.com/v1/oauth2";

pub const TIDAL_CLIENT_ID: &str = "6BDSRdpK9hqEBTgU";
const TIDAL_CLIENT_SECRET: &str = "xeuPmY7nbpZ9IIbLAcQ93shka1VNheUAqN6IcszjTG8=";

const QUALITY_MAP: &[(u8, &str)] = &[
    (0, "LOW"),
    (1, "HIGH"),
    (2, "LOSSLESS"),
    (3, "HI_RES"),
];

fn quality_str(q: u8) -> &'static str {
    QUALITY_MAP.iter().find(|(k, _)| *k == q).map(|(_, v)| *v).unwrap_or("LOSSLESS")
}

fn build_file_name(
    template: &str,
    vars: &std::collections::HashMap<&str, String>,
    restrict: bool,
    truncate: usize,
) -> String {
    let opt_re = regex::Regex::new(r"[\[(][^\])\[]*\{[^}]+\}[^\])\[]*[\])]").unwrap();
    let tpl = opt_re.replace_all(template, |caps: &regex::Captures| {
        let seg = &caps[0];
        let tok_re = regex::Regex::new(r"\{(\w+)(?::\d+)?\}").unwrap();
        let all_empty = tok_re.captures_iter(seg).all(|c| {
            let key = c.get(1).map(|m| m.as_str()).unwrap_or("");
            vars.get(key).map(|v| v.is_empty() || v == "0").unwrap_or(true)
        });
        if all_empty { String::new() } else { seg.to_string() }
    });

    let val_re = regex::Regex::new(r"\{(\w+)(?::(\d+))?\}").unwrap();
    let mut name = val_re.replace_all(&tpl, |caps: &regex::Captures| {
        let key = &caps[1];
        let pad = caps.get(2).and_then(|m| m.as_str().parse::<usize>().ok());
        let val = vars.get(key).cloned().unwrap_or_default();
        if let Some(p) = pad {
            if val.chars().all(|c| c.is_ascii_digit()) && !val.is_empty() {
                return format!("{:0>width$}", val, width = p);
            }
        }
        val
    }).to_string();

    if restrict {
        name = name.chars().map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            '\x00'..='\x1f' => '_',
            other => other,
        }).collect();
    }

    if truncate > 0 && name.len() > truncate {
        name = name[..truncate].to_string();
    }
    name.trim().to_string()
}

#[derive(Debug, Clone)]
pub struct TidalStreamInfo {
    pub url: Option<String>,
    pub manifest: Option<String>,
    pub manifest_mime: String,
    pub audio_quality: String,
    pub encryption_key: Option<String>,
}

#[derive(Debug)]
enum Downloadable {
    Dash {
        init_url: String,
        segment_urls: Vec<String>,
        codec: String,
        ext: String,
        needs_remux: bool,
        actual_quality: String,
    },
    Direct {
        url: String,
        codec: String,
        ext: String,
        enc_key: Option<String>,
        actual_quality: String,
    },
}

impl Downloadable {
    fn ext(&self) -> &str {
        match self {
            Downloadable::Dash { ext, .. } => ext,
            Downloadable::Direct { ext, .. } => ext,
        }
    }

    fn actual_quality(&self) -> &str {
        match self {
            Downloadable::Dash { actual_quality, .. } => actual_quality,
            Downloadable::Direct { actual_quality, .. } => actual_quality,
        }
    }

    fn codec(&self) -> &str {
        match self {
            Downloadable::Dash { codec, .. } => codec,
            Downloadable::Direct { codec, .. } => codec,
        }
    }
}

pub struct TidalClient {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
    pub country_code: String,
    token_expiry: f64,
    pub client: reqwest::Client,
}

impl TidalClient {
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
            access_token,
            refresh_token,
            user_id,
            country_code,
            token_expiry,
            client,
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

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
            base64::engine::general_purpose::STANDARD.encode(
                format!("{}:{}", TIDAL_CLIENT_ID, TIDAL_CLIENT_SECRET)
            )
        };

        let refresh_token_encoded: String = url::form_urlencoded::byte_serialize(
            self.refresh_token.as_bytes()
        ).collect();
        let body = format!(
            "client_id={}&refresh_token={}&grant_type=refresh_token&scope=r_usr%2Bw_usr%2Bw_sub",
            TIDAL_CLIENT_ID,
            refresh_token_encoded
        );

        let resp = self.client
            .post(&format!("{}/token", AUTH_URL))
            .header("Authorization", format!("Basic {}", auth))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send().await?;

        if !resp.status().is_success() {
            return Err(MhError::Auth("Tidal: token refresh failed".into()));
        }

        let json: Value = resp.json().await?;
        self.access_token = json["access_token"]
            .as_str()
            .ok_or_else(|| MhError::Auth("Tidal: no access_token in refresh response".into()))?
            .to_string();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        self.token_expiry = now + json["expires_in"].as_f64().unwrap_or(0.0);

        Ok(())
    }

    async fn verify_token(&mut self) -> MhResult<()> {
        let resp = self.client
            .get("https://api.tidal.com/v1/sessions")
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send().await?;
        if !resp.status().is_success() {
            return Err(MhError::Auth("Tidal: access token invalid".into()));
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
        let mut params: Vec<(&str, &str)> = vec![
            ("countryCode", &self.country_code),
            ("limit", "100"),
        ];
        params.extend_from_slice(extra_params);

        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .query(&params)
            .send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(MhError::Other(format!("Tidal API HTTP {}: {}", status, text)));
        }
        Ok(resp.json().await?)
    }

    async fn api_get_raw(&self, path: &str, extra_params: &[(&str, &str)]) -> MhResult<(u16, String)> {
        let url = format!("{}/{}", BASE, path);
        let mut params: Vec<(&str, &str)> = vec![
            ("countryCode", &self.country_code),
            ("limit", "100"),
        ];
        params.extend_from_slice(extra_params);

        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .query(&params)
            .send().await?;
        let status = resp.status().as_u16();
        let text = resp.text().await?;
        Ok((status, text))
    }

    async fn get_downloadable(&self, track_id: &str, quality: u8) -> MhResult<Downloadable> {
        if quality > 3 {
            return Err(MhError::Other(format!("No streamable format for track {}", track_id)));
        }

        let q_str = quality_str(quality);
        let (status, text) = self.api_get_raw(
            &format!("tracks/{}/playbackinfopostpaywall", track_id),
            &[("audioquality", q_str), ("playbackmode", "STREAM"), ("assetpresentation", "FULL")],
        ).await?;

        if status == 401 || status == 403 {
            if quality > 0 {
                return Box::pin(self.get_downloadable(track_id, quality - 1)).await;
            }
            return Err(MhError::Other(format!("Tidal: no accessible quality for track {}", track_id)));
        }
        if status == 404 {
            return Err(MhError::NotFound(format!("Tidal: track {} not found", track_id)));
        }
        if status != 200 {
            return Err(MhError::Other(format!("Tidal: HTTP {} for track {}", status, track_id)));
        }

        let json: Value = serde_json::from_str(&text)?;
        let manifest_mime = json["manifestMimeType"].as_str().unwrap_or("").to_string();
        let actual_quality = json["audioQuality"].as_str().unwrap_or(q_str).to_string();

        use base64::Engine;
        let raw_manifest = json["manifest"].as_str()
            .and_then(|m| base64::engine::general_purpose::STANDARD.decode(m).ok())
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();

        if manifest_mime == "application/dash+xml" {
            return self.parse_dash_manifest_to_downloadable(&raw_manifest, &actual_quality);
        }

        let manifest: Value = serde_json::from_str(&raw_manifest).map_err(|_| {
            if quality > 0 {
                MhError::Other(format!("retry_lower_quality:{}", quality - 1))
            } else {
                MhError::Parse(format!("Tidal: failed to parse manifest for {}", track_id))
            }
        })?;

        let enc_key = if manifest["encryptionType"].as_str() == Some("NONE") {
            None
        } else {
            manifest["keyId"].as_str().map(|s| s.to_string())
        };

        let codec = manifest["codecs"].as_str().unwrap_or("").to_lowercase();
        let ext = if codec == "flac" || codec == "mqa" { "flac" } else { "m4a" };

        let url = manifest["urls"][0].as_str().ok_or_else(|| {
            if quality > 0 {
                MhError::Other(format!("retry_lower_quality:{}", quality - 1))
            } else {
                MhError::Other(format!("Tidal: no URL in manifest for {}", track_id))
            }
        })?;

        Ok(Downloadable::Direct {
            url: url.to_string(),
            codec,
            ext: ext.to_string(),
            enc_key,
            actual_quality,
        })
    }

    fn parse_dash_manifest_to_downloadable(&self, mpd: &str, actual_quality: &str) -> MhResult<Downloadable> {
        let segments = self.parse_dash_manifest_inner(mpd)?;
        let (init_url, segment_urls) = if segments.is_empty() {
            (String::new(), vec![])
        } else {
            (segments[0].clone(), segments[1..].to_vec())
        };

        let codec_re = regex::Regex::new(r#"<Representation[^>]+codecs="([^"]+)""#).unwrap();
        let codec = codec_re.captures(mpd)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_lowercase())
            .unwrap_or_else(|| "flac".to_string());

        let ext = if codec == "flac" { "flac" } else { "m4a" };
        let needs_remux = codec == "flac";

        Ok(Downloadable::Dash {
            init_url,
            segment_urls,
            codec,
            ext: ext.to_string(),
            needs_remux,
            actual_quality: actual_quality.to_string(),
        })
    }

    pub fn parse_dash_manifest(&self, manifest_b64: &str) -> MhResult<Vec<String>> {
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(manifest_b64)
            .map_err(|e| MhError::Parse(e.to_string()))?;
        let mpd = String::from_utf8(raw).map_err(|e| MhError::Parse(e.to_string()))?;
        self.parse_dash_manifest_inner(&mpd)
    }

    fn parse_dash_manifest_inner(&self, mpd: &str) -> MhResult<Vec<String>> {
        let get_attr = |tag: &str, attr: &str| -> Option<String> {
            let re = regex::Regex::new(&format!(r#"<{tag}[^>]+{attr}="([^"]+)""#, tag=tag, attr=attr)).ok()?;
            re.captures(mpd).and_then(|c| c.get(1)).map(|m| m.as_str().to_string())
        };

        let init_url = get_attr("SegmentTemplate", "initialization").unwrap_or_default();
        let media_template = get_attr("SegmentTemplate", "media").unwrap_or_default();
        let start_number: u64 = get_attr("SegmentTemplate", "startNumber")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        let s_re = regex::Regex::new(r"<S\s[^>]*>").unwrap();
        let mut total_segments: u64 = 0;
        for m in s_re.find_iter(mpd) {
            let tag = m.as_str();
            let r_val: u64 = regex::Regex::new(r#"r="(\d+)""#).ok()
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

    async fn download_dash(&self, init_url: &str, segment_urls: &[String], needs_remux: bool, dest_path: &Path, on_progress: impl Fn(u64, u64)) -> MhResult<()> {
        let mut all_urls = vec![init_url.to_string()];
        all_urls.extend_from_slice(segment_urls);

        let tmp = dest_path.with_extension("tmp.m4a");

        let headers = crate::http_client::build_headers(&[
            ("Authorization", &format!("Bearer {}", self.access_token)),
        ])?;

        let total = all_urls.len() as u64;
        {
            use tokio::io::AsyncWriteExt;
            let mut out = tokio::fs::File::create(&tmp).await?;
            for (i, url) in all_urls.iter().enumerate() {
                let resp = self.client
                    .get(url)
                    .headers(headers.clone())
                    .send().await?;
                if !resp.status().is_success() {
                    return Err(MhError::Other(format!("Tidal segment HTTP {}", resp.status().as_u16())));
                }
                let bytes = resp.bytes().await?;
                out.write_all(&bytes).await?;
                on_progress((i + 1) as u64, total);
            }
        }

        if needs_remux {
            let output = tokio::process::Command::new("ffmpeg")
                .args(&["-y", "-loglevel", "error", "-i"])
                .arg(&tmp)
                .args(&["-vn", "-c:a", "flac"])
                .arg(dest_path)
                .output().await
                .map_err(|e| MhError::Subprocess(e.to_string()))?;
            let _ = tokio::fs::remove_file(&tmp).await;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(MhError::Subprocess(format!("ffmpeg remux failed: {}", &err[..err.len().min(200)])));
            }
        } else {
            tokio::fs::rename(&tmp, dest_path).await?;
        }

        Ok(())
    }

    fn get_album_art_url(album_meta: &Value) -> Option<String> {
        album_meta["cover"].as_str().map(|uuid| {
            let path = uuid.replace('-', "/");
            format!("https://resources.tidal.com/images/{}/1280x1280.jpg", path)
        })
    }

    pub async fn fetch_audio_for_streaming(&self, track_id: &str) -> MhResult<(bytes::Bytes, &'static str)> {
        let downloadable = self.get_downloadable(track_id, 3).await?;
        match downloadable {
            Downloadable::Direct { url, ext, enc_key, .. } => {
                if enc_key.is_some() {
                    return Err(MhError::Other(
                        "Encrypted Tidal track is not supported for streaming".into(),
                    ));
                }
                let mime: &'static str = if ext == "flac" { "audio/flac" } else { "audio/mp4" };
                let resp = self.client
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", self.access_token))
                    .send().await.map_err(MhError::Network)?;
                if !resp.status().is_success() {
                    return Err(MhError::Other(format!("Tidal: CDN HTTP {} for track {}", resp.status().as_u16(), track_id)));
                }
                Ok((resp.bytes().await.map_err(MhError::Network)?, mime))
            },
            Downloadable::Dash { init_url, segment_urls, ext, .. } => {
                let mime: &'static str = if ext == "flac" { "audio/flac" } else { "audio/mp4" };
                let mut all_urls = vec![init_url];
                all_urls.extend(segment_urls);
                let mut all_bytes: Vec<u8> = Vec::new();
                for url in &all_urls {
                    let resp = self.client
                        .get(url)
                        .header("Authorization", format!("Bearer {}", self.access_token))
                        .send().await.map_err(MhError::Network)?;
                    if !resp.status().is_success() {
                        return Err(MhError::Other(format!("Tidal: DASH segment HTTP {} for track {}", resp.status().as_u16(), track_id)));
                    }
                    let chunk = resp.bytes().await.map_err(MhError::Network)?;
                    all_bytes.extend_from_slice(&chunk);
                }
                Ok((bytes::Bytes::from(all_bytes), mime))
            },
        }
    }

    pub async fn fetch_audio_progressive(
        &self,
        track_id: &str,
        tx: tokio::sync::mpsc::Sender<Result<bytes::Bytes, String>>,
    ) -> MhResult<&'static str> {
        use futures_util::StreamExt;

        let downloadable = self.get_downloadable(track_id, 3).await?;
        let mime: &'static str = if downloadable.ext() == "flac" { "audio/flac" } else { "audio/mp4" };

        let client = self.client.clone();
        let access_token = self.access_token.clone();

        tokio::spawn(async move {
            match downloadable {
                Downloadable::Direct { url, enc_key, .. } => {
                    if enc_key.is_some() {
                        let _ = tx.send(Err("Encrypted Tidal track not supported for streaming".into())).await;
                        return;
                    }
                    match client.get(&url)
                        .header("Authorization", format!("Bearer {}", access_token))
                        .send().await
                    {
                        Ok(resp) => {
                            let mut stream = resp.bytes_stream();
                            while let Some(result) = stream.next().await {
                                match result {
                                    Ok(chunk) => { if tx.send(Ok(chunk)).await.is_err() { return; } }
                                    Err(e) => { let _ = tx.send(Err(e.to_string())).await; return; }
                                }
                            }
                        }
                        Err(e) => { let _ = tx.send(Err(e.to_string())).await; }
                    }
                }
                Downloadable::Dash { init_url, segment_urls, .. } => {
                    let all_urls: Vec<String> = std::iter::once(init_url).chain(segment_urls).collect();
                    for url in all_urls {
                        match client.get(&url)
                            .header("Authorization", format!("Bearer {}", access_token))
                            .send().await
                        {
                            Ok(resp) => {
                                let mut stream = resp.bytes_stream();
                                while let Some(result) = stream.next().await {
                                    match result {
                                        Ok(chunk) => { if tx.send(Ok(chunk)).await.is_err() { return; } }
                                        Err(e) => { let _ = tx.send(Err(e.to_string())).await; return; }
                                    }
                                }
                            }
                            Err(e) => { let _ = tx.send(Err(e.to_string())).await; return; }
                        }
                    }
                }
            }
        });

        Ok(mime)
    }

    pub async fn get_stream_url(&self, track_id: &str) -> MhResult<TidalStreamInfo> {
        let (status, text) = self.api_get_raw(
            &format!("tracks/{}/playbackinfopostpaywall", track_id),
            &[("audioquality", "HI_RES"), ("playbackmode", "STREAM"), ("assetpresentation", "FULL")],
        ).await?;

        if !matches!(status, 200..=299) {
            return Err(MhError::Other(format!("Tidal: HTTP {} for track {}", status, track_id)));
        }

        let json: Value = serde_json::from_str(&text)?;

        use base64::Engine;
        let manifest = json["manifest"].as_str().map(|m| {
            base64::engine::general_purpose::STANDARD.decode(m)
                .ok().and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_default()
        });

        Ok(TidalStreamInfo {
            url: None,
            manifest,
            manifest_mime: json["manifestMimeType"].as_str().unwrap_or("").to_string(),
            audio_quality: json["audioQuality"].as_str().unwrap_or("").to_string(),
            encryption_key: json["encryptionKey"].as_str().map(|s| s.to_string()),
        })
    }

    pub async fn get_album_tracks(&self, album_id: &str) -> MhResult<AlbumInfo> {
        let tracks_path = format!("albums/{}/tracks", album_id);
        let album_path = format!("albums/{}", album_id);
        let (tracks_resp, album_resp) = tokio::join!(
            self.api_get(&tracks_path, &[]),
            self.api_get(&album_path, &[]),
        );
        let tracks = tracks_resp?;
        let album = album_resp.unwrap_or(Value::Null);

        let cover = album["cover"].as_str().map(|uuid| {
            let path = uuid.replace('-', "/");
            format!("https://resources.tidal.com/images/{}/320x320.jpg", path)
        }).unwrap_or_default();

        let items = tracks["items"].as_array().cloned().unwrap_or_default();
        Ok(AlbumInfo {
            track_ids: items.iter().filter_map(|t| t["id"].as_u64().map(|id| id.to_string())).collect(),
            track_disc_numbers: items.iter().map(|t| t["volumeNumber"].as_u64().unwrap_or(1) as u32).collect(),
            number_of_volumes: album["numberOfVolumes"].as_u64().unwrap_or(1) as u32,
            title: album["title"].as_str().unwrap_or(&format!("Album {}", album_id)).to_string(),
            artist: album["artist"]["name"].as_str().unwrap_or("").to_string(),
            thumbnail: cover,
            year: album["releaseDate"].as_str().and_then(|d| d.split('-').next()).unwrap_or("").to_string(),
            genre: album["genre"].as_str().unwrap_or("").to_string(),
            label: album["label"]["name"].as_str().unwrap_or("").to_string(),
        })
    }

    pub async fn get_playlist_tracks(&self, playlist_id: &str) -> MhResult<PlaylistInfo> {
        let resp = self.api_get(
            &format!("playlists/{}/tracks", playlist_id),
            &[("limit", "200")],
        ).await?;

        Ok(PlaylistInfo {
            track_ids: resp["items"].as_array()
                .map(|arr| arr.iter().filter_map(|t| t["id"].as_u64().map(|id| id.to_string())).collect())
                .unwrap_or_default(),
            title: format!("Playlist {}", playlist_id),
            artist: String::new(),
            thumbnail: String::new(),
        })
    }

    pub async fn get_album_details_json(&self, album_id: &str) -> MhResult<Value> {
        let tracks_path = format!("albums/{}/tracks", album_id);
        let album_path = format!("albums/{}", album_id);
        let (tracks_resp, album_resp) = tokio::join!(
            self.api_get(&tracks_path, &[("limit", "200")]),
            self.api_get(&album_path, &[]),
        );
        let tracks_data = tracks_resp?;
        let album_data = album_resp.unwrap_or(Value::Null);

        let items = tracks_data["items"].as_array().cloned().unwrap_or_default();
        let cover_url = album_data["cover"].as_str().map(|uuid| {
            format!("https://resources.tidal.com/images/{}/640x640.jpg", uuid.replace('-', "/"))
        });
        Ok(serde_json::json!({
            "tracks": items,
            "thumbnail": cover_url,
            "album": {
                "title": album_data["title"],
                "artist": album_data["artist"]["name"],
                "releaseDate": album_data["releaseDate"],
                "coverUrl": cover_url,
            }
        }))
    }

    pub async fn get_playlist_details_json(&self, playlist_id: &str) -> MhResult<Value> {
        let tracks_path = format!("playlists/{}/tracks", playlist_id);
        let meta_path = format!("playlists/{}", playlist_id);
        let (tracks_resp, meta_resp) = tokio::join!(
            self.api_get(&tracks_path, &[("limit", "200")]),
            self.api_get(&meta_path, &[]),
        );
        let tracks_data = tracks_resp?;
        let meta = meta_resp.unwrap_or(Value::Null);

        let items = tracks_data["items"].as_array().cloned().unwrap_or_default();
        let cover_url = meta["image"].as_str().map(|uuid| {
            format!("https://resources.tidal.com/images/{}/640x640.jpg", uuid.replace('-', "/"))
        });
        Ok(serde_json::json!({
            "tracks": items,
            "thumbnail": cover_url,
            "playlist": {
                "title": meta["title"],
                "creator": meta["creator"]["name"],
                "coverUrl": cover_url,
            }
        }))
    }

    pub async fn get_artist_albums(&self, artist_id: &str) -> MhResult<Vec<String>> {
        let albums_path = format!("artists/{}/albums", artist_id);
        let eps_path = format!("artists/{}/albums", artist_id);
        let (albums_resp, eps_resp) = tokio::join!(
            self.api_get(&albums_path, &[("limit", "500")]),
            self.api_get(&eps_path, &[("filter", "EPSANDSINGLES"), ("limit", "500")]),
        );

        let mut ids = Vec::new();
        if let Ok(albums) = albums_resp {
            for item in albums["items"].as_array().unwrap_or(&vec![]) {
                if let Some(id) = item["id"].as_u64() {
                    ids.push(id.to_string());
                }
            }
        }
        if let Ok(eps) = eps_resp {
            for item in eps["items"].as_array().unwrap_or(&vec![]) {
                if let Some(id) = item["id"].as_u64() {
                    ids.push(id.to_string());
                }
            }
        }
        Ok(ids)
    }

    async fn fetch_lyrics(&self, track_id: &str) -> Option<Value> {
        let qs = format!("countryCode={}", self.country_code);
        let hosts = [
            format!("https://api.tidal.com/v1/tracks/{}/lyrics?{}", track_id, qs),
            format!("https://api.tidalhifi.com/v1/tracks/{}/lyrics?{}", track_id, qs),
        ];
        for url in &hosts {
            if let Ok(resp) = self.client
                .get(url)
                .header("Authorization", format!("Bearer {}", self.access_token))
                .header("X-Tidal-Token", TIDAL_CLIENT_ID)
                .send().await
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

    pub async fn download_track(
        &self,
        track_id: &str,
        quality: u8,
        dest: &Path,
        settings: &Settings,
        on_progress: impl Fn(u64, u64),
        in_collection: bool,
    ) -> MhResult<PathBuf> {
        let downloadable = self.get_downloadable(track_id, quality).await?;

        let meta_result = self.api_get(&format!("tracks/{}", track_id), &[]).await.ok();
        let meta = meta_result.as_ref().unwrap_or(&Value::Null);
        let album_id = meta["album"]["id"].as_u64().map(|id: u64| id.to_string());
        let album_meta: Option<Value> = if let Some(ref aid) = album_id {
            self.api_get(&format!("albums/{}", aid), &[]).await.ok()
        } else {
            None
        };
        let contributors: Option<Value> = self.api_get(&format!("tracks/{}/contributors", track_id), &[]).await.ok();

        let title = meta["title"].as_str().unwrap_or(&format!("track_{}", track_id)).to_string();
        let artist = meta["artists"].as_array()
            .map(|arr: &Vec<Value>| arr.iter().filter_map(|a| a["name"].as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_else(|| "Unknown".to_string());
        let albumartist = album_meta.as_ref()
            .and_then(|m| m["artist"]["name"].as_str())
            .unwrap_or(&artist)
            .to_string();
        let album = meta["album"]["title"].as_str().unwrap_or("").to_string();
        let track_num = meta["trackNumber"].as_u64().unwrap_or(0) as u32;
        let disc_num = meta["volumeNumber"].as_u64().unwrap_or(1) as u32;
        let tracktotal = album_meta.as_ref()
            .and_then(|m| m["numberOfTracks"].as_u64())
            .map(|n| n.to_string())
            .unwrap_or_default();
        let disctotal = album_meta.as_ref()
            .and_then(|m| m["numberOfVolumes"].as_u64())
            .map(|n| n.to_string())
            .unwrap_or_default();
        let year = meta["album"]["releaseDate"].as_str()
            .and_then(|d: &str| d.split('-').next())
            .unwrap_or("")
            .to_string();
        let genre = album_meta.as_ref()
            .and_then(|m| m["genre"].as_str())
            .unwrap_or("")
            .to_string();

        let track_template = if settings.filepaths_track_format.is_empty() {
            "{tracknumber:02}. {artist} - {title}".to_string()
        } else {
            settings.filepaths_track_format.clone()
        };

        let restrict = settings.filepaths_restrict_characters;
        let truncate = settings.filepaths_truncate_to as usize;
        let explicit_str = if meta["explicit"].as_bool().unwrap_or(false) {
            " (Explicit)".to_string()
        } else {
            String::new()
        };

        let mut vars = std::collections::HashMap::new();
        vars.insert("title", title.clone());
        vars.insert("artist", artist.clone());
        vars.insert("albumartist", albumartist.clone());
        vars.insert("album", album.clone());
        vars.insert("tracknumber", track_num.to_string());
        vars.insert("discnumber", disc_num.to_string());
        vars.insert("tracktotal", tracktotal.clone());
        vars.insert("disctotal", disctotal.clone());
        vars.insert("year", year.clone());
        vars.insert("genre", genre.clone());
        vars.insert("explicit", explicit_str);

        let ext = downloadable.ext().to_string();
        let quality_label = downloadable.actual_quality().to_string();
        let downloadable_codec = downloadable.codec().to_string();
        let vars_map: std::collections::HashMap<&str, String> = vars.iter().map(|(k, v): (&&str, &String)| (*k, v.clone())).collect();
        let file_stem = build_file_name(&track_template, &vars_map, restrict, truncate);
        let file_name = format!("{}.{}", file_stem, ext);

        let label = album_meta.as_ref().and_then(|m| m["label"]["name"].as_str()).unwrap_or("").to_string();
        let track_dest = if !in_collection {
            let folder = crate::streamrip::build_album_folder(
                &settings.filepaths_folder_format,
                &albumartist,
                &album,
                &year,
                &genre,
                &label,
            );
            let d = dest.join(folder);
            tokio::fs::create_dir_all(&d).await?;
            d
        } else {
            dest.to_path_buf()
        };

        let mut dest_path = track_dest.join(&file_name);
        tracing::debug!("Tidal: track codec={} quality={}", downloadable_codec, quality_label);

        let (lyrics_text, lrc_content) = if let Some(lyr) = self.fetch_lyrics(track_id).await {
            let text = lyr["lyrics"].as_str().map(|s| s.to_string());
            let lrc = lyr["subtitles"].as_str().map(|s| s.to_string());
            (text, lrc)
        } else {
            (None, None)
        };

        let cover_url = album_meta.as_ref().and_then(|m| Self::get_album_art_url(m));
        let embed_cover = settings.embed_cover;
        let save_cover = settings.save_cover;
        let cover_tmp = if let (Some(ref url), true) = (&cover_url, embed_cover || save_cover) {
            download_cover_art(&self.client, url).await.ok()
        } else {
            None
        };

        match &downloadable {
            Downloadable::Dash { init_url, segment_urls, needs_remux, .. } => {
                self.download_dash(init_url, segment_urls, *needs_remux, &dest_path, on_progress).await?;
            }
            Downloadable::Direct { url, enc_key, .. } => {
                let tmp_path = dest_path.with_extension(format!("{}.tmp", ext));
                let headers = crate::http_client::build_headers(&[
                    ("Authorization", &format!("Bearer {}", self.access_token)),
                ])?;
                download_file(&self.client, url, &tmp_path, Some(&headers), on_progress).await?;
                if let Some(ref key) = enc_key {
                    let encrypted = tokio::fs::read(&tmp_path).await?;
                    let decrypted = crate::crypto::tidal::decrypt_mqa(&encrypted, key);
                    tokio::fs::write(&dest_path, &decrypted).await?;
                    let _ = tokio::fs::remove_file(&tmp_path).await;
                } else {
                    tokio::fs::rename(&tmp_path, &dest_path).await?;
                }
            }
        }

        if save_cover {
            if let Some(ref tmp) = cover_tmp {
                let cover_dest = track_dest.join("cover.jpg");
                let _ = tokio::fs::copy(tmp.path(), &cover_dest).await;
            }
        }

        let contribs = contributors.as_ref()
            .and_then(|c| c["items"].as_array())
            .cloned()
            .unwrap_or_default();
        let by_role = |role: &str| -> Option<String> {
            let names: Vec<&str> = contribs.iter()
                .filter(|c| c["role"].as_str().map(|r| r.eq_ignore_ascii_case(role)).unwrap_or(false))
                .filter_map(|c| c["name"].as_str())
                .collect();
            if names.is_empty() { None } else { Some(names.join(", ")) }
        };

        let metadata = TrackMetadata {
            title: Some(title),
            artist: Some(artist),
            album: Some(album),
            album_artist: Some(albumartist),
            year: meta["album"]["releaseDate"].as_str().map(|s: &str| s.to_string()),
            genre: Some(genre),
            track_number: Some(track_num),
            disc_number: Some(disc_num),
            total_tracks: if tracktotal.is_empty() { None } else { tracktotal.parse().ok() },
            total_discs: if disctotal.is_empty() { None } else { disctotal.parse().ok() },
            isrc: meta["isrc"].as_str().map(|s: &str| s.to_string()),
            upc: album_meta.as_ref().and_then(|m| m["upc"].as_str()).map(|s| s.to_string()),
            copyright: album_meta.as_ref().and_then(|m| m["copyright"].as_str()).map(|s| s.to_string()),
            label: album_meta.as_ref().and_then(|m| m["label"]["name"].as_str()).map(|s| s.to_string()),
            composer: by_role("Composer"),
            conductor: None,
            performer: None,
            producer: by_role("Producer"),
            lyricist: by_role("Lyricist"),
            engineer: by_role("Engineer"),
            mixer: by_role("Mixer"),
            description: None,
            purchase_date: None,
            grouping: None,
            comment: None,
            lyrics: lyrics_text,
        };

        let cover_path = if embed_cover { cover_tmp.as_ref().map(|t| t.path()) } else { None };

        let exclude_tags: Vec<String> = if settings.meta_exclude_tags_check && !settings.excluded_tags.is_empty() {
            settings.excluded_tags.split(',').map(|s| s.trim().to_string()).collect()
        } else {
            vec![]
        };

        let _ = tag_file(&dest_path, &metadata, cover_path, &exclude_tags, "ffmpeg").await;

        if settings.conversion_check {
            use crate::streamrip::converter::{AudioCodec, ConversionSettings, convert_audio};
            let target_codec = match settings.conversion_codec.to_uppercase().as_str() {
                "MP3" => AudioCodec::Mp3,
                "AAC" => AudioCodec::Aac,
                "OPUS" => AudioCodec::Opus,
                "VORBIS" => AudioCodec::Vorbis,
                "ALAC" => AudioCodec::Alac,
                _ => AudioCodec::Flac,
            };
            let new_ext = target_codec.container();
            let new_dest = dest_path.with_extension(new_ext);
            let conv_settings = ConversionSettings {
                codec: target_codec,
                sampling_rate: settings.conversion_sampling_rate,
                bit_depth: settings.conversion_bit_depth,
                lossy_bitrate: Some(settings.conversion_lossy_bitrate),
            };
            if let Err(e) = convert_audio(&dest_path, &new_dest, &conv_settings, "ffmpeg").await {
                tracing::warn!("Tidal conversion failed: {}", e);
            } else if new_dest != dest_path {
                let _ = tokio::fs::remove_file(&dest_path).await;
                dest_path = new_dest;
            }
        }

        if settings.save_lrc_files {
            if let Some(lrc) = lrc_content {
                let lrc_path = dest_path.with_extension("lrc");
                let _ = tokio::fs::write(&lrc_path, lrc.as_bytes()).await;
            }
        }

        Ok(dest_path)
    }

    fn parse_best_m3u8_stream(m3u8: &str) -> Option<String> {
        let lines: Vec<&str> = m3u8.lines().collect();
        let mut best_bandwidth: i64 = -1;
        let mut best_url: Option<String> = None;
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i].trim();
            if line.starts_with("#EXT-X-STREAM-INF:") {
                let bw = regex::Regex::new(r"BANDWIDTH=(\d+)").ok()
                    .and_then(|re| re.captures(line))
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<i64>().ok())
                    .unwrap_or(0);
                if let Some(next) = lines.get(i + 1) {
                    let next = next.trim();
                    if !next.is_empty() && !next.starts_with('#') && bw > best_bandwidth {
                        best_bandwidth = bw;
                        best_url = Some(next.to_string());
                    }
                }
            }
            i += 1;
        }
        best_url
    }

    pub async fn download_video(
        &self,
        video_id: &str,
        dest: &Path,
        ffmpeg: &str,
        on_progress: impl Fn(u64, u64),
    ) -> MhResult<PathBuf> {
        let meta_resp = self.api_get(&format!("videos/{}", video_id), &[]).await;
        if let Err(e) = &meta_resp {
            if e.to_string().contains("404") {
                return Err(MhError::NotFound(format!("Tidal: video {} not found", video_id)));
            }
        }
        let meta = meta_resp?;

        let title = meta["title"].as_str().unwrap_or(&format!("video_{}", video_id)).to_string();
        let _artist = meta["artists"].as_array()
            .map(|arr| arr.iter().filter_map(|a| a["name"].as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_else(|| "Unknown".to_string());

        let stream_resp = self.api_get_raw(
            &format!("videos/{}/playbackinfopostpaywall", video_id),
            &[("videoquality", "HIGH"), ("playbackmode", "STREAM"), ("assetpresentation", "FULL")],
        ).await?;

        if stream_resp.0 != 200 {
            return Err(MhError::Other(format!("Tidal: could not get video stream for {} (HTTP {})", video_id, stream_resp.0)));
        }

        let stream_json: Value = serde_json::from_str(&stream_resp.1)?;
        use base64::Engine;
        let raw_manifest = stream_json["manifest"].as_str()
            .and_then(|m| base64::engine::general_purpose::STANDARD.decode(m).ok())
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();

        let video_url = if raw_manifest.starts_with("#EXTM3U") {
            Self::parse_best_m3u8_stream(&raw_manifest)
                .ok_or_else(|| MhError::Other(format!("Tidal: no streams in HLS manifest for {}", video_id)))?
        } else {
            let mf: Value = serde_json::from_str(&raw_manifest).unwrap_or(Value::Null);
            mf["urls"][0].as_str()
                .ok_or_else(|| MhError::Other(format!("Tidal: no video URL available for {}", video_id)))?
                .to_string()
        };

        let safe_title: String = title.chars().map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            other => other,
        }).collect();
        let file_name = format!("{}.mp4", &safe_title[..safe_title.len().min(120)]);
        let dest_path = dest.join(&file_name);
        tokio::fs::create_dir_all(dest).await?;

        let total_secs = meta["duration"].as_u64().unwrap_or(0);
        let output = tokio::process::Command::new(ffmpeg)
            .args(&["-y", "-loglevel", "error", "-user_agent", "Mozilla/5.0", "-i", &video_url, "-c", "copy"])
            .arg(&dest_path)
            .output().await
            .map_err(|e| MhError::Subprocess(format!("ffmpeg spawn failed: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(MhError::Subprocess(format!("ffmpeg failed: {}", &err[..err.len().min(300)])));
        }

        on_progress(total_secs, total_secs.max(1));

        Ok(dest_path)
    }
}

#[derive(Debug, Clone)]
pub struct AlbumInfo {
    pub track_ids: Vec<String>,
    pub track_disc_numbers: Vec<u32>,
    pub number_of_volumes: u32,
    pub title: String,
    pub artist: String,
    pub thumbnail: String,
    pub year: String,
    pub genre: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct PlaylistInfo {
    pub track_ids: Vec<String>,
    pub title: String,
    pub artist: String,
    pub thumbnail: String,
}

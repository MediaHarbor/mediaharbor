use std::path::{Path, PathBuf};
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_mozilla_client;
use crate::defaults::Settings;
use crate::streamrip::tagger::{TrackMetadata, tag_file, download_cover_art};
use crate::streamrip::downloader::download_file;
use reqwest::header::HeaderMap;

const BASE_URL: &str = "https://www.qobuz.com/api.json/0.2";

fn quality_to_format_id(quality: u8) -> u32 {
    match quality {
        5 | 6 | 7 | 27 => quality as u32,  // Qobuz format IDs passed directly from UI
        1 => 5,
        2 => 6,
        3 => 7,
        _ => 27,
    }
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
            '<'  => '＜',
            '>'  => '＞',
            ':'  => '：',
            '"'  => '＂',
            '/'  => '⁄',
            '\\' => '＼',
            '|'  => '｜',
            '?'  => '？',
            '*'  => '＊',
            '\x00'..='\x1f' => '_',
            other => other,
        }).collect();
    }

    if truncate > 0 && name.len() > truncate {
        name = name[..truncate].to_string();
    }
    name.trim().to_string()
}

pub struct QobuzSpoofer;

impl QobuzSpoofer {
    pub async fn get_app_id_and_secrets(client: &reqwest::Client) -> MhResult<(String, Vec<String>)> {
        let login_page = client.get("https://play.qobuz.com/login")
            .send().await?
            .text().await?;

        let bundle_re = regex::Regex::new(r#"<script src="(/resources/[\d.]+-[a-z]\d+/bundle\.js)"></script>"#)
            .map_err(|e| MhError::Parse(e.to_string()))?;
        let bundle_path = bundle_re.captures(&login_page)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .ok_or_else(|| MhError::Parse("Qobuz: Could not find bundle.js URL".into()))?;

        let bundle_url = format!("https://play.qobuz.com{}", bundle_path);
        let bundle = client.get(&bundle_url).send().await?.text().await?;

        const APP_ID_PATTERNS: &[&str] = &[
            r#"production:\{api:\{appId:"(?P<app_id>\d{9})""#,
            r#"appId:"(?P<app_id>\d{9})""#,
            r#""app_id":"(?P<app_id>\d{9})""#,
            r#"appId\s*:\s*"(?P<app_id>\d{9})""#,
        ];
        let app_id = APP_ID_PATTERNS.iter()
            .find_map(|pat| {
                regex::Regex::new(pat).ok()
                    .and_then(|re| re.captures(&bundle))
                    .and_then(|c| c.name("app_id"))
                    .map(|m| m.as_str().to_string())
            })
            .ok_or_else(|| {
                let snippet: String = bundle.chars().take(200).collect();
                MhError::Parse(format!(
                    "Qobuz: Could not extract app_id from bundle. Bundle start: {}",
                    snippet
                ))
            })?;

        let seed_tz_re = regex::Regex::new(r#"[a-z]\.initialSeed\("(?P<seed>[\w=]+)",window\.utimezone\.(?P<tz>[a-z]+)\)"#)
            .map_err(|e| MhError::Parse(e.to_string()))?;

        let mut secrets: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        let mut key_order: Vec<String> = Vec::new();
        for cap in seed_tz_re.captures_iter(&bundle) {
            let seed = cap.name("seed").unwrap().as_str().to_string();
            let tz = cap.name("tz").unwrap().as_str().to_string();
            if !secrets.contains_key(&tz) {
                key_order.push(tz.clone());
            }
            secrets.entry(tz).or_default().push(seed);
        }

        if key_order.len() >= 2 {
            let mut reordered = key_order.clone();
            reordered.swap(0, 1);

            let tz_pat = reordered.iter()
                .map(|tz| {
                    let mut chars = tz.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                        None => tz.clone(),
                    }
                })
                .collect::<Vec<_>>()
                .join("|");

            let info_re_str = format!(
                r#"name:"\w+/(?P<tz>{tz_pat})",info:"(?P<info>[\w=]+)",extras:"(?P<extras>[\w=]+)""#,
                tz_pat = tz_pat
            );
            if let Ok(info_re) = regex::Regex::new(&info_re_str) {
                for cap in info_re.captures_iter(&bundle) {
                    let tz = cap.name("tz").unwrap().as_str().to_lowercase();
                    let info = cap.name("info").unwrap().as_str().to_string();
                    let extras = cap.name("extras").unwrap().as_str().to_string();
                    if let Some(parts) = secrets.get_mut(&tz) {
                        parts.push(info);
                        parts.push(extras);
                    }
                }
            }

            for parts in secrets.values_mut() {
                let combined = parts.join("");
                let trimmed = if combined.len() >= 44 {
                    &combined[..combined.len() - 44]
                } else {
                    &combined
                };
                use base64::Engine;
                *parts = vec![
                    base64::engine::general_purpose::STANDARD
                        .decode(trimmed)
                        .ok()
                        .and_then(|b| String::from_utf8(b).ok())
                        .unwrap_or_default()
                ];
            }
        }

        let secret_list: Vec<String> = key_order.iter()
            .filter_map(|k| secrets.get(k))
            .filter_map(|v| v.first().cloned())
            .filter(|s| !s.is_empty())
            .collect();

        Ok((app_id, secret_list))
    }
}

pub struct QobuzClient {
    pub app_id: String,
    pub secret: String,
    pub auth_token: String,
    pub client: reqwest::Client,
}

impl QobuzClient {
    pub async fn authenticate(settings: &Settings) -> MhResult<Self> {
        let email_or_id = settings.qobuz_email_or_userid.trim();
        let password_or_token = settings.qobuz_password_or_token.trim();
        let use_token = settings.qobuz_token_or_email;

        if email_or_id.is_empty() || password_or_token.is_empty() {
            return Err(MhError::Auth(
                "Qobuz credentials not set. Go to Settings → Qobuz and enter your email + password.".into()
            ));
        }

        let client = build_mozilla_client()?;
        let (app_id, secrets) = QobuzSpoofer::get_app_id_and_secrets(&client).await?;

        let mut params: Vec<(&str, String)> = vec![
            ("app_id", app_id.clone()),
        ];
        if use_token {
            params.push(("user_id", email_or_id.to_string()));
            params.push(("user_auth_token", password_or_token.to_string()));
        } else {
            params.push(("email", email_or_id.to_string()));
            params.push(("password", password_or_token.to_string()));
        }

        let resp = client.get(&format!("{}/user/login", BASE_URL))
            .header("X-App-Id", &app_id)
            .query(&params)
            .send().await?;

        match resp.status().as_u16() {
            401 => return Err(MhError::Auth("Qobuz: Invalid credentials".into())),
            400 => return Err(MhError::Auth("Qobuz: Invalid app_id".into())),
            _ => {}
        }

        let json: Value = resp.json().await?;
        if json["user"]["credential"]["parameters"].is_null() {
            return Err(MhError::Auth("Qobuz: Free accounts are not eligible to download tracks.".into()));
        }

        let auth_token = json["user_auth_token"]
            .as_str()
            .ok_or_else(|| MhError::Auth("Qobuz: No auth token in response".into()))?
            .to_string();

        let secret = Self::find_valid_secret(&client, &app_id, &auth_token, &secrets).await?;

        Ok(Self { app_id, secret, auth_token, client })
    }

    async fn find_valid_secret(
        client: &reqwest::Client,
        app_id: &str,
        auth_token: &str,
        secrets: &[String],
    ) -> MhResult<String> {
        for s in secrets {
            if Self::test_secret(client, app_id, auth_token, s).await.unwrap_or(false) {
                return Ok(s.clone());
            }
        }
        secrets.first().cloned().ok_or_else(|| MhError::Auth(
            "Qobuz: No app secrets available. Save your Qobuz credentials in Settings.".into()
        ))
    }

    async fn test_secret(client: &reqwest::Client, app_id: &str, auth_token: &str, secret: &str) -> MhResult<bool> {
        let unix_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let ts_str = unix_ts.to_string();
        let sig_str = format!("trackgetFileUrlformat_id27intentstreamtrack_id19512574{}{}", ts_str, secret);
        use md5::{Digest, Md5};
        let hash = format!("{:x}", Md5::digest(sig_str.as_bytes()));

        let params = [
            ("request_ts", ts_str.clone()),
            ("request_sig", hash),
            ("track_id", "19512574".to_string()),
            ("format_id", "27".to_string()),
            ("intent", "stream".to_string()),
            ("app_id", app_id.to_string()),
        ];

        let resp = client.get(&format!("{}/track/getFileUrl", BASE_URL))
            .header("X-App-Id", app_id)
            .header("X-User-Auth-Token", auth_token)
            .query(&params)
            .send().await?;

        Ok(resp.status().as_u16() != 400)
    }

    pub fn api_headers(&self) -> MhResult<HeaderMap> {
        crate::http_client::build_headers(&[
            ("X-App-Id", &self.app_id),
            ("X-User-Auth-Token", &self.auth_token),
        ])
    }

    async fn api_get(&self, endpoint: &str, params: &[(&str, &str)]) -> MhResult<Value> {
        let url = format!("{}/{}", BASE_URL, endpoint);
        let headers = self.api_headers()?;
        let resp = self.client.get(&url)
            .headers(headers)
            .query(params)
            .send().await?;
        let status = resp.status();
        let body = resp.text().await.map_err(MhError::Network)?;
        if !status.is_success() {
            let snippet: String = body.chars().take(200).collect();
            return Err(MhError::Other(format!(
                "Qobuz API error: HTTP {} — {}",
                status.as_u16(), snippet
            )));
        }
        serde_json::from_str(&body).map_err(|e| {
            let snippet: String = body.chars().take(200).collect();
            MhError::Parse(format!("Qobuz: unexpected response from {}: {} — body: {}", endpoint, e, snippet))
        })
    }

    async fn request_file_url_inner(&self, track_id: &str, format_id: u32) -> MhResult<Value> {
        let unix_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let ts_str = unix_ts.to_string();
        let sig_str = format!(
            "trackgetFileUrlformat_id{}intentstreamtrack_id{}{}{}",
            format_id, track_id, ts_str, self.secret
        );
        use md5::{Digest, Md5};
        let hash = format!("{:x}", Md5::digest(sig_str.as_bytes()));

        let format_str = format_id.to_string();
        self.api_get("track/getFileUrl", &[
            ("request_ts", ts_str.as_str()),
            ("request_sig", hash.as_str()),
            ("track_id", track_id),
            ("format_id", format_str.as_str()),
            ("intent", "stream"),
            ("app_id", self.app_id.as_str()),
        ]).await
    }

    pub async fn get_file_url(&self, track_id: &str, format_id: u8) -> MhResult<String> {
        let json = self.request_file_url_inner(track_id, quality_to_format_id(format_id)).await?;
        if let Some(url) = json["url"].as_str() {
            return Ok(url.to_string());
        }
        if let Some(restrictions) = json["restrictions"].as_array() {
            if let Some(first) = restrictions.first() {
                if let Some(code) = first["code"].as_str() {
                    let words = code.chars().enumerate().map(|(i, c)| {
                        if i > 0 && c.is_uppercase() { format!(" {}", c.to_lowercase()) }
                        else { c.to_lowercase().to_string() }
                    }).collect::<String>();
                    return Err(MhError::Other(format!("Qobuz: {}", words)));
                }
            }
        }
        Err(MhError::Other("Qobuz: Could not get download URL".into()))
    }

    pub async fn get_track_metadata(&self, track_id: &str) -> MhResult<Value> {
        self.api_get("track/get", &[
            ("track_id", track_id),
            ("app_id", self.app_id.as_str()),
        ]).await
    }

    pub async fn get_album_tracks(&self, album_id: &str) -> MhResult<AlbumInfo> {
        let json = self.api_get("album/get", &[
            ("album_id", album_id),
            ("app_id", self.app_id.as_str()),
        ]).await?;

        let items = json["tracks"]["items"].as_array().cloned().unwrap_or_default();
        Ok(AlbumInfo {
            track_ids: items.iter().filter_map(|t| t["id"].as_u64().map(|id| id.to_string())).collect(),
            track_disc_numbers: items.iter().map(|t| t["media_number"].as_u64().unwrap_or(1) as u32).collect(),
            number_of_volumes: json["media_count"].as_u64().unwrap_or(1) as u32,
            title: json["title"].as_str().unwrap_or(&format!("Album {}", album_id)).to_string(),
            artist: json["artist"]["name"].as_str().unwrap_or("").to_string(),
            thumbnail: json["image"]["large"].as_str()
                .or_else(|| json["image"]["small"].as_str())
                .unwrap_or("").to_string(),
            year: json["release_date_original"].as_str()
                .and_then(|d| d.split('-').next())
                .unwrap_or("").to_string(),
            genre: json["genre"]["name"].as_str().unwrap_or("").to_string(),
            label: json["label"]["name"].as_str().unwrap_or("").to_string(),
            bit_depth: json["maximum_bit_depth"].as_u64().map(|n| n as u32),
            sampling_rate: json["maximum_sampling_rate"].as_f64(),
        })
    }

    pub async fn get_playlist_tracks(&self, playlist_id: &str) -> MhResult<PlaylistInfo> {
        let json = self.api_get("playlist/get", &[
            ("playlist_id", playlist_id),
            ("extra", "tracks"),
            ("app_id", self.app_id.as_str()),
        ]).await?;

        let track_ids = json["tracks"]["items"].as_array()
            .map(|arr| arr.iter().filter_map(|t| t["id"].as_u64().map(|id| id.to_string())).collect())
            .unwrap_or_default();

        Ok(PlaylistInfo {
            track_ids,
            title: json["name"].as_str().unwrap_or(&format!("Playlist {}", playlist_id)).to_string(),
            artist: String::new(),
            thumbnail: String::new(),
        })
    }

    pub async fn get_artist_albums(&self, artist_id: &str, filters: &ArtistFilters) -> MhResult<Vec<String>> {
        let json = self.api_get("artist/get", &[
            ("artist_id", artist_id),
            ("extra", "albums"),
            ("limit", "500"),
            ("app_id", self.app_id.as_str()),
        ]).await?;

        let artist_name = json["name"].as_str().unwrap_or("").to_string();
        let mut albums: Vec<AlbumEntry> = json["albums"]["items"].as_array()
            .map(|arr| arr.iter().map(|a| AlbumEntry {
                id: a["id"].as_u64().map(|n| n.to_string()).unwrap_or_default(),
                title: a["title"].as_str().unwrap_or("").to_string(),
                albumartist: a["artist"]["name"].as_str().unwrap_or(&artist_name).to_string(),
                sampling_rate: a["maximum_sampling_rate"].as_f64().unwrap_or(0.0),
                bit_depth: a["maximum_bit_depth"].as_u64().unwrap_or(0) as u32,
                explicit: a["parental_warning"].as_bool().unwrap_or(false),
                nb_tracks: a["tracks_count"].as_u64().unwrap_or(0) as u32,
            }).collect())
            .unwrap_or_default();

        albums = self.apply_artist_filters(albums, &artist_name, filters);
        Ok(albums.into_iter().map(|a| a.id).collect())
    }

    fn apply_artist_filters(&self, mut albums: Vec<AlbumEntry>, artist_name: &str, filters: &ArtistFilters) -> Vec<AlbumEntry> {
        if filters.non_albums {
            albums.retain(|a| a.nb_tracks > 1);
        }
        if filters.extras {
            albums.retain(|a| !is_extra(&a.title));
        }
        if filters.features {
            albums.retain(|a| a.albumartist == artist_name);
        }
        if filters.non_studio_albums {
            albums.retain(|a| a.albumartist != "Various Artists" && !is_extra(&a.title));
        }
        if filters.non_remaster {
            albums.retain(|a| is_remaster(&a.title));
        }
        if filters.repeats {
            albums = filter_repeats(albums);
        }
        albums
    }

    pub async fn get_label_albums(&self, label_id: &str) -> MhResult<Vec<String>> {
        let json = self.api_get("label/get", &[
            ("label_id", label_id),
            ("extra", "albums"),
            ("limit", "500"),
            ("app_id", self.app_id.as_str()),
        ]).await?;

        Ok(json["albums"]["items"].as_array()
            .map(|arr| arr.iter().filter_map(|a| a["id"].as_u64().map(|id| id.to_string())).collect())
            .unwrap_or_default())
    }

    pub async fn download_booklet(&self, album_id: &str, dest_dir: &Path) -> MhResult<Option<PathBuf>> {
        let json = match self.api_get("album/get", &[
            ("album_id", album_id),
            ("app_id", self.app_id.as_str()),
        ]).await {
            Ok(j) => j,
            Err(_) => return Ok(None),
        };

        let goodies = json["goodies"].as_array().cloned().unwrap_or_default();
        let pdf = goodies.iter().find(|g| {
            g["file_format_id"].as_u64() == Some(21)
            || g["url"].as_str().map(|u| u.to_lowercase().ends_with(".pdf")).unwrap_or(false)
            || g["original_url"].as_str().map(|u| u.to_lowercase().ends_with(".pdf")).unwrap_or(false)
        });

        let pdf_url = match pdf {
            Some(g) => g["original_url"].as_str().or_else(|| g["url"].as_str()).map(|s| s.to_string()),
            None => return Ok(None),
        };

        let pdf_url = match pdf_url {
            Some(u) => u,
            None => return Ok(None),
        };

        let dest_path = dest_dir.join("booklet.pdf");
        let headers = self.api_headers()?;
        download_file(&self.client, &pdf_url, &dest_path, Some(&headers), |_, _| {}).await?;
        Ok(Some(dest_path))
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
        let primary_format_id = quality_to_format_id(quality);
        let fallback_format_ids: &[u32] = &[27, 7, 6, 5];
        let candidates: Vec<u32> = std::iter::once(primary_format_id)
            .chain(fallback_format_ids.iter().copied().filter(|&f| f != primary_format_id))
            .collect();

        let (json, format_id) = {
            let mut result = None;
            let mut last_err = MhError::Other("Qobuz: Could not get download URL".into());
            for &fid in &candidates {
                match self.request_file_url_inner(track_id, fid).await {
                    Ok(j) if j["url"].is_string() => {
                        result = Some((j, fid));
                        break;
                    }
                    Ok(j) => {
                        if let Some(restrictions) = j["restrictions"].as_array() {
                            if let Some(first) = restrictions.first() {
                                if let Some(code) = first["code"].as_str() {
                                    let words = code.chars().enumerate().map(|(i, c)| {
                                        if i > 0 && c.is_uppercase() { format!(" {}", c.to_lowercase()) }
                                        else { c.to_lowercase().to_string() }
                                    }).collect::<String>();
                                    last_err = MhError::Other(format!("Qobuz: {}", words));
                                }
                            }
                        }
                    }
                    Err(e) => { last_err = e; }
                }
            }
            match result {
                Some(r) => r,
                None => return Err(last_err),
            }
        };

        let stream_url = json["url"].as_str().unwrap().to_string();

        let ext = if format_id == 5 { "mp3" } else { "flac" };

        let meta = self.get_track_metadata(track_id).await?;
        let title = meta["title"].as_str().unwrap_or(&format!("track_{}", track_id)).to_string();
        let artist = meta["performer"]["name"].as_str()
            .or_else(|| meta["album"]["artist"]["name"].as_str())
            .unwrap_or("Unknown")
            .to_string();
        let albumartist = meta["album"]["artist"]["name"].as_str().unwrap_or(&artist).to_string();
        let album = meta["album"]["title"].as_str().unwrap_or("").to_string();
        let track_num = meta["track_number"].as_u64().unwrap_or(0) as u32;
        let disc_num = meta["media_number"].as_u64().unwrap_or(1) as u32;
        let tracktotal = meta["album"]["tracks_count"].as_u64().map(|n| n.to_string()).unwrap_or_default();
        let disctotal = meta["album"]["media_count"].as_u64().map(|n| n.to_string()).unwrap_or_default();
        let year = meta["album"]["release_date_original"].as_str()
            .and_then(|d| d.split('-').next())
            .unwrap_or("")
            .to_string();
        let genre = meta["album"]["genre"]["name"].as_str().unwrap_or("").to_string();

        let track_template = if settings.filepaths_track_format.is_empty() {
            "{tracknumber:02}. {artist} - {title}".to_string()
        } else {
            settings.filepaths_track_format.clone()
        };

        let restrict = settings.filepaths_restrict_characters;
        let truncate = settings.filepaths_truncate_to as usize;
        let explicit_str = if meta["parental_warning"].as_bool().unwrap_or(false) {
            " (Explicit)".to_string()
        } else {
            String::new()
        };

        let isrc = meta["isrc"].as_str().unwrap_or("").to_string();
        let label = meta["album"]["label"]["name"].as_str().unwrap_or("").to_string();
        let date = meta["album"]["release_date_original"].as_str().unwrap_or("").to_string();
        let actual_bit_depth = meta["maximum_bit_depth"].as_u64()
            .or_else(|| meta["album"]["maximum_bit_depth"].as_u64())
            .unwrap_or(16) as u32;
        let actual_sampling_rate = meta["maximum_sampling_rate"].as_f64()
            .or_else(|| meta["album"]["maximum_sampling_rate"].as_f64())
            .unwrap_or(44.1);
        let quality_str = crate::streamrip::qobuz_audio_quality_label(ext, format_id, actual_bit_depth, actual_sampling_rate);

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
        vars.insert("isrc", isrc.clone());
        vars.insert("label", label.clone());
        vars.insert("date", date.clone());
        vars.insert("quality", quality_str.clone());
        vars.insert("format", if ext == "flac" { "FLAC" } else { "MP3" }.to_string());

        let file_stem = build_file_name(&track_template, &vars.iter().map(|(k, v)| (*k, v.clone())).collect(), restrict, truncate);
        let file_name = format!("{}.{}", file_stem, ext);
        let track_dest = if !in_collection {
            let folder = crate::streamrip::build_album_folder(
                &settings.filepaths_folder_format,
                &albumartist,
                &album,
                &year,
                &genre,
                &label,
                quality_str.as_str(),
                if ext == "flac" { "FLAC" } else { "MP3" },
            );
            let d = dest.join(folder);
            tokio::fs::create_dir_all(&d).await?;
            d
        } else {
            dest.to_path_buf()
        };

        let mut dest_path = track_dest.join(&file_name);

        let cover_url = meta["album"]["image"]["large"].as_str()
            .map(|s| s.to_string());
        let embed_cover = settings.embed_cover;
        let save_cover = settings.save_cover;

        let cover_tmp = if let (Some(ref url), true) = (&cover_url, embed_cover || save_cover) {
            download_cover_art(&self.client, url).await.ok()
        } else {
            None
        };

        let headers = self.api_headers()?;
        download_file(&self.client, &stream_url, &dest_path, Some(&headers), on_progress).await?;

        if save_cover {
            if let Some(ref tmp) = cover_tmp {
                let cover_dest = track_dest.join("cover.jpg");
                let _ = tokio::fs::copy(tmp.path(), &cover_dest).await;
            }
        }

        let perf_str = meta["performers"].as_str().unwrap_or("");
        let perf_map = parse_performers(perf_str);
        let get_role = |role: &str| -> Option<String> {
            perf_map.get(&role.to_lowercase()).map(|names| names.join(", "))
        };

        let metadata = TrackMetadata {
            title: Some(title),
            artist: Some(artist),
            album: Some(album),
            album_artist: Some(albumartist),
            year: meta["album"]["release_date_original"].as_str().map(|s| s.to_string()),
            genre: Some(genre),
            track_number: Some(track_num),
            disc_number: Some(disc_num),
            total_tracks: if tracktotal.is_empty() { None } else { tracktotal.parse().ok() },
            total_discs: if disctotal.is_empty() { None } else { disctotal.parse().ok() },
            isrc: meta["isrc"].as_str().map(|s| s.to_string()),
            upc: meta["album"]["upc"].as_str().map(|s| s.to_string()),
            copyright: meta["copyright"].as_str().map(|s| s.to_string()),
            label: meta["album"]["label"]["name"].as_str().map(|s| s.to_string()),
            composer: meta["composer"]["name"].as_str().map(|s| s.to_string())
                .or_else(|| get_role("composer")),
            conductor: None,
            performer: None,
            producer: get_role("producer"),
            lyricist: get_role("lyricist"),
            engineer: get_role("engineer"),
            mixer: get_role("mixer"),
            description: meta["description"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string()),
            purchase_date: meta["purchasable_at"].as_i64().map(|ts| {
                use chrono::TimeZone;
                chrono::Utc.timestamp_opt(ts, 0)
                    .single()
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_default()
            }).filter(|s| !s.is_empty()),
            grouping: meta["album"]["genre"]["name"].as_str().filter(|s| !s.is_empty()).map(|s| s.to_string()),
            comment: None,
            lyrics: None,
        };

        let cover_path = if embed_cover { cover_tmp.as_ref().map(|t| t.path()) } else { None };

        let exclude_tags: Vec<String> = if settings.meta_exclude_tags_check && !settings.excluded_tags.is_empty() {
            settings.excluded_tags.split(',').map(|s| s.trim().to_string()).collect()
        } else {
            vec![]
        };

        let _ = tag_file(
            &dest_path,
            &metadata,
            cover_path,
            &exclude_tags,
            "ffmpeg",
        ).await;

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
                tracing::warn!("Qobuz conversion failed: {}", e);
            } else if new_dest != dest_path {
                let _ = tokio::fs::remove_file(&dest_path).await;
                dest_path = new_dest;
            }
        }

        Ok(dest_path)
    }
}

fn parse_performers(s: &str) -> std::collections::HashMap<String, Vec<String>> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    if s.is_empty() {
        return map;
    }
    for part in s.split(|c| c == '\n' || c == ',') {
        let part = part.trim();
        if let Some(dash_pos) = part.find(" - ") {
            let role = part[..dash_pos].trim().to_lowercase();
            let name = part[dash_pos + 3..].trim().to_string();
            map.entry(role).or_default().push(name);
        }
    }
    map
}

fn is_extra(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("anniversary") || lower.contains("deluxe") || lower.contains("live")
        || lower.contains("collector") || lower.contains("demo") || lower.contains("expanded")
        || lower.contains("remix")
}

fn is_remaster(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("remaster") || lower.contains("remastered")
}

fn title_essence(title: &str) -> String {
    let re = regex::Regex::new(r"^([^(\[]+)").unwrap();
    re.find(title).map(|m| m.as_str().trim().to_lowercase()).unwrap_or_else(|| title.to_lowercase())
}

fn filter_repeats(albums: Vec<AlbumEntry>) -> Vec<AlbumEntry> {
    let mut groups: std::collections::HashMap<String, Vec<AlbumEntry>> = std::collections::HashMap::new();
    for a in albums {
        let key = title_essence(&a.title);
        groups.entry(key).or_default().push(a);
    }
    let mut result = Vec::new();
    for (_, mut group) in groups {
        group.sort_by(|x, y| {
            if x.explicit != y.explicit {
                return y.explicit.cmp(&x.explicit);
            }
            y.sampling_rate.partial_cmp(&x.sampling_rate).unwrap_or(std::cmp::Ordering::Equal)
                .then(y.bit_depth.cmp(&x.bit_depth))
        });
        result.push(group.remove(0));
    }
    result
}

#[derive(Debug, Clone)]
struct AlbumEntry {
    id: String,
    title: String,
    albumartist: String,
    sampling_rate: f64,
    bit_depth: u32,
    explicit: bool,
    nb_tracks: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ArtistFilters {
    pub extras: bool,
    pub repeats: bool,
    pub non_albums: bool,
    pub features: bool,
    pub non_studio_albums: bool,
    pub non_remaster: bool,
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
    pub bit_depth: Option<u32>,
    pub sampling_rate: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct PlaylistInfo {
    pub track_ids: Vec<String>,
    pub title: String,
    pub artist: String,
    pub thumbnail: String,
}

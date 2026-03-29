use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde_json::Value;

use crate::errors::{MhError, MhResult};
use crate::http_client::build_mozilla_client;
use crate::defaults::Settings;
use crate::streamrip::tagger::{TrackMetadata, tag_file, download_cover_art};
use crate::streamrip::downloader::download_and_decrypt_deezer;

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
}

fn quality_info(q: u8) -> (&'static str, &'static str) {
    match q {
        0 => ("MP3_128", "mp3"),
        1 => ("MP3_320", "mp3"),
        2 => ("FLAC", "flac"),
        _ => ("MP3_128", "mp3"),
    }
}

fn build_file_name(template: &str, vars: &std::collections::HashMap<&str, String>, restrict: bool, truncate: usize) -> String {
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

fn safe_name(s: &str) -> String {
    s.chars().map(|c| match c {
        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
        other => other,
    }).collect()
}

impl DeezerClient {
    pub fn new(arl: &str) -> MhResult<Self> {
        let client = build_mozilla_client()?;
        Ok(Self {
            arl: arl.trim().to_string(),
            client,
            session: Arc::new(RwLock::new(None)),
        })
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
        let user_id = resp["results"]["USER"]["USER_ID"]
            .as_u64()
            .unwrap_or(0);
        if user_id == 0 {
            return Err(MhError::Auth(
                "Deezer ARL is invalid or expired. Update it in Settings → Deezer → ARL Token.".into()
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

        let mut session = self.session.write().await;
        *session = Some(DeezerSession {
            token,
            user_id: user_id.to_string(),
            license_token,
            max_quality,
            sid: captured_sid,   // properly capture sid from Set-Cookie
        });
        Ok(())
    }

    async fn require_session(&self) -> MhResult<tokio::sync::RwLockReadGuard<'_, Option<DeezerSession>>> {
        let guard = self.session.read().await;
        if guard.is_none() {
            return Err(MhError::Auth("DeezerClient: not authenticated. Call authenticate() first.".into()));
        }
        Ok(guard)
    }

    async fn gw_get(&self, method: &str, session_snapshot: Option<(&str, Option<&str>)>) -> MhResult<(Value, Option<String>)> {
        let (api_token, sid) = match session_snapshot {
            Some((tok, sid)) => (tok.to_string(), sid.map(|s| s.to_string())),
            None => ("null".to_string(), None),
        };
        let url = format!(
            "{}?method={}&input=3&api_version=1.0&api_token={}",
            GW_BASE, method, api_token
        );
        let cookie = self.cookie(sid.as_deref());
        let resp = self.client
            .get(&url)
            .header("Cookie", &cookie)
            .header("User-Agent", crate::http_client::UA_MOZILLA)
            .send()
            .await?;

        let new_sid: Option<String> = resp.headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(|c| {
                c.split(';').next()
                    .and_then(|part| {
                        let mut kv = part.splitn(2, '=');
                        let k = kv.next()?.trim();
                        let v = kv.next()?.trim();
                        if k == "sid" { Some(v.to_string()) } else { None }
                    })
            });

        let body = resp.text().await?;
        let json: Value = serde_json::from_str(&body)?;

        Ok((json, new_sid))
    }

    async fn gw_post_inner(&self, method: &str, body: Value, api_token: &str, sid: Option<&str>) -> MhResult<Value> {
        let url = format!(
            "{}?method={}&input=3&api_version=1.0&api_token={}",
            GW_BASE, method, api_token
        );
        let cookie = self.cookie(sid);
        let body_str = serde_json::to_string(&body)?;
        let resp = self.client
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

        let page_resp = self.gw_post_inner(
            "deezer.pageTrack",
            serde_json::json!({ "SNG_ID": track_id }),
            &token,
            sid.as_deref(),
        ).await;

        if let Ok(resp) = page_resp {
            if let Some(data) = resp.get("results").and_then(|r| r.get("DATA")) {
                let mut track_info = data.clone();
                if let Some(lyrics) = resp["results"].get("LYRICS") {
                    track_info["LYRICS"] = lyrics.clone();
                }
                return Ok(track_info);
            }
        }

        let resp = self.gw_post_inner(
            "song.getData",
            serde_json::json!({ "SNG_ID": track_id }),
            &token,
            sid.as_deref(),
        ).await?;

        resp.get("results")
            .cloned()
            .ok_or_else(|| MhError::NotFound(format!("Failed to get track info for {}", track_id)))
    }

    pub async fn get_public_track(&self, track_id: &str) -> MhResult<Value> {
        let resp = self.client
            .get(&format!("{}/track/{}", PUBLIC_API, track_id))
            .send().await?;
        if !resp.status().is_success() {
            return Err(MhError::Other(format!("Deezer public API error: {}", resp.status().as_u16())));
        }
        let mut track: Value = resp.json().await?;
        if let Ok(contrib_resp) = self.client
            .get(&format!("{}/track/{}/contributors", PUBLIC_API, track_id))
            .send().await
        {
            if contrib_resp.status().is_success() {
                if let Ok(contribs) = contrib_resp.json::<Value>().await {
                    track["contributors"] = contribs;
                }
            }
        }
        Ok(track)
    }

    pub async fn get_public_album(&self, album_id: &str) -> MhResult<Value> {
        let resp = self.client
            .get(&format!("{}/album/{}", PUBLIC_API, album_id))
            .send().await?;
        if !resp.status().is_success() {
            return Err(MhError::Other(format!("Deezer public API error: {}", resp.status().as_u16())));
        }
        Ok(resp.json().await?)
    }

    pub async fn get_public_playlist(&self, playlist_id: &str) -> MhResult<Value> {
        let resp = self.client
            .get(&format!("{}/playlist/{}", PUBLIC_API, playlist_id))
            .send().await?;
        if !resp.status().is_success() {
            return Err(MhError::Other(format!("Deezer public API error: {}", resp.status().as_u16())));
        }
        Ok(resp.json().await?)
    }

    pub async fn get_album_tracks(&self, album_id: &str) -> MhResult<AlbumInfo> {
        let album = self.get_public_album(album_id).await?;
        let tracks_resp = self.client
            .get(&format!("{}/album/{}/tracks?limit=500", PUBLIC_API, album_id))
            .send().await?;
        let tracks: Value = tracks_resp.json().await?;
        let items = tracks["data"].as_array().cloned().unwrap_or_default();
        Ok(AlbumInfo {
            track_ids: items.iter().filter_map(|t| t["id"].as_u64().map(|id| id.to_string())).collect(),
            track_disc_numbers: items.iter().map(|t| t["disk_number"].as_u64().unwrap_or(1) as u32).collect(),
            number_of_volumes: album["nb_disk"].as_u64().unwrap_or(1) as u32,
            title: album["title"].as_str().unwrap_or(&format!("Album {}", album_id)).to_string(),
            artist: album["artist"]["name"].as_str().unwrap_or("").to_string(),
            thumbnail: album["cover_medium"].as_str().unwrap_or("").to_string(),
            year: album["release_date"].as_str().and_then(|d| d.split('-').next()).unwrap_or("").to_string(),
            genre: album["genres"]["data"][0]["name"].as_str().unwrap_or("").to_string(),
            label: album["label"].as_str().unwrap_or("").to_string(),
        })
    }

    pub async fn get_playlist_tracks(&self, playlist_id: &str) -> MhResult<PlaylistInfo> {
        let meta = self.get_public_playlist(playlist_id).await?;
        let track_ids = meta["tracks"]["data"].as_array()
            .map(|arr| arr.iter().filter_map(|t| t["id"].as_u64().map(|id| id.to_string())).collect())
            .unwrap_or_default();
        Ok(PlaylistInfo {
            track_ids,
            title: meta["title"].as_str().unwrap_or(&format!("Playlist {}", playlist_id)).to_string(),
            artist: meta["creator"]["name"].as_str().unwrap_or("").to_string(),
            thumbnail: meta["picture_medium"].as_str().unwrap_or("").to_string(),
        })
    }

    pub async fn get_artist_albums(&self, artist_id: &str) -> MhResult<Vec<String>> {
        let resp = self.client
            .get(&format!("{}/artist/{}/albums?limit=500", PUBLIC_API, artist_id))
            .send().await?;
        if !resp.status().is_success() {
            return Err(MhError::NotFound(format!("Deezer: artist {} not found", artist_id)));
        }
        let json: Value = resp.json().await?;
        let ids = json["data"].as_array()
            .map(|arr| arr.iter().filter_map(|a| a["id"].as_u64().map(|id| id.to_string())).collect())
            .unwrap_or_default();
        Ok(ids)
    }

    pub fn get_label_albums(&self, _label_id: &str) -> MhResult<Vec<String>> {
        Err(MhError::Unsupported(
            "Deezer does not support label downloads via its public API. Use Qobuz for label downloads.".into()
        ))
    }

    async fn get_token_url(&self, track_token: &str, format: &str, license_token: &str, sid: Option<&str>) -> MhResult<Option<String>> {
        let body = serde_json::json!({
            "license_token": license_token,
            "media": [{ "type": "FULL", "formats": [{ "cipher": "BF_CBC_STRIPE", "format": format }] }],
            "track_tokens": [track_token],
        });
        let url = format!("{}/get_url", MEDIA_BASE);
        let cookie = self.cookie(sid);
        let resp = self.client
            .post(&url)
            .header("Cookie", &cookie)
            .header("User-Agent", crate::http_client::UA_MOZILLA)
            .header("Content-Type", "application/json")
            .json(&body)
            .send().await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let json: Value = resp.json().await?;
        Ok(json["data"][0]["media"][0]["sources"][0]["url"]
            .as_str()
            .map(|s| s.to_string()))
    }

    pub async fn get_stream_url(&self, track_id: &str, quality: u8) -> MhResult<(String, &'static str)> {
        let guard = self.require_session().await?;
        let sess = guard.as_ref().unwrap();
        let effective_quality = quality.min(sess.max_quality);
        let license_token = sess.license_token.clone();
        let sid = sess.sid.clone();
        drop(guard);

        let track_info = self.get_track_info(track_id).await?;
        let (format, ext) = quality_info(effective_quality);

        let mut url: Option<String> = None;
        if let Some(token) = track_info["TRACK_TOKEN"].as_str() {
            if !token.is_empty() {
                url = self.get_token_url(token, format, &license_token, sid.as_deref()).await.ok().flatten();
            }
        }

        if url.is_none() {
            if let Some(token) = track_info["FALLBACK"]["TRACK_TOKEN"].as_str() {
                if !token.is_empty() {
                    url = self.get_token_url(token, format, &license_token, sid.as_deref()).await.ok().flatten();
                }
            }
        }

        if url.is_none() {
            let md5 = track_info["MD5_ORIGIN"].as_str()
                .or_else(|| track_info["FALLBACK"]["MD5_ORIGIN"].as_str());
            let md5 = md5.ok_or_else(|| MhError::Other(format!(
                "Deezer: Track {} is not available for streaming. It may be region-locked or unavailable on your subscription tier.",
                track_id
            )))?;
            let media_version = track_info["MEDIA_VERSION"].as_str()
                .or_else(|| track_info["FALLBACK"]["MEDIA_VERSION"].as_str())
                .unwrap_or("1");
            let effective_id = track_info["FALLBACK"]["SNG_ID"].as_str().unwrap_or(track_id);
            url = Some(crate::crypto::deezer::get_encrypted_url(
                effective_id, md5, media_version, effective_quality,
            ));
        }

        Ok((url.unwrap(), ext))
    }

    async fn get_lyrics(&self, track_id: &str) -> MhResult<Value> {
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
        let resp = self.client
            .get(&url)
            .header("Cookie", &cookie)
            .header("User-Agent", crate::http_client::UA_MOZILLA)
            .send().await?;
        let text = resp.text().await?;
        Ok(serde_json::from_str(&text)?)
    }

    pub async fn download_track(
        &self,
        track_id: &str,
        dest: &Path,
        settings: &Settings,
        on_progress: impl Fn(u64, u64),
        in_collection: bool,
    ) -> MhResult<PathBuf> {
        let _guard = self.require_session().await?;
        drop(_guard);

        let quality = {
            let guard = self.session.read().await;
            let sess = guard.as_ref().unwrap();
            let preferred = match settings.deezer_quality.to_uppercase().as_str() {
                "FLAC" => 2u8,
                "MP3_320" | "320" => 1u8,
                "MP3_128" | "128" => 0u8,
                _ => 2u8,
            };
            preferred.min(sess.max_quality)
        };

        let (stream_url, ext) = self.get_stream_url(track_id, quality).await?;

        let public_meta = self.get_public_track(track_id).await.ok();
        let album_meta: Option<Value> = if let Some(ref pm) = public_meta {
            if let Some(album_id) = pm["album"]["id"].as_u64() {
                self.get_public_album(&album_id.to_string()).await.ok()
            } else {
                None
            }
        } else {
            None
        };

        let title = public_meta.as_ref()
            .and_then(|m| m["title"].as_str())
            .unwrap_or(&format!("track_{}", track_id))
            .to_string();
        let artist = public_meta.as_ref()
            .and_then(|m| m["artist"]["name"].as_str())
            .unwrap_or("Unknown")
            .to_string();
        let albumartist = album_meta.as_ref()
            .and_then(|m| m["artist"]["name"].as_str())
            .unwrap_or(&artist)
            .to_string();
        let album = public_meta.as_ref()
            .and_then(|m| m["album"]["title"].as_str())
            .unwrap_or("")
            .to_string();
        let track_num = public_meta.as_ref()
            .and_then(|m| m["track_position"].as_u64())
            .unwrap_or(0) as u32;
        let disc_num = public_meta.as_ref()
            .and_then(|m| m["disk_number"].as_u64())
            .unwrap_or(1) as u32;
        let tracktotal = album_meta.as_ref()
            .and_then(|m| m["nb_tracks"].as_u64())
            .map(|n| n.to_string())
            .unwrap_or_default();
        let year = public_meta.as_ref()
            .and_then(|m| m["release_date"].as_str())
            .and_then(|d| d.split('-').next())
            .unwrap_or("")
            .to_string();
        let genre = album_meta.as_ref()
            .and_then(|m| m["genres"]["data"][0]["name"].as_str())
            .unwrap_or("")
            .to_string();

        let track_template = if settings.filepaths_track_format.is_empty() {
            "{tracknumber:02}. {artist} - {title}".to_string()
        } else {
            settings.filepaths_track_format.clone()
        };

        let restrict = settings.filepaths_restrict_characters;
        let truncate = settings.filepaths_truncate_to as usize;

        let explicit_str = if public_meta.as_ref()
            .and_then(|m| m["explicit_lyrics"].as_bool())
            .unwrap_or(false)
        {
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
        vars.insert("year", year.clone());
        vars.insert("genre", genre.clone());
        vars.insert("explicit", explicit_str);

        let file_stem = build_file_name(&track_template, &vars.iter().map(|(k, v)| (*k, v.clone())).collect(), restrict, truncate);
        let file_stem = if !restrict { safe_name(&file_stem) } else { file_stem };
        let file_name = format!("{}.{}", file_stem, ext);

        let label = album_meta.as_ref().and_then(|m| m["label"].as_str()).unwrap_or("").to_string();
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

        let (lyrics_text, lrc_content): (Option<String>, Option<String>) = match self.get_lyrics(track_id).await {
            Ok(lyr) => {
                let ldata = &lyr["results"];
                let has_error = lyr["error"].as_array().map(|a| !a.is_empty()).unwrap_or(false);
                if !has_error {
                    let text = ldata["LYRICS_TEXT"].as_str().map(|s| s.to_string());
                    let lrc = ldata["LYRICS_SYNC_JSON"].as_array().and_then(|arr| {
                        if arr.is_empty() {
                            None
                        } else {
                            let lines: Vec<String> = arr.iter()
                                .filter_map(|l| {
                                    let ts = l["lrc_timestamp"].as_str()?;
                                    let line = l["line"].as_str().unwrap_or("");
                                    Some(format!("{}{}", ts, line))
                                })
                                .collect();
                            if lines.is_empty() { None } else { Some(lines.join("\n")) }
                        }
                    });
                    (text, lrc)
                } else {
                    (None, None)
                }
            }
            Err(_) => (None, None),
        };

        let cover_url = public_meta.as_ref()
            .and_then(|m| m["album"]["cover_xl"].as_str()
                .or_else(|| m["album"]["cover_medium"].as_str()))
            .or_else(|| album_meta.as_ref().and_then(|m| m["cover_xl"].as_str()))
            .map(|s| s.to_string());

        let embed_cover = settings.embed_cover;
        let save_cover = settings.save_cover;

        let cover_tmp = if let (Some(ref url), true) = (&cover_url, embed_cover || save_cover) {
            download_cover_art(&self.client, url).await.ok()
        } else {
            None
        };

        download_and_decrypt_deezer(&self.client, &stream_url, track_id, &dest_path, on_progress).await?;

        if save_cover {
            if let Some(ref tmp) = cover_tmp {
                let cover_dest = track_dest.join("cover.jpg");
                let _ = tokio::fs::copy(tmp.path(), &cover_dest).await;
            }
        }

        let contribs = public_meta.as_ref()
            .and_then(|m| m["contributors"]["data"].as_array())
            .cloned()
            .unwrap_or_default();
        let by_role = |role: &str| -> Option<String> {
            let names: Vec<&str> = contribs.iter()
                .filter(|c| c["role"].as_str() == Some(role))
                .filter_map(|c| c["name"].as_str())
                .collect();
            if names.is_empty() { None } else { Some(names.join(", ")) }
        };

        let metadata = TrackMetadata {
            title: Some(title),
            artist: Some(artist),
            album: Some(album),
            album_artist: Some(albumartist),
            year: public_meta.as_ref()
                .and_then(|m| m["release_date"].as_str())
                .map(|s| s.to_string()),
            genre: Some(genre),
            track_number: Some(track_num),
            disc_number: Some(disc_num),
            total_tracks: if tracktotal.is_empty() { None } else { tracktotal.parse().ok() },
            total_discs: None,
            isrc: public_meta.as_ref().and_then(|m| m["isrc"].as_str()).map(|s| s.to_string()),
            upc: album_meta.as_ref().and_then(|m| m["upc"].as_str()).map(|s| s.to_string()),
            copyright: album_meta.as_ref().and_then(|m| m["copyright"].as_str()).map(|s| s.to_string()),
            label: album_meta.as_ref().and_then(|m| m["label"].as_str()).map(|s| s.to_string()),
            composer: by_role("Composer"),
            conductor: None,
            performer: None,
            producer: by_role("Producer"),
            lyricist: by_role("Lyricist"),
            engineer: by_role("Engineer"),
            mixer: None,
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
                tracing::warn!("Deezer conversion failed: {}", e);
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

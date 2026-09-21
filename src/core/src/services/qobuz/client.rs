use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::defaults::Settings;
use crate::downloads::dedup::quality_rank;
use crate::errors::{MhError, MhResult};
use crate::http_client::build_mozilla_client;
use crate::services::common::ids::now_secs;
use crate::services::common::library::string_at;
use crate::services::common::lyrics::{resolve_track_lyrics, FoundLyrics};
use crate::services::common::pipeline::converter::ConversionSource;
use crate::services::common::pipeline::downloader::{
    count_of, cover_tmp_for, download_file, existing_final_file, finalize_track,
    is_transient_http_error, preflight, resolve_track_dest, retry_transient, save_cover_file,
    CommonTrackFields, CoverFiles, FinalizeTrack,
};
use crate::services::common::pipeline::orchestrator::TrackJob;
use crate::services::common::pipeline::tagger::{one, TrackMetadata};
use crate::services::common::pipeline::{playlist_rows, AlbumInfo, PlaylistInfo, TrackOutcome};
use reqwest::header::HeaderMap;

const BASE_URL: &str = "https://www.qobuz.com/api.json/0.2";

use crate::services::qobuz::app_credentials;
use qobuz_settings_to_format_id as quality_to_format_id;

/// Tiers to try, best first, never above what was asked for.
fn qobuz_downgrade_ladder(primary: u32) -> Vec<u32> {
    [27u32, 7, 6, 5]
        .into_iter()
        .filter(|&f| f <= primary)
        .collect()
}

/// Qobuz cover URLs end in `_<edge>.jpg`, with `_max` for the original. JPEG only.
fn resize_qobuz_cover(url: &str, size: u32) -> String {
    static SUFFIX_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"_(?:\d+|max)\.jpg$").unwrap());
    let suffix = if size > 600 {
        "_max.jpg".to_string()
    } else {
        let snapped = crate::services::common::pipeline::nearest_cover_size(size, &[50, 230, 600]);
        format!("_{snapped}.jpg")
    };
    SUFFIX_RE.replace(url, suffix.as_str()).into_owned()
}

fn qobuz_restriction_message(code: &str) -> String {
    let words = code
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if i > 0 && c.is_uppercase() {
                format!(" {}", c.to_lowercase())
            } else {
                c.to_lowercase().to_string()
            }
        })
        .collect::<String>();
    format!("Qobuz: {}", words)
}

#[derive(Clone)]
pub struct QobuzClient {
    pub app_id: String,
    pub secret: String,
    pub auth_token: String,
    pub client: reqwest::Client,
    pub download_client: reqwest::Client,
}

impl QobuzClient {
    /// Resolves the app credentials this token needs, then holds them.
    ///
    /// A Qobuz token only works with the app_id that minted it, and the signing
    /// secret only works with that same app_id — see
    /// [`app_credentials::resolve`] for how both are recovered from the token's
    /// own mint date.
    pub async fn authenticate(settings: &Settings) -> MhResult<Self> {
        Self::authenticate_verbose(settings).await.0
    }

    /// As [`Self::authenticate`], but hands back the resolution trail so the
    /// caller can put it in front of the user. The trail comes back on failure
    /// too — that is when it is worth reading.
    pub async fn authenticate_verbose(settings: &Settings) -> (MhResult<Self>, Vec<String>) {
        let client = match build_mozilla_client() {
            Ok(c) => c,
            Err(e) => return (Err(e), Vec::new()),
        };
        let download_client = match crate::http_client::build_audio_client() {
            Ok(c) => c,
            Err(e) => return (Err(e), Vec::new()),
        };
        let (outcome, trail) = app_credentials::resolve(&client, settings).await;
        let built = outcome.map(|pair| Self {
            app_id: pair.app_id,
            secret: pair.secret,
            auth_token: settings.qobuz_password_or_token.trim().to_string(),
            client,
            download_client,
        });
        (built, trail)
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
        let resp = self
            .client
            .get(&url)
            .headers(headers)
            .query(params)
            .send()
            .await?;
        crate::services::common::http::read_json(&format!("Qobuz {endpoint}"), resp).await
    }

    async fn request_file_url_inner(&self, track_id: &str, format_id: u32) -> MhResult<Value> {
        let unix_ts = now_secs();

        let ts_str = unix_ts.to_string();
        let format_str = format_id.to_string();
        let hash = crate::services::qobuz::app_credentials::file_url_signature(
            track_id,
            &format_str,
            &ts_str,
            &self.secret,
        );
        self.api_get(
            "track/getFileUrl",
            &[
                ("request_ts", ts_str.as_str()),
                ("request_sig", hash.as_str()),
                ("track_id", track_id),
                ("format_id", format_str.as_str()),
                ("intent", "stream"),
                ("app_id", self.app_id.as_str()),
            ],
        )
        .await
    }

    async fn request_file_url_with_retry(&self, track_id: &str, format_id: u32) -> MhResult<Value> {
        retry_transient(3, is_transient_http_error, || {
            self.request_file_url_inner(track_id, format_id)
        })
        .await
    }

    pub async fn get_file_url(&self, track_id: &str, format_id: u8) -> MhResult<String> {
        let json = self
            .request_file_url_with_retry(track_id, quality_to_format_id(format_id))
            .await?;
        if let Some(url) = json["url"].as_str() {
            return Ok(url.to_string());
        }
        if let Some(restrictions) = json["restrictions"].as_array() {
            if let Some(first) = restrictions.first() {
                if let Some(code) = first["code"].as_str() {
                    return Err(MhError::Other(qobuz_restriction_message(code)));
                }
            }
        }
        Err(MhError::Other("Qobuz: Could not get download URL".into()))
    }

    pub async fn get_track_metadata(&self, track_id: &str) -> MhResult<Value> {
        self.api_get(
            "track/get",
            &[("track_id", track_id), ("app_id", self.app_id.as_str())],
        )
        .await
    }

    pub async fn get_album_tracks(&self, album_id: &str) -> MhResult<AlbumInfo> {
        let json = self
            .api_get(
                "album/get",
                &[("album_id", album_id), ("app_id", self.app_id.as_str())],
            )
            .await?;

        let items = json["tracks"]["items"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let rows = crate::services::common::pipeline::release_rows(
            &items,
            "/performer/name",
            Some("media_number"),
        );

        Ok(AlbumInfo {
            tracks: rows,
            album_record: json.clone(),
            number_of_volumes: json["media_count"].as_u64().unwrap_or(1) as u32,
            title: json["title"]
                .as_str()
                .unwrap_or(&format!("Album {}", album_id))
                .to_string(),
            artist: string_at(&json, &["/artist/name"]),
            year: json["release_date_original"]
                .as_str()
                .and_then(|d| d.split('-').next())
                .unwrap_or("")
                .to_string(),
            genre: string_at(&json, &["/genre/name"]),
            label: string_at(&json, &["/label/name"]),
            bit_depth: json["maximum_bit_depth"].as_u64().map(|n| n as u32),
            sampling_rate: json["maximum_sampling_rate"].as_f64(),
        })
    }

    pub async fn get_playlist_tracks(&self, playlist_id: &str) -> MhResult<PlaylistInfo> {
        let json = self
            .api_get(
                "playlist/get",
                &[
                    ("playlist_id", playlist_id),
                    ("extra", "tracks"),
                    ("app_id", self.app_id.as_str()),
                ],
            )
            .await?;

        let rows = playlist_rows(&json["tracks"]["items"], "/performer/name");

        Ok(PlaylistInfo {
            tracks: rows,
            title: json["name"]
                .as_str()
                .unwrap_or(&format!("Playlist {}", playlist_id))
                .to_string(),
            artist: string_at(&json, &["/owner/name"]),
        })
    }

    pub async fn get_artist_albums(
        &self,
        artist_id: &str,
        filters: &ArtistFilters,
    ) -> MhResult<Vec<String>> {
        let json = self
            .api_get(
                "artist/get",
                &[
                    ("artist_id", artist_id),
                    ("extra", "albums"),
                    ("limit", "500"),
                    ("app_id", self.app_id.as_str()),
                ],
            )
            .await?;

        let artist_name = string_at(&json, &["/name"]);
        let mut albums: Vec<AlbumEntry> = json["albums"]["items"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|a| AlbumEntry {
                        id: a["id"].as_u64().map(|n| n.to_string()).unwrap_or_default(),
                        title: string_at(a, &["/title"]),
                        albumartist: a["artist"]["name"]
                            .as_str()
                            .unwrap_or(&artist_name)
                            .to_string(),
                        sampling_rate: a["maximum_sampling_rate"].as_f64().unwrap_or(0.0),
                        bit_depth: a["maximum_bit_depth"].as_u64().unwrap_or(0) as u32,
                        explicit: a["parental_warning"].as_bool().unwrap_or(false),
                        nb_tracks: a["tracks_count"].as_u64().unwrap_or(0) as u32,
                    })
                    .collect()
            })
            .unwrap_or_default();

        albums = self.apply_artist_filters(albums, &artist_name, filters);
        Ok(albums.into_iter().map(|a| a.id).collect())
    }

    fn apply_artist_filters(
        &self,
        mut albums: Vec<AlbumEntry>,
        artist_name: &str,
        filters: &ArtistFilters,
    ) -> Vec<AlbumEntry> {
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
        let json = self
            .api_get(
                "label/get",
                &[
                    ("label_id", label_id),
                    ("extra", "albums"),
                    ("limit", "500"),
                    ("app_id", self.app_id.as_str()),
                ],
            )
            .await?;

        Ok(json["albums"]["items"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a["id"].as_u64().map(|id| id.to_string()))
                    .collect()
            })
            .unwrap_or_default())
    }

    pub async fn download_booklet(
        &self,
        album_id: &str,
        dest_dir: &Path,
    ) -> MhResult<Option<PathBuf>> {
        let json = match self
            .api_get(
                "album/get",
                &[("album_id", album_id), ("app_id", self.app_id.as_str())],
            )
            .await
        {
            Ok(j) => j,
            Err(_) => return Ok(None),
        };

        let goodies = json["goodies"].as_array().cloned().unwrap_or_default();
        let pdf = goodies.iter().find(|g| {
            g["file_format_id"].as_u64() == Some(21)
                || g["url"]
                    .as_str()
                    .map(|u| u.to_lowercase().ends_with(".pdf"))
                    .unwrap_or(false)
                || g["original_url"]
                    .as_str()
                    .map(|u| u.to_lowercase().ends_with(".pdf"))
                    .unwrap_or(false)
        });

        let pdf_url = match pdf {
            Some(g) => g["original_url"]
                .as_str()
                .or_else(|| g["url"].as_str())
                .map(|s| s.to_string()),
            None => return Ok(None),
        };

        let pdf_url = match pdf_url {
            Some(u) => u,
            None => return Ok(None),
        };

        let dest_path = dest_dir.join("booklet.pdf");
        let headers = self.api_headers()?;
        download_file(
            &self.client,
            &pdf_url,
            &dest_path,
            Some(&headers),
            |_, _| {},
        )
        .await?;
        Ok(Some(dest_path))
    }

    /// The tier Qobuz actually grants for this release, walked down the same ladder a
    /// real download uses so the folder cannot claim a resolution the account is not
    /// entitled to.
    pub async fn probe_served_quality(
        &self,
        track_id: &str,
        quality: u8,
    ) -> MhResult<(String, String)> {
        let primary = quality_to_format_id(quality);
        for fid in qobuz_downgrade_ladder(primary) {
            let Ok(json) = self.request_file_url_with_retry(track_id, fid).await else {
                continue;
            };
            if !json["url"].is_string() {
                continue;
            }
            let ext = if fid == 5 { "mp3" } else { "flac" };
            let bd = json["bit_depth"].as_u64().unwrap_or(0) as u32;
            let sr = json["sampling_rate"].as_f64().unwrap_or(0.0);
            let label = qobuz_audio_quality_label(
                ext,
                fid,
                if bd == 0 { 24 } else { bd },
                if sr == 0.0 { 192.0 } else { sr },
            );
            let format = if ext == "mp3" { "MP3" } else { "FLAC" };
            return Ok((label, format.to_string()));
        }
        Err(MhError::Other(format!(
            "Qobuz: no streamable format for track {track_id}"
        )))
    }

    pub async fn download_track(&self, job: TrackJob<'_>) -> MhResult<TrackOutcome> {
        let rank = quality_rank("qobuz", &job.quality.to_string());
        if let Some(out) = preflight(&job, "qobuz", "Qobuz", &["streamable"], rank)? {
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

        let primary_format_id = quality_to_format_id(quality);
        let candidates: Vec<u32> = qobuz_downgrade_ladder(primary_format_id);

        let (json, format_id) = {
            let mut result = None;
            let mut last_err = MhError::Other("Qobuz: Could not get download URL".into());
            for &fid in &candidates {
                match self.request_file_url_with_retry(track_id, fid).await {
                    Ok(j) if j["url"].is_string() => {
                        result = Some((j, fid));
                        break;
                    }
                    Ok(j) => {
                        if let Some(restrictions) = j["restrictions"].as_array() {
                            if let Some(first) = restrictions.first() {
                                if let Some(code) = first["code"].as_str() {
                                    last_err = MhError::Other(qobuz_restriction_message(code));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        last_err = e;
                    }
                }
            }
            match result {
                Some(r) => r,
                None => return Err(last_err),
            }
        };

        let stream_url = json["url"].as_str().unwrap().to_string();

        if format_id != primary_format_id {
            on_log(format!(
                "  ⚠ Qobuz {} unavailable for this track; served {} instead",
                qobuz_quality_label(primary_format_id),
                qobuz_quality_label(format_id)
            ));
        }

        let ext = if format_id == 5 { "mp3" } else { "flac" };

        let meta = match record.filter(|r| !r.is_null()) {
            Some(r) => {
                let mut merged = r.clone();
                if let Some(alb) = release.filter(|a| !a.is_null()) {
                    merged["album"] = alb.clone();
                }
                merged
            }
            None => self.get_track_metadata(track_id).await?,
        };
        let title = meta["title"]
            .as_str()
            .unwrap_or(&format!("track_{}", track_id))
            .to_string();
        let artist = meta["performer"]["name"]
            .as_str()
            .or_else(|| meta["album"]["artist"]["name"].as_str())
            .unwrap_or("Unknown")
            .to_string();
        let albumartist = meta["album"]["artist"]["name"]
            .as_str()
            .unwrap_or(&artist)
            .to_string();
        let album = string_at(&meta, &["/album/title"]);
        let track_num = meta["track_number"].as_u64().unwrap_or(0) as u32;
        let disc_num = meta["media_number"].as_u64().unwrap_or(1) as u32;
        let tracktotal = meta["album"]["tracks_count"]
            .as_u64()
            .map(|n| n.to_string())
            .unwrap_or_default();
        let disctotal = meta["album"]["media_count"]
            .as_u64()
            .map(|n| n.to_string())
            .unwrap_or_default();
        let year = meta["album"]["release_date_original"]
            .as_str()
            .and_then(|d| d.split('-').next())
            .unwrap_or("")
            .to_string();
        let genre = meta["album"]["genre"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let explicit_str = if meta["parental_warning"].as_bool().unwrap_or(false) {
            " (Explicit)".to_string()
        } else {
            String::new()
        };

        let isrc = string_at(&meta, &["/isrc"]);
        let label = meta["album"]["label"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let date = meta["album"]["release_date_original"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let actual_bit_depth = json["bit_depth"]
            .as_u64()
            .or_else(|| meta["maximum_bit_depth"].as_u64())
            .or_else(|| meta["album"]["maximum_bit_depth"].as_u64())
            .unwrap_or(16) as u32;
        let actual_sampling_rate = json["sampling_rate"]
            .as_f64()
            .or_else(|| meta["maximum_sampling_rate"].as_f64())
            .or_else(|| meta["album"]["maximum_sampling_rate"].as_f64())
            .unwrap_or(44.1);
        let quality_str =
            qobuz_audio_quality_label(ext, format_id, actual_bit_depth, actual_sampling_rate);

        let format_label = if ext == "flac" { "FLAC" } else { "MP3" };
        let composer_name = string_at(&meta, &["/composer/name"]);
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
            quality_label: quality_str.clone(),
            format_label: format_label.to_string(),
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

        let found = resolve_track_lyrics(
            FoundLyrics::default(),
            &fields.title,
            &fields.artist,
            meta["duration"].as_f64(),
            settings,
            on_log,
        )
        .await;

        let cover_url = meta["album"]["image"]["large"]
            .as_str()
            .map(|s| resize_qobuz_cover(s, settings.pipeline_cover_size));
        let embed_cover = settings.embed_cover;
        let cover_files = CoverFiles::from_settings(settings);
        let cover_tmp = cover_tmp_for(&self.client, cover_url.as_deref(), settings, covers).await;

        let headers = self.api_headers()?;
        download_file(
            &self.download_client,
            &stream_url,
            &dest_path,
            Some(&headers),
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

        let perf_str = meta["performers"].as_str().unwrap_or("");
        let perf_map = parse_performers(perf_str);
        let get_role = |role: &str| -> Vec<String> {
            perf_map
                .get(&role.to_lowercase())
                .cloned()
                .unwrap_or_default()
        };
        let gain = |scope: &Value, key: &str| -> Option<String> {
            scope["audio_info"][key]
                .as_f64()
                .map(|v| format!("{v:+.2} dB"))
        };
        let peak = |scope: &Value, key: &str| -> Option<String> {
            scope["audio_info"][key].as_f64().map(|v| format!("{v:.6}"))
        };

        let metadata = TrackMetadata {
            title: Some(title),
            artist: {
                let billed = performer_names(meta["performers"].as_str().unwrap_or(""));
                if billed.is_empty() {
                    one(Some(artist.clone()))
                } else {
                    billed
                }
            },
            album: Some(album),
            album_artist: one(Some(albumartist)),
            year: meta["album"]["release_date_original"]
                .as_str()
                .map(|s| s.to_string()),
            original_date: meta["album"]["release_date_original"]
                .as_str()
                .map(|s| s.to_string()),
            genre: one(Some(genre)),
            track_number: Some(track_num),
            disc_number: Some(disc_num),
            total_tracks: count_of(&tracktotal),
            total_discs: count_of(&disctotal),
            isrc: meta["isrc"].as_str().map(|s| s.to_string()),
            upc: meta["album"]["upc"].as_str().map(|s| s.to_string()),
            copyright: meta["copyright"].as_str().map(|s| s.to_string()),
            label: meta["album"]["label"]["name"]
                .as_str()
                .map(|s| s.to_string()),
            composer: meta["composer"]["name"]
                .as_str()
                .map(|s| vec![s.to_string()])
                .unwrap_or_else(|| get_role("composer")),
            conductor: get_role("conductor"),
            performer: get_role("performer"),
            producer: get_role("producer"),
            lyricist: get_role("lyricist"),
            engineer: get_role("engineer"),
            mixer: get_role("mixer"),
            bpm: None,
            replaygain_track_gain: gain(&meta, "replaygain_track_gain"),
            replaygain_track_peak: peak(&meta, "replaygain_track_peak"),
            replaygain_album_gain: None,
            replaygain_album_peak: None,
            description: meta["description"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
            purchase_date: meta["purchasable_at"]
                .as_i64()
                .map(|ts| {
                    use chrono::TimeZone;
                    chrono::Utc
                        .timestamp_opt(ts, 0)
                        .single()
                        .map(|dt| dt.format("%Y-%m-%d").to_string())
                        .unwrap_or_default()
                })
                .filter(|s| !s.is_empty()),
            grouping: None,
            comment: None,
            lyrics: settings.embed_lyrics.then(|| found.plain.clone()).flatten(),
        };

        finalize_track(FinalizeTrack {
            dest: dest_path,
            platform: "qobuz",
            track_id,
            rank,
            served_rank: quality_rank("qobuz", &format_id.to_string()),
            source: ConversionSource::from_ext(ext),
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
}

/// Qobuz ships credits as one delimited string, not a structured array:
/// `Name, Role, Role - Name, Role - …`. Entries are separated by ` - `, and
/// inside an entry the first comma-separated token is the person and the rest
/// are their roles.
///
/// Splitting on commas first — as this used to — shreds every entry, and reading
/// the text before a dash as the role inverts the format: it picks up the
/// *previous* person's last role and pairs it with the *next* person's name.
fn parse_performers(s: &str) -> std::collections::HashMap<String, Vec<String>> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    for entry in s.split(" - ") {
        let mut fields = entry.split(',').map(str::trim).filter(|f| !f.is_empty());
        let Some(name) = fields.next() else { continue };
        for role in fields {
            for canonical in canonical_roles(role) {
                let names = map.entry(canonical.to_string()).or_default();
                if !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                    names.push(name.to_string());
                }
            }
        }
    }
    map
}

/// Qobuz's role vocabulary mapped onto the tag fields. One credit can land in
/// two places — `ComposerLyricist` is exactly what it says.
///
/// An unrecognised role is kept under its own lowercased name rather than
/// dropped, so `get_role` can still find it and nothing vanishes silently.
fn canonical_roles(role: &str) -> Vec<&'static str> {
    let key: String = role
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    match key.as_str() {
        "composerlyricist" => vec!["composer", "lyricist"],
        "composer" | "writer" => vec!["composer"],
        "lyricist" | "author" => vec!["lyricist"],
        "producer"
        | "additionalproducer"
        | "coproducer"
        | "vocalproducer"
        | "additionalstudioproducer" => vec!["producer"],
        "mixingengineer" | "mixer" | "remixer" => vec!["mixer"],
        "masteringengineer"
        | "recordingengineer"
        | "engineer"
        | "studiopersonnel"
        | "additionalengineer"
        | "asstrecordingengineer" => vec!["engineer"],
        "conductor" => vec!["conductor"],
        "mainartist"
        | "featuredartist"
        | "associatedperformer"
        | "performer"
        | "additionalvocalist"
        | "backgroundvocalist" => vec!["performer"],
        _ => vec![],
    }
}

/// The performing credits, in the order Qobuz lists them, so a featured artist
/// is not silently dropped from `ARTIST`.
fn performer_names(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in s.split(" - ") {
        let mut fields = entry.split(',').map(str::trim).filter(|f| !f.is_empty());
        let Some(name) = fields.next() else { continue };
        let billed = fields.any(|role| {
            let key: String = role
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            key == "mainartist" || key == "featuredartist"
        });
        if billed && !out.iter().any(|n: &String| n.eq_ignore_ascii_case(name)) {
            out.push(name.to_string());
        }
    }
    out
}

fn is_extra(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("anniversary")
        || lower.contains("deluxe")
        || lower.contains("live")
        || lower.contains("collector")
        || lower.contains("demo")
        || lower.contains("expanded")
        || lower.contains("remix")
}

fn is_remaster(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("remaster") || lower.contains("remastered")
}

fn title_essence(title: &str) -> String {
    let head = title.split(['(', '[']).next().unwrap_or(title).trim();
    if head.is_empty() {
        title.to_lowercase()
    } else {
        head.to_lowercase()
    }
}

fn filter_repeats(albums: Vec<AlbumEntry>) -> Vec<AlbumEntry> {
    let mut groups: std::collections::HashMap<String, Vec<AlbumEntry>> =
        std::collections::HashMap::new();
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
            y.sampling_rate
                .partial_cmp(&x.sampling_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
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

#[cfg(test)]
mod tests {
    use super::qobuz_downgrade_ladder;

    #[test]
    fn the_ladder_only_ever_walks_down() {
        assert_eq!(qobuz_downgrade_ladder(27), vec![27, 7, 6, 5]);
        assert_eq!(qobuz_downgrade_ladder(7), vec![7, 6, 5]);
        assert_eq!(qobuz_downgrade_ladder(6), vec![6, 5]);
        assert_eq!(qobuz_downgrade_ladder(5), vec![5]);
    }
}

#[cfg(test)]
mod performer_tests {
    use super::{parse_performers, performer_names};

    /// Shaped like a captured `track/get` response. Entries are separated by
    /// ` - ` and each begins with the person, not the role.
    const REAL: &str = "Daft Punk, MainArtist, Producer - Nile Rodgers, \
                        Producer, ComposerLyricist - Pharrell Williams, ComposerLyricist";

    #[test]
    fn credits_attach_to_the_person_they_belong_to() {
        let map = parse_performers(REAL);
        assert_eq!(
            map.get("producer").map(Vec::as_slice),
            Some(["Daft Punk".to_string(), "Nile Rodgers".to_string()].as_slice())
        );
    }

    /// `ComposerLyricist` is one role that fills two tags, and it is why COMPOSER
    /// came out empty: the key was stored as "composerlyricist" and never matched.
    #[test]
    fn a_composer_lyricist_credit_fills_both_fields() {
        let map = parse_performers(REAL);
        let expected = ["Nile Rodgers".to_string(), "Pharrell Williams".to_string()];
        assert_eq!(
            map.get("composer").map(Vec::as_slice),
            Some(expected.as_slice())
        );
        assert_eq!(
            map.get("lyricist").map(Vec::as_slice),
            Some(expected.as_slice())
        );
    }

    /// The featured artist is in this string and nowhere else — `performer.name`
    /// carries the main billing only.
    #[test]
    fn featured_artists_are_billed_alongside_the_main_one() {
        let real =
            "Daft Punk, MainArtist - Julian Casablancas, FeaturedArtist, AdditionalVocalist - \
                    Peter Franco, Producer";
        assert_eq!(
            performer_names(real),
            vec!["Daft Punk".to_string(), "Julian Casablancas".to_string()]
        );
        assert!(!performer_names(real).contains(&"Peter Franco".to_string()));
    }

    #[test]
    fn the_same_person_is_not_credited_twice_for_one_role() {
        let map = parse_performers("Nile Rodgers, Producer - NILE RODGERS, Producer");
        assert_eq!(map.get("producer").map(Vec::len), Some(1));
    }

    #[test]
    fn an_empty_or_roleless_string_yields_nothing() {
        assert!(parse_performers("").is_empty());
        assert!(parse_performers("Just A Name").is_empty());
        assert!(performer_names("").is_empty());
    }
}

#[cfg(test)]
mod cover_tests {
    use super::resize_qobuz_cover;

    const LARGE: &str = "https://static.qobuz.com/images/covers/ab/cd/0123456789abcd_600.jpg";

    #[test]
    fn the_size_suffix_follows_the_setting() {
        assert_eq!(
            resize_qobuz_cover(LARGE, 1280),
            "https://static.qobuz.com/images/covers/ab/cd/0123456789abcd_max.jpg"
        );
        assert_eq!(
            resize_qobuz_cover(LARGE, 640),
            "https://static.qobuz.com/images/covers/ab/cd/0123456789abcd_max.jpg"
        );
        assert_eq!(
            resize_qobuz_cover(LARGE, 600),
            "https://static.qobuz.com/images/covers/ab/cd/0123456789abcd_600.jpg"
        );
        assert_eq!(
            resize_qobuz_cover(LARGE, 320),
            "https://static.qobuz.com/images/covers/ab/cd/0123456789abcd_230.jpg"
        );
    }

    #[test]
    fn an_already_max_url_can_still_be_narrowed() {
        let max = "https://static.qobuz.com/images/covers/ab/cd/0123456789abcd_max.jpg";
        assert_eq!(
            resize_qobuz_cover(max, 320),
            "https://static.qobuz.com/images/covers/ab/cd/0123456789abcd_230.jpg"
        );
    }
}

use std::sync::Arc;

use crate::downloads::dedup::DedupLedger;
use crate::services::common::pipeline::orchestrator::{
    download_collection, extract_or_err, extract_platform_id, ContentType, Platform, SharedItem,
    SharedLog, SharedProgress, SharedQuality, TrackSourceClient,
};

#[async_trait::async_trait]
impl TrackSourceClient for QobuzClient {
    fn platform_name(&self) -> &'static str {
        "Qobuz"
    }

    fn requested_quality(&self, settings: &Settings) -> u8 {
        settings.qobuz_quality
    }

    fn extract_id(&self, url: &str, content_type: ContentType) -> Option<String> {
        extract_platform_id(url, Platform::Qobuz, content_type)
    }

    fn album_url(&self, album_id: &str) -> String {
        format!("https://play.qobuz.com/album/{}", album_id)
    }

    fn album_quality_format(&self, album: &AlbumInfo, settings: &Settings) -> (String, String) {
        let quality = settings.qobuz_quality;
        let fid = qobuz_settings_to_format_id(quality);
        let album_quality = {
            let bd = album.bit_depth.unwrap_or(16);
            let sr = album.sampling_rate.unwrap_or(44.1);
            let ext = if fid == 5 { "mp3" } else { "flac" };
            qobuz_audio_quality_label(ext, fid, bd, sr)
        };
        let qobuz_format = if fid == 5 { "MP3" } else { "FLAC" };
        (album_quality, qobuz_format.to_string())
    }

    fn playlist_quality_format(&self, settings: &Settings) -> (String, String) {
        let fid = qobuz_settings_to_format_id(settings.qobuz_quality);
        let qobuz_format = if fid == 5 { "MP3" } else { "FLAC" };
        (
            qobuz_quality_label(fid).to_string(),
            qobuz_format.to_string(),
        )
    }

    async fn probe_release_quality(
        &self,
        first_track_id: &str,
        quality: u8,
        _settings: &Settings,
    ) -> Option<(String, String)> {
        QobuzClient::probe_served_quality(self, first_track_id, quality)
            .await
            .ok()
    }

    async fn get_album_tracks(&self, album_id: &str) -> MhResult<AlbumInfo> {
        QobuzClient::get_album_tracks(self, album_id).await
    }

    async fn get_playlist_tracks(&self, playlist_id: &str) -> MhResult<PlaylistInfo> {
        QobuzClient::get_playlist_tracks(self, playlist_id).await
    }

    async fn get_artist_albums(
        &self,
        artist_id: &str,
        settings: &Settings,
    ) -> MhResult<Vec<String>> {
        let filters = qobuz_filters_from_settings(settings);
        QobuzClient::get_artist_albums(self, artist_id, &filters).await
    }

    async fn download_track(&self, job: TrackJob<'_>) -> MhResult<TrackOutcome> {
        QobuzClient::download_track(self, job).await
    }

    async fn after_album(&self, album_id: &str, dest_dir: &Path, settings: &Settings) {
        if settings.qobuz_download_booklets {
            let _ = self.download_booklet(album_id, dest_dir).await;
        }
    }

    async fn download_label(
        &self,
        url: &str,
        base_dir: &Path,
        settings: &Settings,
        on_progress: SharedProgress,
        on_log: SharedLog,
        on_item: SharedItem,
        on_quality: SharedQuality,
        bytes: Arc<crate::downloads::ByteProgress>,
        dedup: Option<&DedupLedger>,
    ) -> MhResult<()> {
        let id = extract_or_err(self, url, ContentType::Label)?;
        let album_ids = self.get_label_albums(&id).await?;
        on_log(format!("Label: {} albums", album_ids.len()));
        for album_id in &album_ids {
            let album_url = self.album_url(album_id);
            let _ = Box::pin(download_collection(
                self,
                &album_url,
                ContentType::Album,
                base_dir,
                settings,
                on_progress.clone(),
                on_log.clone(),
                on_item.clone(),
                on_quality.clone(),
                bytes.clone(),
                dedup,
            ))
            .await;
        }
        Ok(())
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
            "Qobuz does not support video downloads.".into(),
        ))
    }
}

/// Fallback when actual audio metadata is unavailable — based on requested format_id only.
pub fn qobuz_quality_label(format_id: u32) -> &'static str {
    match format_id {
        5 => "320kbps",
        6 => "16-bit ⁄ 44.1kHz",
        7 => "24-bit ⁄ 96kHz",
        27 => "24-bit ⁄ 192kHz",
        _ => "",
    }
}

/// Converts settings quality u8 to Qobuz format_id u32.
pub fn qobuz_settings_to_format_id(quality: u8) -> u32 {
    match quality {
        5 | 6 | 7 | 27 => quality as u32,
        1 => 5,
        2 => 6,
        3 => 7,
        _ => 27,
    }
}

/// Accurate quality spec (no format prefix): caps at user's requested format_id, then takes min with API-reported max.
pub fn qobuz_audio_quality_label(
    ext: &str,
    format_id: u32,
    api_max_bit_depth: u32,
    api_max_sampling_rate: f64,
) -> String {
    if ext != "flac" {
        return "320kbps".to_string();
    }
    let (req_bd, req_sr): (u32, f64) = match format_id {
        6 => (16, 44.1),
        7 => (24, 96.0),
        27 => (24, 192.0),
        _ => (16, 44.1),
    };
    let eff_bd = req_bd.min(api_max_bit_depth);
    let eff_sr = req_sr.min(api_max_sampling_rate);
    let sr_str = if eff_sr.fract() == 0.0 {
        format!("{}kHz", eff_sr as u32)
    } else {
        format!("{:.1}kHz", eff_sr)
    };
    format!("{}-bit ⁄ {}", eff_bd, sr_str)
}

#[cfg(test)]
mod quality_label_tests {
    use super::*;

    #[test]
    fn qobuz_downgrade_only_fallback() {
        assert_eq!(qobuz_settings_to_format_id(1), 5);
        assert_eq!(qobuz_settings_to_format_id(2), 6);
        assert_eq!(qobuz_settings_to_format_id(3), 7);
        assert_eq!(qobuz_settings_to_format_id(4), 27);
    }
}

pub(crate) fn qobuz_filters_from_settings(settings: &Settings) -> ArtistFilters {
    ArtistFilters {
        extras: settings.qobuz_filters_extras,
        repeats: settings.qobuz_repeats,
        non_albums: settings.qobuz_non_albums,
        features: settings.qobuz_features,
        non_studio_albums: settings.qobuz_non_studio_albums,
        non_remaster: settings.qobuz_non_remaster,
    }
}

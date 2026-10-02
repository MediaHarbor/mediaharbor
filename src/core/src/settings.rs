use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde_json::Value;
use tokio::fs;

use crate::{defaults::Settings, errors::MhResult};

/// gamdl keys that match MediaHarbor's `apple_*` setting names one-for-one.
#[rustfmt::skip]
const GAMDL_PASSTHROUGH: &[&str] = &[
    "cookies_path", "output_path", "temp_path", "download_mode", "cover_format",
    "cover_size", "save_cover", "synced_lyrics_format", "synced_lyrics_only",
    "no_synced_lyrics", "date_tag_template", "save_playlist", "overwrite", "language",
    "truncate", "exclude_tags", "log_level", "use_album_date", "artist_auto_select",
    "playlist_folder_template", "no_exceptions", "uploaded_video_quality", "nm3u8dlre_path",
    "ffmpeg_path", "wvd_path", "use_wrapper", "wrapper_url", "wrapper_decrypt_host",
    "wrapper_decrypt_port",
];

/// gamdl keys MediaHarbor spells differently.
const GAMDL_RENAMES: &[(&str, &str)] = &[
    ("template_folder_album", "album_folder_template"),
    ("template_folder_compilation", "compilation_folder_template"),
    ("template_file_single_disc", "single_disc_file_template"),
    ("template_file_multi_disc", "multi_disc_file_template"),
    ("template_folder_no_album", "no_album_folder_template"),
    ("template_file_no_album", "no_album_file_template"),
    ("template_file_playlist", "playlist_file_template"),
    ("mv_codec_priority", "music_video_codec_priority"),
    ("mv_remux_format", "music_video_remux_format"),
    ("mv_resolution", "music_video_resolution"),
];

/// votify keys that match MediaHarbor's `spotify_*` setting names one-for-one.
#[rustfmt::skip]
const VOTIFY_PASSTHROUGH: &[&str] = &[
    "cookies_path", "audio_quality", "audio_download_mode", "audio_remux_mode",
    "video_format", "video_resolution", "video_remux_mode", "cover_size", "wvd_path",
    "no_drm", "wait_interval", "overwrite", "no_synced_lyrics_file", "save_playlist_file",
    "save_cover_file", "synced_lyrics_only", "album_folder_template",
    "compilation_folder_template", "podcast_folder_template", "no_album_folder_template",
    "single_disc_file_template", "multi_disc_file_template", "podcast_file_template",
    "no_album_file_template", "playlist_file_template", "date_tag_template", "truncate",
    "exclude_tags", "log_level", "no_exceptions", "artist_media_option", "prefer_video",
    "session_type",
];

/// votify keys MediaHarbor spells differently.
const VOTIFY_RENAMES: &[(&str, &str)] =
    &[("output_path", "output"), ("dll_path", "spotify_dll_path")];

/// These maps are **whitelists**: `save_service_config` only writes keys present
/// here, so a setting the downloader does not understand never reaches its INI.
fn key_map(
    passthrough: &[&'static str],
    renames: &[(&'static str, &'static str)],
) -> HashMap<&'static str, &'static str> {
    passthrough
        .iter()
        .map(|k| (*k, *k))
        .chain(renames.iter().copied())
        .collect()
}

pub fn apple_to_gamdl_key_map() -> HashMap<&'static str, &'static str> {
    key_map(GAMDL_PASSTHROUGH, GAMDL_RENAMES)
}

pub fn spotify_to_votify_key_map() -> HashMap<&'static str, &'static str> {
    key_map(VOTIFY_PASSTHROUGH, VOTIFY_RENAMES)
}

pub const CURRENT_SCHEMA_VERSION: u32 = 5;

/// Flat settings key -> its dotted location in the nested on-disk file. The
/// single source of truth for both directions; `every_flat_key_is_in_grouping_table`
/// fails the build if a new setting is missing here (it would be lost on load).
#[rustfmt::skip]
pub fn settings_grouping_table() -> &'static [(&'static str, &'static str)] {
    &[
    ("autoUpdate",                          "general.autoUpdate"),
    ("autoDownloadUpdates",                 "general.autoDownloadUpdates"),
    ("theme",                               "general.theme"),
    ("downloadLocation",                    "general.downloadLocation"),
    ("createPlatformSubfolders",            "general.createPlatformSubfolders"),
    ("orpheusDL",                           "general.orpheusEnabled"),
    ("enabledServices",                     "general.enabledServices"),
    ("radioDirectorySources",               "general.radioDirectorySources"),
    ("onboarding_completed",                "general.onboardingCompleted"),
    ("replaygain_mode",                     "player.replayGainMode"),
    ("crossfade_enabled",                   "player.crossfadeEnabled"),
    ("crossfade_duration",                  "player.crossfadeDuration"),
    ("use_cookies",                         "general.useCookies"),
    ("cookies",                             "general.cookies"),
    ("cookies_from_browser",                "general.cookiesFromBrowser"),
    ("use_aria2",                           "tools.ytdlp.useAria2"),
    ("max_downloads",                       "tools.ytdlp.maxDownloads"),
    ("download_speed_limit",                "tools.ytdlp.downloadSpeedLimit"),
    ("speed_limit_type",                    "tools.ytdlp.speedLimitType"),
    ("speed_limit_value",                   "tools.ytdlp.speedLimitValue"),
    ("max_retries",                         "tools.ytdlp.maxRetries"),
    ("download_output_template",            "tools.ytdlp.downloadOutputTemplate"),
    ("continue",                            "tools.ytdlp.continue"),
    ("add_metadata",                        "tools.ytdlp.addMetadata"),
    ("embed_chapters",                      "tools.ytdlp.embedChapters"),
    ("add_subtitle_to_file",                "tools.ytdlp.addSubtitleToFile"),
    ("use_proxy",                           "tools.ytdlp.useProxy"),
    ("proxy_url",                           "tools.ytdlp.proxyUrl"),
    ("use_authentication",                  "tools.ytdlp.useAuthentication"),
    ("username",                            "tools.ytdlp.username"),
    ("password",                            "tools.ytdlp.password"),
    ("sponsorblock_mark",                   "tools.ytdlp.sponsorblockMark"),
    ("sponsorblock_remove",                 "tools.ytdlp.sponsorblockRemove"),
    ("sponsorblock_chapter_title",          "tools.ytdlp.sponsorblockChapterTitle"),
    ("no_sponsorblock",                     "tools.ytdlp.noSponsorblock"),
    ("sponsorblock_api_url",                "tools.ytdlp.sponsorblockApiUrl"),
    ("concurrent_fragments",                "tools.ytdlp.concurrentFragments"),
    ("remux_format",                        "tools.ytdlp.remuxFormat"),
    ("convert_thumbnails_jpg",              "tools.ytdlp.convertThumbnailsJpg"),
    ("sub_langs",                           "tools.ytdlp.subLangs"),
    ("write_auto_subs",                     "tools.ytdlp.writeAutoSubs"),
    ("convert_subs_srt",                    "tools.ytdlp.convertSubsSrt"),
    ("fragment_retries",                    "tools.ytdlp.fragmentRetries"),
    ("extractor_retries",                   "tools.ytdlp.extractorRetries"),
    ("file_access_retries",                 "tools.ytdlp.fileAccessRetries"),
    ("socket_timeout",                      "tools.ytdlp.socketTimeout"),
    ("throttled_rate",                      "tools.ytdlp.throttledRate"),
    ("sleep_requests",                      "tools.ytdlp.sleepRequests"),
    ("sleep_interval",                      "tools.ytdlp.sleepInterval"),
    ("max_sleep_interval",                  "tools.ytdlp.maxSleepInterval"),
    ("aria2c_args",                         "tools.ytdlp.aria2cArgs"),
    ("postprocessor_args",                  "tools.ytdlp.postprocessorArgs"),
    ("faststart_mp4",                       "tools.ytdlp.faststartMp4"),
    ("trim_filenames",                      "tools.ytdlp.trimFilenames"),
    ("restrict_filenames",                  "tools.ytdlp.restrictFilenames"),
    ("windows_filenames",                   "tools.ytdlp.windowsFilenames"),
    ("no_overwrites",                       "tools.ytdlp.noOverwrites"),
    ("geo_bypass_country",                  "tools.ytdlp.geoBypassCountry"),
    ("set_mtime",                           "tools.ytdlp.setMtime"),
    ("extra_args",                          "tools.ytdlp.extraArgs"),
    ("disc_subdirectories",                 "general.pipeline.discSubdirectories"),
    ("save_playlist_file",                  "general.pipeline.savePlaylistFile"),
    ("conversion_check",                    "general.pipeline.conversionCheck"),
    ("conversion_codec",                    "general.pipeline.conversionCodec"),
    ("conversion_sampling_rate",            "general.pipeline.conversionSamplingRate"),
    ("conversion_bit_depth",                "general.pipeline.conversionBitDepth"),
    ("conversion_lossy_bitrate",            "general.pipeline.conversionLossyBitrate"),
    ("meta_exclude_tags_check",             "general.pipeline.metaExcludeTagsCheck"),
    ("excluded_tags",                       "general.pipeline.excludedTags"),
    ("filepaths_folder_format",             "general.pipeline.filepathsFolderFormat"),
    ("filepaths_track_format",              "general.pipeline.filepathsTrackFormat"),
    ("filepaths_restrict_characters",       "general.pipeline.filepathsRestrictCharacters"),
    ("filepaths_truncate_to",               "general.pipeline.filepathsTruncateTo"),
    ("embed_cover",                         "general.pipeline.embedCover"),
    ("save_album_cover",                    "general.pipeline.saveAlbumCover"),
    ("save_cover",                          "general.pipeline.saveCover"),
    ("pipeline_cover_size",                 "general.pipeline.coverSize"),
    ("pipeline_overwrite",                  "general.pipeline.overwrite"),
    ("save_lrc_files",                      "general.pipeline.saveLrcFiles"),
    ("lyrics_fallback_lrclib",              "general.pipeline.lyricsFallbackLrclib"),
    ("native_synced_lyrics_format",         "general.pipeline.nativeSyncedLyricsFormat"),
    ("embed_lyrics",                        "general.pipeline.embedLyrics"),
    ("native_skip_existing",                "general.pipeline.nativeSkipExisting"),
    ("spotify_client_id",                   "services.spotify.clientId"),
    ("spotify_client_secret",               "services.spotify.clientSecret"),
    ("spotify_cookies_path",                "services.spotify.cookiesPath"),
    ("spotify_output_path",                 "services.spotify.outputPath"),
    ("spotify_audio_quality",               "services.spotify.audioQuality"),
    ("spotify_audio_download_mode",         "services.spotify.audioDownloadMode"),
    ("spotify_audio_remux_mode",            "services.spotify.audioRemuxMode"),
    ("spotify_video_format",                "services.spotify.videoFormat"),
    ("spotify_video_resolution",            "services.spotify.videoResolution"),
    ("spotify_video_remux_mode",            "services.spotify.videoRemuxMode"),
    ("spotify_cover_size",                  "services.spotify.coverSize"),
    ("spotify_wvd_path",                    "services.spotify.wvdPath"),
    ("spotify_no_drm",                      "services.spotify.noDrm"),
    ("spotify_wait_interval",               "services.spotify.waitInterval"),
    ("spotify_overwrite",                   "services.spotify.overwrite"),
    ("spotify_no_synced_lyrics_file",       "services.spotify.noSyncedLyricsFile"),
    ("spotify_save_playlist_file",          "services.spotify.savePlaylistFile"),
    ("spotify_save_cover_file",             "services.spotify.saveCoverFile"),
    ("spotify_synced_lyrics_only",          "services.spotify.syncedLyricsOnly"),
    ("spotify_album_folder_template",       "services.spotify.albumFolderTemplate"),
    ("spotify_compilation_folder_template", "services.spotify.compilationFolderTemplate"),
    ("spotify_podcast_folder_template",     "services.spotify.podcastFolderTemplate"),
    ("spotify_no_album_folder_template",    "services.spotify.noAlbumFolderTemplate"),
    ("spotify_single_disc_file_template",   "services.spotify.singleDiscFileTemplate"),
    ("spotify_multi_disc_file_template",    "services.spotify.multiDiscFileTemplate"),
    ("spotify_podcast_file_template",       "services.spotify.podcastFileTemplate"),
    ("spotify_no_album_file_template",      "services.spotify.noAlbumFileTemplate"),
    ("spotify_playlist_file_template",      "services.spotify.playlistFileTemplate"),
    ("spotify_date_tag_template",           "services.spotify.dateTagTemplate"),
    ("spotify_truncate",                    "services.spotify.truncate"),
    ("spotify_exclude_tags",                "services.spotify.excludeTags"),
    ("spotify_log_level",                   "services.spotify.logLevel"),
    ("spotify_no_exceptions",               "services.spotify.noExceptions"),
    ("spotify_artist_media_option",         "services.spotify.artistMediaOption"),
    ("spotify_prefer_video",                "services.spotify.preferVideo"),
    ("spotify_session_type",                "services.spotify.sessionType"),
    ("spotify_dll_path",                    "services.spotify.dllPath"),
    ("spotify_downloader_backend",          "services.spotify.downloaderBackend"),
    ("spotify_native_quality",              "services.spotify.nativeQuality"),
    ("spotify_sync_playback_history",       "services.spotify.syncPlaybackHistory"),
    ("spotify_telemetry_enabled",           "services.spotify.telemetryEnabled"),
    ("apple_cookies_path",                  "services.apple.cookiesPath"),
    ("apple_output_path",                   "services.apple.outputPath"),
    ("apple_temp_path",                     "services.apple.tempPath"),
    ("apple_download_mode",                 "services.apple.downloadMode"),
    ("apple_cover_format",                  "services.apple.coverFormat"),
    ("apple_cover_size",                    "services.apple.coverSize"),
    ("apple_save_cover",                    "services.apple.saveCover"),
    ("apple_sync_playback_history",         "services.apple.syncPlaybackHistory"),
    ("apple_synced_lyrics_format",          "services.apple.syncedLyricsFormat"),
    ("apple_synced_lyrics_only",            "services.apple.syncedLyricsOnly"),
    ("apple_no_synced_lyrics",              "services.apple.noSyncedLyrics"),
    ("apple_template_folder_album",         "services.apple.templateFolderAlbum"),
    ("apple_template_folder_compilation",   "services.apple.templateFolderCompilation"),
    ("apple_template_file_single_disc",     "services.apple.templateFileSingleDisc"),
    ("apple_template_file_multi_disc",      "services.apple.templateFileMultiDisc"),
    ("apple_template_folder_no_album",      "services.apple.templateFolderNoAlbum"),
    ("apple_template_file_no_album",        "services.apple.templateFileNoAlbum"),
    ("apple_template_file_playlist",        "services.apple.templateFilePlaylist"),
    ("apple_date_tag_template",             "services.apple.dateTagTemplate"),
    ("apple_save_playlist",                 "services.apple.savePlaylist"),
    ("apple_overwrite",                     "services.apple.overwrite"),
    ("apple_language",                      "services.apple.language"),
    ("apple_truncate",                      "services.apple.truncate"),
    ("apple_exclude_tags",                  "services.apple.excludeTags"),
    ("apple_log_level",                     "services.apple.logLevel"),
    ("apple_use_album_date",                "services.apple.useAlbumDate"),
    ("apple_no_exceptions",                 "services.apple.noExceptions"),
    ("apple_mv_codec_priority",             "services.apple.mvCodecPriority"),
    ("apple_mv_remux_format",               "services.apple.mvRemuxFormat"),
    ("apple_mv_resolution",                 "services.apple.mvResolution"),
    ("apple_uploaded_video_quality",        "services.apple.uploadedVideoQuality"),
    ("apple_nm3u8dlre_path",                "services.apple.nm3u8dlrePath"),
    ("apple_ffmpeg_path",                   "services.apple.ffmpegPath"),
    ("apple_wvd_path",                      "services.apple.wvdPath"),
    ("apple_use_wrapper",                   "services.apple.useWrapper"),
    ("apple_wrapper_url",                   "services.apple.wrapperUrl"),
    ("apple_wrapper_decrypt_host",          "services.apple.wrapperDecryptHost"),
    ("apple_wrapper_decrypt_port",          "services.apple.wrapperDecryptPort"),
    ("apple_wrapper_email",                 "services.apple.wrapperEmail"),
    ("apple_wrapper_password",              "services.apple.wrapperPassword"),
    ("apple_artist_auto_select",            "services.apple.artistAutoSelect"),
    ("apple_playlist_folder_template",      "services.apple.playlistFolderTemplate"),
    ("apple_downloader_backend",            "services.apple.downloaderBackend"),
    ("apple_native_quality",                "services.apple.nativeQuality"),
    ("qobuz_quality",                       "services.qobuz.quality"),
    ("qobuz_download_booklets",             "services.qobuz.downloadBooklets"),
    ("qobuz_email_or_userid",               "services.qobuz.emailOrUserid"),
    ("qobuz_password_or_token",             "services.qobuz.passwordOrToken"),
    ("qobuz_app_id",                        "services.qobuz.appId"),
    ("qobuz_app_secret",                    "services.qobuz.appSecret"),
    ("qobuz_secrets",                       "services.qobuz.secrets"),
    ("qobuz_telemetry_enabled",             "services.qobuz.telemetryEnabled"),
    ("qobuz_sync_playback_history",         "services.qobuz.syncPlaybackHistory"),
    ("qobuz_filters_extras",                "services.qobuz.filtersExtras"),
    ("qobuz_repeats",                       "services.qobuz.repeats"),
    ("qobuz_non_albums",                    "services.qobuz.nonAlbums"),
    ("qobuz_features",                      "services.qobuz.features"),
    ("qobuz_non_studio_albums",             "services.qobuz.nonStudioAlbums"),
    ("qobuz_non_remaster",                  "services.qobuz.nonRemaster"),
    ("tidal_quality",                       "services.tidal.quality"),
    ("tidal_telemetry_enabled",             "services.tidal.telemetryEnabled"),
    ("tidal_sync_playback_history",         "services.tidal.syncPlaybackHistory"),
    ("tidal_device_model",                  "services.tidal.deviceModel"),
    ("tidal_device_vendor",                 "services.tidal.deviceVendor"),
    ("tidal_device_type",                   "services.tidal.deviceType"),
    ("tidal_os_version",                    "services.tidal.osVersion"),
    ("tidal_screen_width",                  "services.tidal.screenWidth"),
    ("tidal_screen_height",                 "services.tidal.screenHeight"),
    ("tidal_download_videos",               "services.tidal.downloadVideos"),
    ("tidal_video_quality",                 "services.tidal.videoQuality"),
    ("tidal_user_id",                       "services.tidal.userId"),
    ("tidal_country_code",                  "services.tidal.countryCode"),
    ("tidal_access_token",                  "services.tidal.accessToken"),
    ("tidal_refresh_token",                 "services.tidal.refreshToken"),
    ("tidal_token_expiry",                  "services.tidal.tokenExpiry"),
    ("tidal_client_id",                     "services.tidal.clientId"),
    ("tidal_client_secret",                 "services.tidal.clientSecret"),
    ("deezer_quality",                      "services.deezer.quality"),
    ("deezer_arl",                          "services.deezer.arl"),
    ("deezer_lrc_public_fallback",          "services.deezer.lrcPublicFallback"),
    ("deezer_telemetry_enabled",            "services.deezer.telemetryEnabled"),
    ("deezer_sync_playback_history",        "services.deezer.syncPlaybackHistory"),
    ("youtube_quality",                     "services.youtube.quality"),
    ("youtube_cookies_path",                "services.youtube.cookiesPath"),
    ("youtube_api_key",                     "services.youtube.apiKey"),
    ("yt_override_download_extension",      "services.youtube.overrideDownloadExtension"),
    ("youtubeVideoExtensions",              "services.youtube.videoExtensions"),
    ("youtubeAudioExtensions",              "services.youtube.audioExtensions"),
    ("ytmusic_cookies_path",                "services.ytmusic.cookiesPath"),
    ("ytmusic_sync_playback_history",       "services.ytmusic.syncPlaybackHistory"),
    ("ytm_override_download_extension",     "services.ytmusic.overrideDownloadExtension"),
    ("ytdlp_cookies_path",                  "tools.ytdlp.cookiesPath"),
    ("player_client",                       "tools.ytdlp.playerClient"),
    ("ejs_remote_components",               "tools.ytdlp.ejsRemoteComponents"),
    ("pot_provider_enabled",                "tools.ytdlp.potProviderEnabled"),
    ("po_token",                            "tools.ytdlp.poToken"),
    ("pot_trace",                           "tools.ytdlp.potTrace"),
    ("orpheus_dl_enabled_modules",          "tools.orpheus.enabledModules"),
    ("orpheus_custom_modules",              "tools.orpheus.customModules"),
    ]
}

pub fn nest_flat_value(flat: Value) -> Value {
    let Value::Object(mut src) = flat else {
        return flat;
    };
    let mut out = serde_json::Map::new();
    if let Some(v) = src.remove("schemaVersion") {
        out.insert("schemaVersion".to_string(), v);
    }
    for (flat_key, dotted) in settings_grouping_table() {
        let Some(value) = src.remove(*flat_key) else {
            continue;
        };
        let mut cursor = &mut out;
        let segments: Vec<&str> = dotted.split('.').collect();
        for seg in &segments[..segments.len() - 1] {
            cursor = cursor
                .entry(seg.to_string())
                .or_insert_with(|| Value::Object(serde_json::Map::new()))
                .as_object_mut()
                .expect("grouping path segment is an object");
        }
        cursor.insert(segments[segments.len() - 1].to_string(), value);
    }
    for (k, v) in src {
        out.insert(k, v);
    }
    Value::Object(out)
}

pub fn flatten_nested_value(nested: Value) -> Value {
    let Value::Object(src) = &nested else {
        return nested;
    };
    let mut out = serde_json::Map::new();
    if let Some(v) = src.get("schemaVersion") {
        out.insert("schemaVersion".to_string(), v.clone());
    }
    for (flat_key, dotted) in settings_grouping_table() {
        let mut cursor: &Value = &nested;
        let mut found = true;
        for seg in dotted.split('.') {
            match cursor.get(seg) {
                Some(v) => cursor = v,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if found {
            out.insert(flat_key.to_string(), cursor.clone());
        }
    }
    Value::Object(out)
}

fn is_nested_shape(value: &Value) -> bool {
    value
        .as_object()
        .map(|m| m.contains_key("services") || m.contains_key("tools") || m.contains_key("general"))
        .unwrap_or(false)
}

pub fn settings_file_path(user_data: &Path) -> PathBuf {
    user_data.join("mh-settings.json")
}

pub fn spotify_config_path(user_data: &Path) -> PathBuf {
    user_data.join("votify_config.ini")
}

pub fn apple_config_path(user_data: &Path) -> PathBuf {
    user_data.join("gamdl_config.ini")
}

/// Rewrites values whose meaning changed between schema versions. Keys that were
/// merely deleted need nothing: `Settings` ignores what it does not declare, and
/// the file is rewritten from the struct on every load.
fn migrate_flat(flat: &mut Value, from_version: u64) {
    if from_version < 3 {
        if let Some(v) = flat.get_mut("native_synced_lyrics_format") {
            let migrated = match v.as_str().unwrap_or("").trim() {
                "ttml" | "both" => "ttml",
                _ => "lrc",
            };
            *v = Value::String(migrated.to_string());
        }
    }

    if from_version < 4 && flat.get("lyrics_fallback_lrclib").is_none() {
        let deezer_on = flat
            .get("deezer_lrc_public_fallback")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if let Some(m) = flat.as_object_mut() {
            m.insert("lyrics_fallback_lrclib".to_string(), Value::Bool(deezer_on));
        }
    }

    if from_version < 4 && flat.get("save_album_cover").is_none() {
        let had_covers = flat
            .get("save_cover")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if let Some(m) = flat.as_object_mut() {
            m.insert("save_album_cover".to_string(), Value::Bool(had_covers));
        }
    }

    // v5 added the Icecast directory as a default radio source. A saved list
    // that is exactly the old default was never a choice — the third option did
    // not exist yet — so it is replaced rather than left one source short. A
    // list the user actually changed is left alone.
    if from_version < 5 {
        const V4_DEFAULT: &[&str] = &["radiobrowser", "somafm"];
        let untouched = flat
            .get("radioDirectorySources")
            .and_then(|v| v.as_array())
            .is_some_and(|list| {
                list.len() == V4_DEFAULT.len()
                    && list
                        .iter()
                        .zip(V4_DEFAULT)
                        .all(|(v, want)| v.as_str() == Some(*want))
            });
        if untouched {
            if let Some(m) = flat.as_object_mut() {
                m.insert(
                    "radioDirectorySources".to_string(),
                    Value::Array(
                        crate::services::radio::DEFAULT_SOURCES
                            .iter()
                            .map(|s| Value::String((*s).to_string()))
                            .collect(),
                    ),
                );
            }
        }
    }
}

pub async fn load_settings(user_data: &Path) -> Settings {
    let path = settings_file_path(user_data);

    let mut settings: Settings = match fs::read_to_string(&path).await {
        Ok(data) => match serde_json::from_str::<Value>(&data) {
            Ok(value) => {
                let was_nested = is_nested_shape(&value);
                let on_disk_version = value
                    .get("schemaVersion")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let mut flat = if was_nested {
                    flatten_nested_value(value)
                } else {
                    value
                };
                migrate_flat(&mut flat, on_disk_version);
                match serde_json::from_value::<Settings>(flat) {
                    Ok(mut s) => {
                        if on_disk_version < CURRENT_SCHEMA_VERSION as u64 {
                            let bak = path.with_extension("json.bak");
                            let _ = fs::write(&bak, &data).await;
                        }
                        s.schema_version = CURRENT_SCHEMA_VERSION;
                        s
                    }
                    Err(_) => Settings::default(),
                }
            }
            Err(_) => Settings::default(),
        },
        Err(_) => Settings::default(),
    };

    if settings.download_location.is_empty() && !crate::sandbox::is_flatpak() {
        settings.download_location = crate::defaults::default_download_dir();
    }

    if settings.apple_temp_path.is_empty()
        || !std::path::Path::new(&settings.apple_temp_path).is_absolute()
    {
        settings.apple_temp_path = std::env::temp_dir()
            .join("mediaharbor")
            .to_string_lossy()
            .to_string();
    }

    if let Ok(spotify_cfg) = load_service_config(&spotify_config_path(user_data)).await {
        settings = merge_service_settings_json(settings, spotify_cfg, "spotify");
    }
    if let Ok(apple_cfg) = load_service_config(&apple_config_path(user_data)).await {
        settings = merge_service_settings_json(settings, apple_cfg, "apple");
    }

    let nested = nest_flat_value(serde_json::to_value(&settings).unwrap_or(Value::Null));
    let _ = fs::write(
        &path,
        serde_json::to_string_pretty(&nested).unwrap_or_default(),
    )
    .await;

    settings
}

pub async fn save_settings(settings: &Settings, user_data: &Path) -> MhResult<()> {
    let mut s = settings.clone();
    if s.create_platform_subfolders {
        s.spotify_output_path = std::path::Path::new(&s.download_location)
            .join("Spotify")
            .to_string_lossy()
            .to_string();
        s.apple_output_path = std::path::Path::new(&s.download_location)
            .join("Apple Music")
            .to_string_lossy()
            .to_string();
    } else {
        s.spotify_output_path = s.download_location.clone();
        s.apple_output_path = s.download_location.clone();
    }

    let nested = nest_flat_value(serde_json::to_value(&s)?);
    let json = serde_json::to_string_pretty(&nested)?;
    fs::write(settings_file_path(user_data), json).await?;

    save_service_config(&spotify_config_path(user_data), &s, "spotify").await?;
    save_service_config(&apple_config_path(user_data), &s, "apple").await?;

    Ok(())
}

/// The INI dialect one of the Python downloaders reads its config from.
struct IniTarget {
    section: &'static str,
    passthrough: &'static [&'static str],
    renames: &'static [(&'static str, &'static str)],
    /// Rewrites a value on the way out, keyed by the *downloader's* key name.
    fixup: Option<fn(&str, &str) -> String>,
    /// Carry over keys already in the file that MediaHarbor does not manage,
    /// instead of dropping them when the file is rewritten.
    keep_unmanaged: bool,
}

/// An unset or bare `N_m3u8DL-RE` has to become a real path, because gamdl execs
/// it directly rather than going through a shell PATH lookup.
fn gamdl_fixup(key: &str, value: &str) -> String {
    if key == "nm3u8dlre_path" && (value.is_empty() || value == "N_m3u8DL-RE") {
        crate::venv_manager::find_managed_or_path("N_m3u8DL-RE")
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "N_m3u8DL-RE".to_string())
    } else {
        value.to_string()
    }
}

fn ini_target(prefix: &str) -> Option<IniTarget> {
    match prefix {
        "apple" => Some(IniTarget {
            section: "gamdl",
            passthrough: GAMDL_PASSTHROUGH,
            renames: GAMDL_RENAMES,
            fixup: Some(gamdl_fixup),
            keep_unmanaged: false,
        }),
        "spotify" => Some(IniTarget {
            section: "votify",
            passthrough: VOTIFY_PASSTHROUGH,
            renames: VOTIFY_RENAMES,
            fixup: None,
            keep_unmanaged: true,
        }),
        _ => None,
    }
}

pub async fn save_service_config(
    config_path: &Path,
    settings: &Settings,
    prefix: &str,
) -> MhResult<()> {
    let flat = flatten_settings_for_prefix(settings, prefix);

    let Some(target) = ini_target(prefix) else {
        let json = serde_json::to_string_pretty(&flat)?;
        fs::write(config_path, json).await?;
        return Ok(());
    };

    let key_map = key_map(target.passthrough, target.renames);
    let mut ini = format!("[{}]\n", target.section);
    for (app_key, value) in &flat {
        if let Some(&out_key) = key_map.get(app_key.as_str()) {
            let out_value = match target.fixup {
                Some(f) => f(out_key, value),
                None => value.clone(),
            };
            ini.push_str(&format!("{} = {}\n", out_key, out_value));
        }
    }

    if target.keep_unmanaged {
        let managed: std::collections::HashSet<&str> = key_map.values().copied().collect();
        if let Ok(existing) = fs::read_to_string(config_path).await {
            for (k, v) in existing.lines().filter_map(parse_ini_line) {
                if !managed.contains(k) {
                    ini.push_str(&format!("{} = {}\n", k, v));
                }
            }
        }
    }

    fs::write(config_path, ini).await?;
    Ok(())
}

/// `key = value` out of an INI body line, skipping blanks, sections and comments.
fn parse_ini_line(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('[') || t.starts_with('#') {
        return None;
    }
    let (k, v) = t.split_once('=')?;
    Some((k.trim(), v.trim()))
}

pub async fn load_service_config(config_path: &Path) -> MhResult<HashMap<String, Value>> {
    let data = fs::read_to_string(config_path).await?;

    let ext = config_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if ext != "ini" {
        return Ok(serde_json::from_str(&data)?);
    }

    let prefix = if data.contains("[votify]") {
        "spotify"
    } else {
        "apple"
    };
    let target = ini_target(prefix).expect("both ini prefixes have a target");
    let to_app: HashMap<&str, &str> = key_map(target.passthrough, target.renames)
        .into_iter()
        .map(|(app, ini)| (ini, app))
        .collect();

    let mut result = HashMap::new();
    for (k, v) in data.lines().filter_map(parse_ini_line) {
        if let Some(&app_key) = to_app.get(k) {
            let val = match v {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => Value::String(v.to_string()),
            };
            result.insert(app_key.to_string(), val);
        }
    }
    Ok(result)
}

fn flatten_settings_for_prefix(settings: &Settings, prefix: &str) -> Vec<(String, String)> {
    let obj = serde_json::to_value(settings).unwrap_or(Value::Null);
    if let Value::Object(map) = obj {
        map.into_iter()
            .filter(|(k, v)| {
                k.starts_with(prefix)
                    && !matches!(v, Value::Null)
                    && v.as_str() != Some("")
                    && v.as_str() != Some("null")
            })
            .map(|(k, v)| {
                let short_key = k[prefix.len() + 1..].to_string();
                let str_val = match &v {
                    Value::Bool(b) => b.to_string(),
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                (short_key, str_val)
            })
            .collect()
    } else {
        Vec::new()
    }
}

fn merge_service_settings_json(
    settings: Settings,
    config: HashMap<String, Value>,
    prefix: &str,
) -> Settings {
    let mut obj = serde_json::to_value(&settings).unwrap_or(Value::Null);
    if let Value::Object(ref mut map) = obj {
        for (short_key, value) in config {
            let full_key = format!("{}_{}", prefix, short_key);
            let entry = map.entry(full_key);
            match entry {
                serde_json::map::Entry::Vacant(e) => {
                    e.insert(value);
                }
                serde_json::map::Entry::Occupied(mut e) => {
                    let cur = e.get();
                    let is_falsy = cur.is_null()
                        || cur == &Value::Bool(false)
                        || cur == &Value::Number(0.into())
                        || cur.as_str() == Some("");
                    if is_falsy {
                        e.insert(value);
                    }
                }
            }
        }
    }
    serde_json::from_value(obj).unwrap_or(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn round_trip_settings() {
        let dir = tempdir().unwrap();
        let settings = Settings::default();
        save_settings(&settings, dir.path()).await.unwrap();
        let loaded = load_settings(dir.path()).await;
        assert_eq!(settings.max_retries, loaded.max_retries);
        assert_eq!(settings.conversion_codec, loaded.conversion_codec);
    }

    #[test]
    fn apple_key_map_has_expected_keys() {
        let m = apple_to_gamdl_key_map();
        assert_eq!(m["cookies_path"], "cookies_path");
        assert_eq!(m["mv_codec_priority"], "music_video_codec_priority");
    }

    #[test]
    fn spotify_key_map_has_expected_keys() {
        let m = spotify_to_votify_key_map();
        assert_eq!(m["output_path"], "output");
        assert_eq!(m["prefer_video"], "prefer_video");
    }

    #[test]
    fn every_flat_key_is_in_grouping_table() {
        let flat = serde_json::to_value(Settings::default()).unwrap();
        let obj = flat.as_object().unwrap();
        let mapped: std::collections::HashSet<&str> =
            settings_grouping_table().iter().map(|(k, _)| *k).collect();
        let mut missing: Vec<&str> = obj
            .keys()
            .map(|k| k.as_str())
            .filter(|k| *k != "schemaVersion" && !mapped.contains(k))
            .collect();
        missing.sort_unstable();
        assert!(
            missing.is_empty(),
            "settings keys absent from grouping table (would be lost on next load): {missing:?}"
        );
    }

    /// The TypeScript mirror is hand-maintained, so nothing but a test stops it
    /// drifting from the struct. It did: `qobuz_app_secret` was missing while a
    /// phantom snake_case `auto_update` sat alongside the real `autoUpdate`, and
    /// the two errors cancelled out in a key count.
    #[test]
    fn typescript_mirror_declares_every_settings_key() {
        const TS: &str = include_str!("../../types/settings.ts");
        let declared: std::collections::HashSet<&str> = TS
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                let name = l.split(['?', ':']).next()?.trim();
                (!name.is_empty()
                    && l.contains(':')
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                .then_some(name)
            })
            .collect();

        let flat = serde_json::to_value(Settings::default()).unwrap();
        let mut missing: Vec<&str> = flat
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .filter(|k| !declared.contains(k))
            .collect();
        missing.sort_unstable();
        assert!(
            missing.is_empty(),
            "settings keys missing from src/types/settings.ts: {missing:?}"
        );
    }

    /// The Icecast directory arrived after v4 shipped, so a saved list of
    /// exactly the two old sources was never a decision the user made.
    #[test]
    fn v5_adds_the_new_radio_source_only_to_an_untouched_list() {
        let mut untouched = serde_json::json!({
            "radioDirectorySources": ["radiobrowser", "somafm"],
        });
        migrate_flat(&mut untouched, 4);
        assert_eq!(
            untouched["radioDirectorySources"],
            serde_json::json!(crate::services::radio::DEFAULT_SOURCES)
        );

        let mut chosen = serde_json::json!({ "radioDirectorySources": ["somafm"] });
        migrate_flat(&mut chosen, 4);
        assert_eq!(
            chosen["radioDirectorySources"],
            serde_json::json!(["somafm"]),
            "a list the user changed must be left alone"
        );
    }

    #[test]
    fn grouping_table_has_no_duplicate_targets() {
        let mut seen = std::collections::HashSet::new();
        for (_, dotted) in settings_grouping_table() {
            assert!(seen.insert(*dotted), "duplicate nested target: {dotted}");
        }
    }

    #[test]
    fn nest_then_flatten_is_identity() {
        let flat = serde_json::to_value(Settings::default()).unwrap();
        let nested = nest_flat_value(flat.clone());
        assert!(nested.get("services").is_some(), "expected nested services");
        assert!(nested.get("general").is_some(), "expected nested general");
        assert!(
            nested.pointer("/services/spotify/audioQuality").is_some(),
            "expected services.spotify.audioQuality"
        );
        assert!(
            nested.pointer("/tools/ytdlp/sponsorblockMark").is_some(),
            "sponsorblock must live under tools.ytdlp, not a generic bucket"
        );
        assert!(
            nested.pointer("/tools/ytdlp/subLangs").is_some(),
            "subtitle settings must live under tools.ytdlp"
        );
        assert!(
            nested
                .pointer("/general/pipeline/conversionCodec")
                .is_some(),
            "pipeline/native shared settings stay under general.pipeline"
        );
        assert!(
            nested.pointer("/services/ytmusic/cookiesPath").is_some(),
            "YT Music has its own service entry"
        );
        assert!(
            nested.pointer("/services/youtube/cookiesPath").is_some(),
            "YouTube has its own service entry"
        );

        let recovered = flatten_nested_value(nested);
        let back: Settings = serde_json::from_value(recovered).unwrap();
        let reserialized = serde_json::to_value(&back).unwrap();
        assert_eq!(flat, reserialized, "round-trip changed the settings value");
    }

    #[tokio::test]
    async fn nested_disk_file_still_generates_ini() {
        let dir = tempdir().unwrap();
        let settings = Settings {
            spotify_audio_quality: "OGG_VORBIS_320".into(),
            spotify_prefer_video: true,
            apple_cover_size: 5000,
            ..Default::default()
        };
        save_settings(&settings, dir.path()).await.unwrap();

        let on_disk: Value = serde_json::from_str(
            &fs::read_to_string(&settings_file_path(dir.path()))
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(
            on_disk.get("services").is_some(),
            "disk file must be nested"
        );

        let votify = fs::read_to_string(&spotify_config_path(dir.path()))
            .await
            .unwrap();
        assert!(votify.contains("[votify]"));
        assert!(
            votify.contains("audio_quality = OGG_VORBIS_320"),
            "votify INI lost the spotify setting: {votify}"
        );
        assert!(votify.contains("prefer_video = true"));

        let gamdl = fs::read_to_string(&apple_config_path(dir.path()))
            .await
            .unwrap();
        assert!(gamdl.contains("[gamdl]"));
        assert!(
            gamdl.contains("cover_size = 5000"),
            "gamdl INI lost the apple setting: {gamdl}"
        );
    }

    #[tokio::test]
    async fn legacy_flat_file_migrates_and_backs_up() {
        let dir = tempdir().unwrap();
        let path = settings_file_path(dir.path());
        let legacy = r#"{"theme":"dark","spotify_audio_quality":"OGG_VORBIS_320","max_retries":9}"#;
        fs::write(&path, legacy).await.unwrap();

        let loaded = load_settings(dir.path()).await;
        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.spotify_audio_quality, "OGG_VORBIS_320");
        assert_eq!(loaded.max_retries, 9);
        assert_eq!(loaded.schema_version, CURRENT_SCHEMA_VERSION);

        let on_disk: Value =
            serde_json::from_str(&fs::read_to_string(&path).await.unwrap()).unwrap();
        assert_eq!(
            on_disk.pointer("/services/spotify/audioQuality"),
            Some(&Value::String("OGG_VORBIS_320".into()))
        );
        assert_eq!(
            on_disk.pointer("/general/theme"),
            Some(&Value::String("dark".into()))
        );

        let bak = path.with_extension("json.bak");
        assert!(bak.exists(), "expected pre-migration .bak");
        assert_eq!(fs::read_to_string(&bak).await.unwrap(), legacy);
    }

    /// "Both" used to write two sidecars for one track, and "ttml" suppressed the
    /// `.lrc` outright. `ttml` now means "use TTML where it carries word timings",
    /// so `both` has to land there rather than falling through to the LRC default.
    #[tokio::test]
    async fn the_sidecar_format_setting_migrates_to_a_preference() {
        for (before, after) in [
            ("both", "ttml"),
            ("ttml", "ttml"),
            ("lrc", "lrc"),
            ("", "lrc"),
        ] {
            let dir = tempdir().unwrap();
            let path = settings_file_path(dir.path());
            fs::write(
                &path,
                format!(r#"{{"schemaVersion":2,"native_synced_lyrics_format":"{before}"}}"#),
            )
            .await
            .unwrap();

            let loaded = load_settings(dir.path()).await;
            assert_eq!(
                loaded.native_synced_lyrics_format, after,
                "{before:?} should migrate to {after:?}"
            );
        }
    }

    /// Settings removed in this pass must not survive a load, and must not take any
    /// live neighbour with them.
    #[tokio::test]
    async fn deleted_keys_are_dropped_without_disturbing_the_rest() {
        let dir = tempdir().unwrap();
        let path = settings_file_path(dir.path());
        fs::write(
            &path,
            r#"{"schemaVersion":2,"theme":"dark","lastfm_source":"qobuz","concurrency":true,
                "soundcloud_client_id":"abc","max_retries":7}"#,
        )
        .await
        .unwrap();

        let loaded = load_settings(dir.path()).await;
        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.max_retries, 7);

        let on_disk = fs::read_to_string(&path).await.unwrap();
        for gone in ["lastfm_source", "concurrency", "soundcloud_client_id"] {
            assert!(!on_disk.contains(gone), "{gone} survived the rewrite");
        }
    }
}

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde_json::Value;
use tokio::fs;

use crate::{defaults::Settings, errors::MhResult};

pub fn apple_to_gamdl_key_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("cookies_path", "cookies_path");
    m.insert("output_path", "output_path");
    m.insert("temp_path", "temp_path");
    m.insert("download_mode", "download_mode");
    m.insert("remux_mode", "music_video_remux_mode");
    m.insert("cover_format", "cover_format");
    m.insert("cover_size", "cover_size");
    m.insert("save_cover", "save_cover");
    m.insert("synced_lyrics_format", "synced_lyrics_format");
    m.insert("synced_lyrics_only", "synced_lyrics_only");
    m.insert("no_synced_lyrics", "no_synced_lyrics");
    m.insert("template_folder_album", "album_folder_template");
    m.insert("template_folder_compilation", "compilation_folder_template");
    m.insert("template_file_single_disc", "single_disc_file_template");
    m.insert("template_file_multi_disc", "multi_disc_file_template");
    m.insert("template_folder_no_album", "no_album_folder_template");
    m.insert("template_file_no_album", "no_album_file_template");
    m.insert("template_file_playlist", "playlist_file_template");
    m.insert("date_tag_template", "date_tag_template");
    m.insert("save_playlist", "save_playlist");
    m.insert("overwrite", "overwrite");
    m.insert("language", "language");
    m.insert("truncate", "truncate");
    m.insert("exclude_tags", "exclude_tags");
    m.insert("log_level", "log_level");
    m.insert("use_album_date", "use_album_date");
    m.insert("fetch_extra_tags", "fetch_extra_tags");
    m.insert("no_exceptions", "no_exceptions");
    m.insert("mv_codec_priority", "music_video_codec_priority");
    m.insert("mv_remux_format", "music_video_remux_format");
    m.insert("mv_resolution", "music_video_resolution");
    m.insert("uploaded_video_quality", "uploaded_video_quality");
    m.insert("nm3u8dlre_path", "nm3u8dlre_path");
    m.insert("mp4decrypt_path", "mp4decrypt_path");
    m.insert("ffmpeg_path", "ffmpeg_path");
    m.insert("mp4box_path", "mp4box_path");
    m.insert("wvd_path", "wvd_path");
    m.insert("use_wrapper", "use_wrapper");
    m.insert("wrapper_account_url", "wrapper_account_url");
    m.insert("wrapper_decrypt_ip", "wrapper_decrypt_ip");
    m
}

pub fn spotify_to_votify_key_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("cookies_path", "cookies_path");
    m.insert("output_path", "output");
    m.insert("audio_quality", "audio_quality");
    m.insert("audio_download_mode", "audio_download_mode");
    m.insert("audio_remux_mode", "audio_remux_mode");
    m.insert("video_format", "video_format");
    m.insert("video_resolution", "video_resolution");
    m.insert("video_remux_mode", "video_remux_mode");
    m.insert("cover_size", "cover_size");
    m.insert("wvd_path", "wvd_path");
    m.insert("no_drm", "no_drm");
    m.insert("wait_interval", "wait_interval");
    m.insert("overwrite", "overwrite");
    m.insert("no_synced_lyrics_file", "no_synced_lyrics_file");
    m.insert("save_playlist_file", "save_playlist_file");
    m.insert("save_cover_file", "save_cover_file");
    m.insert("synced_lyrics_only", "synced_lyrics_only");
    m.insert("album_folder_template", "album_folder_template");
    m.insert("compilation_folder_template", "compilation_folder_template");
    m.insert("podcast_folder_template", "podcast_folder_template");
    m.insert("no_album_folder_template", "no_album_folder_template");
    m.insert("single_disc_file_template", "single_disc_file_template");
    m.insert("multi_disc_file_template", "multi_disc_file_template");
    m.insert("podcast_file_template", "podcast_file_template");
    m.insert("no_album_file_template", "no_album_file_template");
    m.insert("playlist_file_template", "playlist_file_template");
    m.insert("date_tag_template", "date_tag_template");
    m.insert("truncate", "truncate");
    m.insert("exclude_tags", "exclude_tags");
    m.insert("log_level", "log_level");
    m.insert("no_exceptions", "no_exceptions");
    m.insert("artist_media_option", "artist_media_option");
    m.insert("prefer_video", "prefer_video");
    m
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

pub async fn load_settings(user_data: &Path) -> Settings {
    let path = settings_file_path(user_data);

    let mut settings: Settings = match fs::read_to_string(&path).await {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Settings::default(),
    };

    if settings.apple_temp_path.is_empty() || !std::path::Path::new(&settings.apple_temp_path).is_absolute() {
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

    let _ = fs::write(&path, serde_json::to_string_pretty(&settings).unwrap_or_default()).await;

    settings
}

pub async fn save_settings(settings: &Settings, user_data: &Path) -> MhResult<()> {
    let mut s = settings.clone();
    if s.create_platform_subfolders {
        s.spotify_output_path = format!("{}/Spotify", s.download_location);
        s.apple_output_path = format!("{}/Apple Music", s.download_location);
    } else {
        s.spotify_output_path = s.download_location.clone();
        s.apple_output_path = s.download_location.clone();
    }

    let json = serde_json::to_string_pretty(&s)?;
    fs::write(settings_file_path(user_data), json).await?;

    save_service_config(&spotify_config_path(user_data), &s, "spotify").await?;
    save_service_config(&apple_config_path(user_data), &s, "apple").await?;

    Ok(())
}

pub async fn save_service_config(config_path: &Path, settings: &Settings, prefix: &str) -> MhResult<()> {
    let flat = flatten_settings_for_prefix(settings, prefix);

    match prefix {
        "apple" => {
            let key_map = apple_to_gamdl_key_map();
            let mut ini = "[gamdl]\n".to_string();
            for (app_key, value) in &flat {
                if let Some(&gamdl_key) = key_map.get(app_key.as_str()) {
                    ini.push_str(&format!("{} = {}\n", gamdl_key, value));
                }
            }
            fs::write(config_path, ini).await?;
        }
        "spotify" => {
            let key_map = spotify_to_votify_key_map();
            let mapped_ini_keys: std::collections::HashSet<&str> =
                key_map.values().copied().collect();

            let mut extra: Vec<(String, String)> = Vec::new();
            if let Ok(existing) = fs::read_to_string(config_path).await {
                for line in existing.lines() {
                    let t = line.trim();
                    if t.is_empty() || t.starts_with('[') || t.starts_with('#') {
                        continue;
                    }
                    if let Some(eq) = t.find('=') {
                        let k = t[..eq].trim().to_string();
                        if !mapped_ini_keys.contains(k.as_str()) {
                            extra.push((k, t[eq + 1..].trim().to_string()));
                        }
                    }
                }
            }

            let mut ini = "[votify]\n".to_string();
            for (app_key, value) in &flat {
                if let Some(&votify_key) = key_map.get(app_key.as_str()) {
                    ini.push_str(&format!("{} = {}\n", votify_key, value));
                }
            }
            for (k, v) in &extra {
                ini.push_str(&format!("{} = {}\n", k, v));
            }
            fs::write(config_path, ini).await?;
        }
        _ => {
            let json = serde_json::to_string_pretty(&flat)?;
            fs::write(config_path, json).await?;
        }
    }

    Ok(())
}

pub async fn load_service_config(config_path: &Path) -> MhResult<HashMap<String, Value>> {
    let data = fs::read_to_string(config_path).await?;

    let ext = config_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    if ext == "ini" {
        let is_votify = data.contains("[votify]");
        let key_map: HashMap<&str, &str> = if is_votify {
            spotify_to_votify_key_map()
                .into_iter()
                .map(|(a, b)| (b, a))
                .collect()
        } else {
            apple_to_gamdl_key_map()
                .into_iter()
                .map(|(a, b)| (b, a))
                .collect()
        };

        let mut result = HashMap::new();
        for line in data.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('[') || t.starts_with('#') {
                continue;
            }
            if let Some(eq) = t.find('=') {
                let k = t[..eq].trim();
                let v = t[eq + 1..].trim();
                if let Some(&app_key) = key_map.get(k) {
                    let val: Value = match v {
                        "true" => Value::Bool(true),
                        "false" => Value::Bool(false),
                        _ => Value::String(v.to_string()),
                    };
                    result.insert(app_key.to_string(), val);
                }
            }
        }
        return Ok(result);
    }

    Ok(serde_json::from_str(&data)?)
}

fn flatten_settings_for_prefix(settings: &Settings, prefix: &str) -> Vec<(String, String)> {
    let obj = serde_json::to_value(settings).unwrap_or(Value::Null);
    if let Value::Object(map) = obj {
        map.into_iter()
            .filter(|(k, v)| {
                k.starts_with(prefix) && !matches!(v, Value::Null)
                    && v.as_str() != Some("")
                    && v.as_str() != Some("null")
            })
            .map(|(k, v)| {
                let short_key = k[prefix.len() + 1..].to_string(); // strip "prefix_"
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
                serde_json::map::Entry::Vacant(e) => { e.insert(value); }
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
}

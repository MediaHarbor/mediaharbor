use serde::{Deserialize, Serialize};

use crate::services::common::library::ServicePlatform;

pub fn default_download_dir() -> String {
    if crate::sandbox::is_snap() {
        if let Some(p) = crate::sandbox::snap_download_dir() {
            return p.to_string_lossy().to_string();
        }
    }
    dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
        .or_else(dirs::home_dir)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "autoUpdate")]
    pub auto_update: bool,
    #[serde(rename = "autoDownloadUpdates")]
    pub auto_download_updates: bool,
    pub theme: String,
    #[serde(rename = "downloadLocation")]
    pub download_location: String,
    #[serde(rename = "createPlatformSubfolders")]
    pub create_platform_subfolders: bool,
    #[serde(rename = "orpheusDL")]
    pub orpheus_dl: bool,

    #[serde(rename = "enabledServices")]
    pub enabled_services: Vec<String>,

    /// Station directories the radio page asks. The user's own stations are not
    /// listed here — they are data rather than a feed, and are always present.
    #[serde(rename = "radioDirectorySources")]
    pub radio_directory_sources: Vec<String>,

    pub use_cookies: bool,
    pub cookies: String,
    pub cookies_from_browser: String,
    pub ytdlp_cookies_path: String,
    pub youtube_cookies_path: String,
    pub ytmusic_cookies_path: String,
    pub ytmusic_sync_playback_history: bool,
    pub yt_override_download_extension: bool,
    pub ytm_override_download_extension: bool,
    #[serde(rename = "youtubeVideoExtensions")]
    pub youtube_video_extensions: String,
    #[serde(rename = "youtubeAudioExtensions")]
    pub youtube_audio_extensions: String,
    pub use_aria2: bool,
    pub max_downloads: u32,
    pub download_speed_limit: bool,
    pub speed_limit_type: String,
    pub speed_limit_value: u32,
    pub max_retries: u32,
    pub download_output_template: String,
    #[serde(rename = "continue")]
    pub continue_download: bool,
    pub add_metadata: bool,
    pub embed_chapters: bool,
    pub add_subtitle_to_file: bool,
    pub use_proxy: bool,
    pub proxy_url: String,
    pub use_authentication: bool,
    pub username: String,
    pub password: String,
    pub sponsorblock_mark: String,
    pub sponsorblock_remove: String,
    pub sponsorblock_chapter_title: String,
    pub no_sponsorblock: bool,
    pub sponsorblock_api_url: String,

    pub concurrent_fragments: u32,
    pub remux_format: String,
    pub convert_thumbnails_jpg: bool,
    pub sub_langs: String,
    pub write_auto_subs: bool,
    pub convert_subs_srt: bool,
    pub fragment_retries: String,
    pub extractor_retries: String,
    pub file_access_retries: String,
    pub socket_timeout: u32,
    pub throttled_rate: String,
    pub sleep_requests: String,
    pub sleep_interval: String,
    pub max_sleep_interval: String,
    pub aria2c_args: String,
    pub postprocessor_args: String,
    pub faststart_mp4: bool,
    pub trim_filenames: u32,
    pub restrict_filenames: bool,
    pub windows_filenames: bool,
    pub no_overwrites: bool,
    pub geo_bypass_country: String,
    pub set_mtime: bool,
    pub player_client: String,
    pub ejs_remote_components: String,
    pub pot_provider_enabled: bool,
    pub po_token: String,
    pub pot_trace: bool,
    pub extra_args: String,

    pub disc_subdirectories: bool,
    pub save_playlist_file: bool,

    pub qobuz_quality: u8,
    pub qobuz_download_booklets: bool,
    pub qobuz_email_or_userid: String,
    pub qobuz_password_or_token: String,
    pub qobuz_app_id: String,
    pub qobuz_app_secret: String,
    pub qobuz_secrets: String,
    pub qobuz_telemetry_enabled: bool,
    pub qobuz_sync_playback_history: bool,

    pub tidal_quality: u8,
    pub tidal_telemetry_enabled: bool,
    pub tidal_sync_playback_history: bool,
    pub tidal_device_model: String,
    pub tidal_device_vendor: String,
    pub tidal_device_type: String,
    pub tidal_os_version: String,
    pub tidal_screen_width: u32,
    pub tidal_screen_height: u32,
    pub tidal_download_videos: bool,
    pub tidal_video_quality: String,
    pub tidal_user_id: String,
    pub tidal_country_code: String,
    pub tidal_access_token: String,
    pub tidal_refresh_token: String,
    pub tidal_token_expiry: String,

    pub deezer_quality: String,
    pub deezer_arl: String,
    pub deezer_lrc_public_fallback: bool,
    pub deezer_telemetry_enabled: bool,
    pub deezer_sync_playback_history: bool,

    pub conversion_check: bool,
    pub conversion_codec: String,
    pub conversion_sampling_rate: Option<u32>,
    pub conversion_bit_depth: Option<u32>,
    pub conversion_lossy_bitrate: u32,

    pub meta_exclude_tags_check: bool,
    pub excluded_tags: String,

    pub filepaths_folder_format: String,
    pub filepaths_track_format: String,
    pub filepaths_restrict_characters: bool,
    pub filepaths_truncate_to: u32,

    pub embed_cover: bool,
    pub save_cover: bool,
    /// One `cover.jpg` for the release folder, independent of the per-track sidecar.
    pub save_album_cover: bool,
    /// Longest edge of the cover art fetched for Tidal, Qobuz and Deezer. Apple and
    /// Spotify have had their own key for this; these three had a hardcoded size.
    pub pipeline_cover_size: u32,
    /// Whether a track already sitting on disk is re-fetched and overwritten. The
    /// dedup ledger only knows what this install downloaded, so it cannot see a file
    /// restored from a backup or kept across a library reset.
    pub pipeline_overwrite: bool,
    pub save_lrc_files: bool,
    /// LRCLIB is account-less and applies to every service, so it is switched
    /// separately from Deezer's public endpoint.
    pub lyrics_fallback_lrclib: bool,
    pub native_synced_lyrics_format: String,
    pub embed_lyrics: bool,

    pub qobuz_filters_extras: bool,
    pub qobuz_repeats: bool,
    pub qobuz_non_albums: bool,
    pub qobuz_features: bool,
    pub qobuz_non_studio_albums: bool,
    pub qobuz_non_remaster: bool,

    pub youtube_quality: u32,

    pub spotify_client_id: String,
    pub spotify_client_secret: String,
    pub tidal_client_id: String,
    pub tidal_client_secret: String,
    pub youtube_api_key: String,

    pub spotify_cookies_path: String,
    pub spotify_output_path: String,
    pub spotify_audio_quality: String,
    pub spotify_audio_download_mode: String,
    pub spotify_audio_remux_mode: String,
    pub spotify_video_format: String,
    pub spotify_video_resolution: String,
    pub spotify_video_remux_mode: String,
    pub spotify_cover_size: String,
    pub spotify_wvd_path: String,
    pub spotify_no_drm: bool,
    pub spotify_wait_interval: u32,
    pub spotify_overwrite: bool,
    pub spotify_no_synced_lyrics_file: bool,
    pub spotify_save_playlist_file: bool,
    pub spotify_save_cover_file: bool,
    pub spotify_synced_lyrics_only: bool,
    pub spotify_album_folder_template: String,
    pub spotify_compilation_folder_template: String,
    pub spotify_podcast_folder_template: String,
    pub spotify_no_album_folder_template: String,
    pub spotify_single_disc_file_template: String,
    pub spotify_multi_disc_file_template: String,
    pub spotify_podcast_file_template: String,
    pub spotify_no_album_file_template: String,
    pub spotify_playlist_file_template: String,
    pub spotify_date_tag_template: String,
    pub spotify_truncate: u32,
    pub spotify_exclude_tags: String,
    pub spotify_log_level: String,
    pub spotify_no_exceptions: bool,
    pub spotify_artist_media_option: String,
    pub spotify_prefer_video: bool,
    pub spotify_session_type: String,
    pub spotify_dll_path: String,

    pub spotify_downloader_backend: String,
    pub spotify_native_quality: String,
    pub spotify_sync_playback_history: bool,
    pub spotify_telemetry_enabled: bool,

    pub native_skip_existing: bool,

    pub apple_cookies_path: String,
    pub apple_output_path: String,
    pub apple_temp_path: String,
    pub apple_download_mode: String,
    pub apple_cover_format: String,
    pub apple_cover_size: u32,
    pub apple_save_cover: bool,
    pub apple_synced_lyrics_format: String,
    pub apple_synced_lyrics_only: bool,
    pub apple_no_synced_lyrics: bool,
    pub apple_template_folder_album: String,
    pub apple_template_folder_compilation: String,
    pub apple_template_file_single_disc: String,
    pub apple_template_file_multi_disc: String,
    pub apple_template_folder_no_album: String,
    pub apple_template_file_no_album: String,
    pub apple_template_file_playlist: String,
    pub apple_date_tag_template: String,
    pub apple_save_playlist: bool,
    pub apple_overwrite: bool,
    pub apple_language: String,
    pub apple_truncate: u32,
    pub apple_exclude_tags: String,
    pub apple_log_level: String,
    pub apple_use_album_date: bool,
    pub apple_no_exceptions: bool,
    pub apple_mv_codec_priority: String,
    pub apple_mv_remux_format: String,
    pub apple_mv_resolution: String,
    pub apple_uploaded_video_quality: String,
    pub apple_nm3u8dlre_path: String,
    pub apple_ffmpeg_path: String,
    pub apple_wvd_path: String,
    pub apple_use_wrapper: bool,
    pub apple_wrapper_url: String,
    pub apple_wrapper_decrypt_host: String,
    pub apple_wrapper_decrypt_port: String,
    pub apple_wrapper_email: String,
    pub apple_wrapper_password: String,
    pub apple_artist_auto_select: String,
    pub apple_playlist_folder_template: String,

    pub apple_downloader_backend: String,
    pub apple_native_quality: String,
    pub apple_sync_playback_history: bool,

    pub orpheus_dl_enabled_modules: String,
    pub orpheus_custom_modules: String,

    pub replaygain_mode: String,
    pub crossfade_enabled: bool,
    pub crossfade_duration: u32,

    pub onboarding_completed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let download_location = if crate::sandbox::is_flatpak() {
            String::new()
        } else {
            default_download_dir()
        };

        let apple_temp_path = std::env::temp_dir()
            .join("mediaharbor")
            .to_string_lossy()
            .to_string();

        Self {
            schema_version: crate::settings::CURRENT_SCHEMA_VERSION,
            auto_update: true,
            auto_download_updates: false,
            theme: "auto".into(),
            download_location,
            create_platform_subfolders: false,
            orpheus_dl: false,
            enabled_services: Vec::new(),
            radio_directory_sources: crate::services::radio::DEFAULT_SOURCES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),

            use_cookies: false,
            cookies: String::new(),
            cookies_from_browser: String::new(),
            ytdlp_cookies_path: String::new(),
            youtube_cookies_path: String::new(),
            ytmusic_cookies_path: String::new(),
            ytmusic_sync_playback_history: false,
            yt_override_download_extension: false,
            ytm_override_download_extension: false,
            youtube_video_extensions: "mp4".into(),
            youtube_audio_extensions: "mp3".into(),
            use_aria2: false,
            max_downloads: 0,
            download_speed_limit: false,
            speed_limit_type: "M".into(),
            speed_limit_value: 0,
            max_retries: 5,
            download_output_template: "%(title)s.%(ext)s".into(),
            continue_download: true,
            add_metadata: false,
            embed_chapters: false,
            add_subtitle_to_file: false,
            use_proxy: false,
            proxy_url: String::new(),
            use_authentication: false,
            username: String::new(),
            password: String::new(),
            sponsorblock_mark: "all".into(),
            sponsorblock_remove: String::new(),
            sponsorblock_chapter_title: "[SponsorBlock]: %(category_names)l".into(),
            no_sponsorblock: false,
            sponsorblock_api_url: "https://sponsor.ajay.app".into(),

            concurrent_fragments: 4,
            remux_format: "mp4".into(),
            convert_thumbnails_jpg: true,
            sub_langs: String::new(),
            write_auto_subs: false,
            convert_subs_srt: false,
            fragment_retries: "10".into(),
            extractor_retries: "3".into(),
            file_access_retries: "3".into(),
            socket_timeout: 15,
            throttled_rate: String::new(),
            sleep_requests: String::new(),
            sleep_interval: String::new(),
            max_sleep_interval: String::new(),
            aria2c_args: "-x16 -s16 -k1M".into(),
            postprocessor_args: String::new(),
            faststart_mp4: true,
            trim_filenames: 0,
            restrict_filenames: false,
            windows_filenames: false,
            no_overwrites: true,
            geo_bypass_country: String::new(),
            set_mtime: false,
            player_client: String::new(),
            ejs_remote_components: "ejs:github".into(),
            pot_provider_enabled: false,
            po_token: String::new(),
            pot_trace: false,
            extra_args: String::new(),

            disc_subdirectories: true,
            save_playlist_file: true,

            qobuz_quality: 27,
            qobuz_download_booklets: true,
            qobuz_email_or_userid: String::new(),
            qobuz_password_or_token: String::new(),
            qobuz_app_id: String::new(),
            qobuz_app_secret: String::new(),
            qobuz_secrets: String::new(),
            qobuz_telemetry_enabled: false,
            qobuz_sync_playback_history: false,

            tidal_quality: 3,
            tidal_telemetry_enabled: false,
            tidal_sync_playback_history: false,
            tidal_device_model: "SM-A166B".to_string(),
            tidal_device_vendor: "samsung".to_string(),
            tidal_device_type: "phone".to_string(),
            tidal_os_version: "14".to_string(),
            tidal_screen_width: 1080,
            tidal_screen_height: 2340,
            tidal_download_videos: false,
            tidal_video_quality: "1080p".into(),
            tidal_user_id: String::new(),
            tidal_country_code: String::new(),
            tidal_access_token: String::new(),
            tidal_refresh_token: String::new(),
            tidal_token_expiry: String::new(),

            deezer_quality: "FLAC".into(),
            deezer_arl: String::new(),
            deezer_lrc_public_fallback: true,
            deezer_telemetry_enabled: false,
            deezer_sync_playback_history: false,

            conversion_check: false,
            conversion_codec: "FLAC".into(),
            conversion_sampling_rate: None,
            conversion_bit_depth: None,
            conversion_lossy_bitrate: 320,

            meta_exclude_tags_check: false,
            excluded_tags: String::new(),

            filepaths_folder_format: "{albumartist} - {album} ({year})".into(),
            filepaths_track_format: "{tracknumber:02}. {artist} - {title}{explicit}".into(),
            filepaths_restrict_characters: true,
            filepaths_truncate_to: 120,

            embed_cover: true,
            save_cover: false,
            save_album_cover: false,
            pipeline_cover_size: 1280,
            pipeline_overwrite: false,
            save_lrc_files: false,
            lyrics_fallback_lrclib: true,
            native_synced_lyrics_format: "lrc".into(),
            embed_lyrics: true,

            qobuz_filters_extras: false,
            qobuz_repeats: false,
            qobuz_non_albums: false,
            qobuz_features: false,
            qobuz_non_studio_albums: false,
            qobuz_non_remaster: false,

            youtube_quality: 0,

            spotify_client_id: String::new(),
            spotify_client_secret: String::new(),
            tidal_client_id: String::new(),
            tidal_client_secret: String::new(),
            youtube_api_key: String::new(),

            spotify_cookies_path: String::new(),
            spotify_output_path: String::new(),
            spotify_audio_quality: "FLAC".into(),
            spotify_audio_download_mode: "ytdlp".into(),
            spotify_audio_remux_mode: "ffmpeg".into(),
            spotify_video_format: "mp4".into(),
            spotify_video_resolution: "1080p".into(),
            spotify_video_remux_mode: "ffmpeg".into(),
            spotify_cover_size: "large".into(),
            spotify_wvd_path: String::new(),
            spotify_no_drm: false,
            spotify_wait_interval: 10,
            spotify_overwrite: false,
            spotify_no_synced_lyrics_file: false,
            spotify_save_playlist_file: false,
            spotify_save_cover_file: false,
            spotify_synced_lyrics_only: false,
            spotify_album_folder_template: "{album_artist}/{album}".into(),
            spotify_compilation_folder_template: "Compilations/{album}".into(),
            spotify_podcast_folder_template: "{podcast_name}".into(),
            spotify_no_album_folder_template: "{album_artist}/Unknown Album".into(),
            spotify_single_disc_file_template: "{track:02d} {title}".into(),
            spotify_multi_disc_file_template: "{disc}-{track:02d} {title}".into(),
            spotify_podcast_file_template: "{episode_number} - {title}".into(),
            spotify_no_album_file_template: "{title}".into(),
            spotify_playlist_file_template: "Playlists/{playlist_title}/{track:02d} {title}".into(),
            spotify_date_tag_template: "%Y-%m-%dT%H:%M:%SZ".into(),
            spotify_truncate: 40,
            spotify_exclude_tags: String::new(),
            spotify_log_level: "INFO".into(),
            spotify_no_exceptions: false,
            spotify_artist_media_option: "albums".into(),
            spotify_prefer_video: false,
            spotify_session_type: "librespot".into(),
            spotify_dll_path: String::new(),

            spotify_downloader_backend: "native".into(),
            spotify_native_quality: "aac-high".into(),
            spotify_sync_playback_history: false,
            spotify_telemetry_enabled: false,
            native_skip_existing: true,

            apple_cookies_path: String::new(),
            apple_output_path: "Apple Music".into(),
            apple_temp_path,
            apple_download_mode: "ytdlp".into(),
            apple_cover_format: "jpg".into(),
            apple_cover_size: 1200,
            apple_save_cover: false,
            apple_synced_lyrics_format: "lrc".into(),
            apple_synced_lyrics_only: false,
            apple_no_synced_lyrics: false,
            apple_template_folder_album: "{album_artist}/{album}".into(),
            apple_template_folder_compilation: "Compilations/{album}".into(),
            apple_template_file_single_disc: "{track:02d} {title}".into(),
            apple_template_file_multi_disc: "{disc}-{track:02d} {title}".into(),
            apple_template_folder_no_album: "{album_artist}/Unknown Album".into(),
            apple_template_file_no_album: "{title}".into(),
            apple_template_file_playlist: "Playlists/{playlist_title}/{track:02d} {title}".into(),
            apple_date_tag_template: "%Y-%m-%dT%H:%M:%SZ".into(),
            apple_save_playlist: true,
            apple_overwrite: true,
            apple_language: "en-US".into(),
            apple_truncate: 40,
            apple_exclude_tags: String::new(),
            apple_log_level: "INFO".into(),
            apple_use_album_date: false,
            apple_no_exceptions: false,
            apple_mv_codec_priority: "h264".into(),
            apple_mv_remux_format: "m4v".into(),
            apple_mv_resolution: "1080p".into(),
            apple_uploaded_video_quality: "best".into(),
            apple_nm3u8dlre_path: "N_m3u8DL-RE".into(),
            apple_ffmpeg_path: "ffmpeg".into(),
            apple_wvd_path: String::new(),
            apple_use_wrapper: false,
            apple_wrapper_url: "http://127.0.0.1".into(),
            apple_wrapper_decrypt_host: "127.0.0.1".into(),
            apple_wrapper_decrypt_port: "10020".into(),
            apple_wrapper_email: String::new(),
            apple_wrapper_password: String::new(),
            apple_artist_auto_select: String::new(),
            apple_playlist_folder_template: "Playlists/{playlist_name}".into(),

            apple_downloader_backend: "native".into(),
            apple_native_quality: "aac-256".into(),
            apple_sync_playback_history: false,

            orpheus_dl_enabled_modules: "tidal,qobuz,deezer".into(),
            orpheus_custom_modules: "[]".into(),

            replaygain_mode: "off".to_string(),
            crossfade_enabled: false,
            crossfade_duration: 6,

            onboarding_completed: false,
        }
    }
}

impl Settings {
    /// Whether finished plays should be written back to the service's own
    /// listening history. Off for every service by default.
    pub fn sync_playback_history(&self, platform: ServicePlatform) -> bool {
        match platform {
            ServicePlatform::Spotify => self.spotify_sync_playback_history,
            ServicePlatform::Tidal => self.tidal_sync_playback_history,
            ServicePlatform::Qobuz => self.qobuz_sync_playback_history,
            ServicePlatform::Deezer => self.deezer_sync_playback_history,
            ServicePlatform::AppleMusic => self.apple_sync_playback_history,
            ServicePlatform::YtMusic => self.ytmusic_sync_playback_history,
            ServicePlatform::Youtube => false,
        }
    }

    /// Whether to replicate the service app's own analytics traffic. Only the
    /// four services whose telemetry has been captured can answer `true`.
    pub fn telemetry_enabled(&self, platform: ServicePlatform) -> bool {
        match platform {
            ServicePlatform::Spotify => self.spotify_telemetry_enabled,
            ServicePlatform::Tidal => self.tidal_telemetry_enabled,
            ServicePlatform::Qobuz => self.qobuz_telemetry_enabled,
            ServicePlatform::Deezer => self.deezer_telemetry_enabled,
            ServicePlatform::AppleMusic | ServicePlatform::YtMusic | ServicePlatform::Youtube => {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_match_js() {
        let s = Settings::default();
        assert!(s.auto_update);
        assert_eq!(s.theme, "auto");
        assert_eq!(s.max_retries, 5);
        assert_eq!(s.conversion_codec, "FLAC");
        assert_eq!(s.conversion_lossy_bitrate, 320);
        assert_eq!(s.filepaths_truncate_to, 120);
        assert_eq!(s.spotify_audio_download_mode, "ytdlp");
        assert_eq!(s.apple_cover_size, 1200);
        assert!(s.embed_cover);
    }

    #[test]
    fn settings_round_trip_json() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        let s2: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s.theme, s2.theme);
        assert_eq!(s.max_retries, s2.max_retries);
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(rename = "autoUpdate")]
    pub auto_update: bool,
    pub theme: String,
    #[serde(rename = "downloadLocation")]
    pub download_location: String,
    #[serde(rename = "createPlatformSubfolders")]
    pub create_platform_subfolders: bool,
    #[serde(rename = "orpheusDL")]
    pub orpheus_dl: bool,

    pub use_cookies: bool,
    pub cookies: String,
    pub cookies_from_browser: String,
    pub override_download_extension: bool,
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

    pub disc_subdirectories: bool,
    pub concurrency: bool,
    pub max_connections: u32,
    pub requests_per_minute: u32,

    pub qobuz_quality: u8,
    pub qobuz_download_booklets: bool,
    pub qobuz_token_or_email: bool,
    pub qobuz_email_or_userid: String,
    pub qobuz_password_or_token: String,
    pub qobuz_app_id: String,
    pub qobuz_app_secret: String,
    pub qobuz_secrets: String,

    pub tidal_quality: u8,
    pub tidal_download_videos: bool,
    pub tidal_user_id: String,
    pub tidal_country_code: String,
    pub tidal_access_token: String,
    pub tidal_refresh_token: String,
    pub tidal_token_expiry: String,

    pub deezer_quality: String,
    pub deezer_use_deezloader: bool,
    pub deezer_arl: String,
    pub deezloader_warnings: bool,

    pub downloads_database_check: bool,
    pub downloads_database: String,
    pub failed_downloads_database_check: bool,
    pub failed_downloads_database: String,

    pub conversion_check: bool,
    pub conversion_codec: String,
    pub conversion_sampling_rate: Option<u32>,
    pub conversion_bit_depth: Option<u32>,
    pub conversion_lossy_bitrate: u32,

    pub meta_album_name_playlist_check: bool,
    pub meta_album_order_playlist_check: bool,
    pub meta_exclude_tags_check: bool,
    pub excluded_tags: String,

    pub filepaths_add_singles_to_folder: bool,
    pub filepaths_folder_format: String,
    pub filepaths_track_format: String,
    pub filepaths_restrict_characters: bool,
    pub filepaths_truncate_to: u32,

    pub embed_cover: bool,
    pub save_cover: bool,
    pub save_lrc_files: bool,

    pub qobuz_filters_extras: bool,
    pub qobuz_repeats: bool,
    pub qobuz_non_albums: bool,
    pub qobuz_features: bool,
    pub qobuz_non_studio_albums: bool,
    pub qobuz_non_remaster: bool,

    pub soundcloud_quality: u32,
    pub soundcloud_client_id: String,
    pub soundcloud_app_version: String,

    pub youtube_quality: u32,
    pub youtube_download_videos: bool,
    pub youtube_video_downloads_folder: String,

    pub lastfm_source: String,
    pub lastfm_fallback_source: String,

    pub cli_text_output: bool,
    pub cli_progress_bars: bool,
    pub cli_max_search_results: String,

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

    pub apple_cookies_path: String,
    pub apple_output_path: String,
    pub apple_temp_path: String,
    pub apple_download_mode: String,
    pub apple_remux_mode: String,
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
    pub apple_fetch_extra_tags: bool,
    pub apple_no_exceptions: bool,
    pub apple_mv_enabled: bool,
    pub apple_mv_codec_priority: String,
    pub apple_mv_remux_format: String,
    pub apple_mv_resolution: String,
    pub apple_uploaded_video_quality: String,
    pub apple_custom_paths_enabled: bool,
    pub apple_nm3u8dlre_path: String,
    pub apple_mp4decrypt_path: String,
    pub apple_ffmpeg_path: String,
    pub apple_mp4box_path: String,
    pub apple_wvd_path: String,
    pub apple_use_wrapper: bool,
    pub apple_wrapper_account_url: String,
    pub apple_wrapper_decrypt_ip: String,

    pub orpheus_dl_enabled_modules: String,
    pub orpheus_download_quality: String,
    pub orpheus_covers_enabled: bool,
    pub orpheus_custom_modules: String,
}

impl Default for Settings {
    fn default() -> Self {
        let download_location = dirs::download_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let apple_temp_path = std::env::temp_dir()
            .join("mediaharbor")
            .to_string_lossy()
            .to_string();

        Self {
            auto_update: true,
            theme: "auto".into(),
            download_location,
            create_platform_subfolders: false,
            orpheus_dl: false,

            use_cookies: false,
            cookies: String::new(),
            cookies_from_browser: String::new(),
            override_download_extension: false,
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

            disc_subdirectories: true,
            concurrency: true,
            max_connections: 6,
            requests_per_minute: 60,

            qobuz_quality: 27,
            qobuz_download_booklets: true,
            qobuz_token_or_email: false,
            qobuz_email_or_userid: String::new(),
            qobuz_password_or_token: String::new(),
            qobuz_app_id: String::new(),
            qobuz_app_secret: String::new(),
            qobuz_secrets: String::new(),

            tidal_quality: 3,
            tidal_download_videos: false,
            tidal_user_id: String::new(),
            tidal_country_code: String::new(),
            tidal_access_token: String::new(),
            tidal_refresh_token: String::new(),
            tidal_token_expiry: String::new(),

            deezer_quality: "FLAC".into(),
            deezer_use_deezloader: false,
            deezer_arl: String::new(),
            deezloader_warnings: true,

            downloads_database_check: false,
            downloads_database: String::new(),
            failed_downloads_database_check: false,
            failed_downloads_database: String::new(),

            conversion_check: false,
            conversion_codec: "FLAC".into(),
            conversion_sampling_rate: None,
            conversion_bit_depth: None,
            conversion_lossy_bitrate: 320,

            meta_album_name_playlist_check: false,
            meta_album_order_playlist_check: false,
            meta_exclude_tags_check: false,
            excluded_tags: String::new(),

            filepaths_add_singles_to_folder: true,
            filepaths_folder_format: "{albumartist} - {album} ({year})".into(),
            filepaths_track_format: "{tracknumber:02}. {artist} - {title}{explicit}".into(),
            filepaths_restrict_characters: true,
            filepaths_truncate_to: 120,

            embed_cover: true,
            save_cover: false,
            save_lrc_files: false,

            qobuz_filters_extras: false,
            qobuz_repeats: false,
            qobuz_non_albums: false,
            qobuz_features: false,
            qobuz_non_studio_albums: false,
            qobuz_non_remaster: false,

            soundcloud_quality: 0,
            soundcloud_client_id: String::new(),
            soundcloud_app_version: String::new(),

            youtube_quality: 0,
            youtube_download_videos: false,
            youtube_video_downloads_folder: String::new(),

            lastfm_source: "qobuz".into(),
            lastfm_fallback_source: String::new(),

            cli_text_output: true,
            cli_progress_bars: true,
            cli_max_search_results: "100".into(),

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

            apple_cookies_path: String::new(),
            apple_output_path: "Apple Music".into(),
            apple_temp_path,
            apple_download_mode: "ytdlp".into(),
            apple_remux_mode: "ffmpeg".into(),
            apple_cover_format: "jpg".into(),
            apple_cover_size: 1200,
            apple_save_cover: true,
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
            apple_fetch_extra_tags: false,
            apple_no_exceptions: false,
            apple_mv_enabled: false,
            apple_mv_codec_priority: "h264".into(),
            apple_mv_remux_format: "m4v".into(),
            apple_mv_resolution: "1080p".into(),
            apple_uploaded_video_quality: "best".into(),
            apple_custom_paths_enabled: false,
            apple_nm3u8dlre_path: "N_m3u8DL-RE".into(),
            apple_mp4decrypt_path: "mp4decrypt".into(),
            apple_ffmpeg_path: "ffmpeg".into(),
            apple_mp4box_path: "MP4Box".into(),
            apple_wvd_path: String::new(),
            apple_use_wrapper: false,
            apple_wrapper_account_url: String::new(),
            apple_wrapper_decrypt_ip: String::new(),

            orpheus_dl_enabled_modules: "tidal,qobuz,deezer".into(),
            orpheus_download_quality: "lossless".into(),
            orpheus_covers_enabled: true,
            orpheus_custom_modules: "[]".into(),
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
        assert_eq!(s.requests_per_minute, 60);
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

export interface Settings {
  autoUpdate: boolean;
  theme: 'auto' | 'dark' | 'light';
  downloadLocation: string;
  createPlatformSubfolders: boolean;
  orpheusDL: boolean;

  use_cookies: boolean;
  cookies: string;
  cookies_from_browser: string;
  override_download_extension: boolean;
  yt_override_download_extension: boolean;
  ytm_override_download_extension: boolean;
  youtubeVideoExtensions: string;
  youtubeAudioExtensions: string;
  use_aria2: boolean;
  auto_update: boolean;
  max_downloads: number;
  download_speed_limit: boolean;
  speed_limit_type: string;
  speed_limit_value: number;
  max_retries: number;
  download_output_template: string;
  continue: boolean;
  add_metadata: boolean;
  embed_chapters: boolean;
  add_subtitle_to_file: boolean;
  use_proxy: boolean;
  proxy_url: string;
  use_authentication: boolean;
  username: string;
  password: string;
  sponsorblock_mark: string;
  sponsorblock_remove: string;
  sponsorblock_chapter_title: string;
  no_sponsorblock: boolean;
  sponsorblock_api_url: string;

  disc_subdirectories: boolean;
  concurrency: boolean;
  max_connections: number;
  requests_per_minute: number;

  qobuz_quality: number;
  qobuz_download_booklets: boolean;
  qobuz_token_or_email: boolean;
  qobuz_email_or_userid: string;
  qobuz_password_or_token: string;
  qobuz_app_id: string;
  qobuz_secrets: string;
  qobuz_filters_extras: boolean;
  qobuz_repeats: boolean;
  qobuz_non_albums: boolean;
  qobuz_features: boolean;
  qobuz_non_studio_albums: boolean;
  qobuz_non_remaster: boolean;

  tidal_quality: number;
  tidal_download_videos: boolean;
  tidal_user_id: string;
  tidal_country_code: string;
  tidal_access_token: string;
  tidal_refresh_token: string;
  tidal_token_expiry: string;
  tidal_client_id: string;
  tidal_client_secret: string;

  deezer_quality: string;
  deezer_use_deezloader: boolean;
  deezer_arl: string;
  deezloader_warnings: boolean;

  downloads_database_check: boolean;
  downloads_database: string;
  failed_downloads_database_check: boolean;
  failed_downloads_database: string;

  conversion_check: boolean;
  conversion_codec: string;
  conversion_sampling_rate: number | null;
  conversion_bit_depth: number | null;
  conversion_lossy_bitrate: number;

  meta_album_name_playlist_check: boolean;
  meta_album_order_playlist_check: boolean;
  meta_exclude_tags_check: boolean;
  excluded_tags: string;

  filepaths_add_singles_to_folder: boolean;
  filepaths_folder_format: string;
  filepaths_track_format: string;
  filepaths_restrict_characters: boolean;
  filepaths_truncate_to: number;

  embed_cover: boolean;
  save_cover: boolean;
  save_lrc_files: boolean;

  soundcloud_quality: number;
  soundcloud_client_id: string;
  soundcloud_app_version: string;

  youtube_quality: number;
  youtube_download_videos: boolean;
  youtube_video_downloads_folder: string;
  youtube_api_key: string;

  lastfm_source: string;
  lastfm_fallback_source: string;

  cli_text_output: boolean;
  cli_progress_bars: boolean;
  cli_max_search_results: string;

  spotify_client_id: string;
  spotify_client_secret: string;
  spotify_cookies_path: string;
  spotify_output_path: string;
  spotify_audio_quality: string;
  spotify_audio_download_mode: string;
  spotify_audio_remux_mode: string;
  spotify_video_format: string;
  spotify_video_resolution: string;
  spotify_video_remux_mode: string;
  spotify_cover_size: string;
  spotify_wvd_path: string;
  spotify_no_drm: boolean;
  spotify_wait_interval: number;
  spotify_overwrite: boolean;
  spotify_no_synced_lyrics_file: boolean;
  spotify_save_playlist_file: boolean;
  spotify_save_cover_file: boolean;
  spotify_synced_lyrics_only: boolean;
  spotify_album_folder_template: string;
  spotify_compilation_folder_template: string;
  spotify_podcast_folder_template: string;
  spotify_no_album_folder_template: string;
  spotify_single_disc_file_template: string;
  spotify_multi_disc_file_template: string;
  spotify_podcast_file_template: string;
  spotify_no_album_file_template: string;
  spotify_playlist_file_template: string;
  spotify_date_tag_template: string;
  spotify_truncate: number;
  spotify_exclude_tags: string;
  spotify_log_level: string;
  spotify_no_exceptions: boolean;
  spotify_artist_media_option: string;
  spotify_prefer_video: boolean;

  apple_cookies_path: string;
  apple_output_path: string;
  apple_temp_path: string;
  apple_download_mode: string;
  apple_remux_mode: string;
  apple_cover_format: string;
  apple_cover_size: number;
  apple_save_cover: boolean;
  apple_synced_lyrics_format: string;
  apple_synced_lyrics_only: boolean;
  apple_no_synced_lyrics: boolean;
  apple_template_folder_album: string;
  apple_template_folder_compilation: string;
  apple_template_file_single_disc: string;
  apple_template_file_multi_disc: string;
  apple_template_folder_no_album: string;
  apple_template_file_no_album: string;
  apple_template_file_playlist: string;
  apple_date_tag_template: string;
  apple_save_playlist: boolean;
  apple_overwrite: boolean;
  apple_language: string;
  apple_truncate: number;
  apple_exclude_tags: string;
  apple_log_level: string;
  apple_use_album_date: boolean;
  apple_fetch_extra_tags: boolean;
  apple_no_exceptions: boolean;
  apple_mv_enabled: boolean;
  apple_mv_codec_priority: string;
  apple_mv_remux_format: string;
  apple_mv_resolution: string;
  apple_uploaded_video_quality: string;
  apple_custom_paths_enabled: boolean;
  apple_nm3u8dlre_path: string;
  apple_mp4decrypt_path: string;
  apple_ffmpeg_path: string;
  apple_mp4box_path: string;
  apple_wvd_path: string;
  apple_use_wrapper: boolean;
  apple_wrapper_account_url: string;
  apple_wrapper_decrypt_ip: string;

  orpheus_dl_enabled_modules: string;
  orpheus_download_quality: string;
  orpheus_covers_enabled: boolean;
  orpheus_custom_modules: string;
}

export type SettingsSetter = (key: keyof Settings, value: unknown) => void;

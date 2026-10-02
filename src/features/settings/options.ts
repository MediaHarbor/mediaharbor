/// Option tables for the Settings field DSL: `[value, label]` pairs, `as const` so the
/// value side stays a literal union.

export const THEME_OPTS = [
  ['auto', 'Auto (System)'],
  ['dark', 'Dark'],
  ['light', 'Light'],
] as const;

export const CONVERSION_CODEC_OPTS = [
  ['FLAC', 'FLAC'],
  ['ALAC', 'ALAC'],
  ['MP3', 'MP3'],
  ['AAC', 'AAC'],
  ['OPUS', 'Opus'],
  ['VORBIS', 'Ogg Vorbis'],
] as const;

export const TIDAL_VIDEO_QUALITY_OPTS = [
  ['2160p', '2160p'],
  ['1080p', '1080p'],
  ['720p', '720p'],
  ['480p', '480p'],
] as const;

export const TIDAL_DEVICE_TYPE_OPTS = [
  ['phone', 'phone'],
  ['tablet', 'tablet'],
] as const;

export const SPOTIFY_SESSION_TYPE_OPTS = [
  ['librespot', 'Librespot'],
  ['desktop', 'Desktop'],
  ['web', 'Web'],
] as const;

export const SPOTIFY_AUDIO_DOWNLOAD_MODE_OPTS = [
  ['ytdlp', 'yt-dlp'],
  ['aria2c', 'aria2c'],
  ['curl', 'curl'],
  ['ytdlp', 'Buffered'],
  ['ffmpeg', 'Stream'],
] as const;

export const SPOTIFY_AUDIO_REMUX_MODE_OPTS = [
  ['ffmpeg', 'FFmpeg'],
  ['mp4box', 'MP4Box'],
  ['mp4decrypt', 'mp4decrypt'],
] as const;

export const SPOTIFY_COVER_SIZE_OPTS = [
  ['large', 'Large'],
  ['extra-large', 'Extra Large'],
  ['medium', 'Medium'],
  ['small', 'Small'],
] as const;

export const SPOTIFY_LOG_LEVEL_OPTS = [
  ['DEBUG', 'DEBUG'],
  ['INFO', 'INFO'],
  ['WARNING', 'WARNING'],
  ['ERROR', 'ERROR'],
] as const;

export const APPLE_DOWNLOAD_MODE_OPTS = [
  ['ytdlp', 'yt-dlp'],
  ['nm3u8dlre', 'N_m3u8DL-RE'],
] as const;

export const APPLE_COVER_FORMAT_OPTS = [
  ['jpg', 'JPG'],
  ['png', 'PNG'],
  ['raw', 'RAW'],
] as const;

export const APPLE_MV_CODEC_PRIORITY_OPTS = [
  ['h264', 'H.264'],
  ['h265', 'H.265'],
  ['ask', 'Ask'],
] as const;

export const APPLE_MV_REMUX_FORMAT_OPTS = [
  ['m4v', 'M4V'],
  ['mp4', 'MP4'],
] as const;

export const APPLE_MV_RESOLUTION_OPTS = [
  ['240p', '240p'],
  ['360p', '360p'],
  ['480p', '480p'],
  ['540p', '540p'],
  ['720p', '720p'],
  ['1080p', '1080p'],
  ['1440p', '1440p'],
  ['2160p', '2160p'],
] as const;

export const APPLE_UPLOADED_VIDEO_QUALITY_OPTS = [
  ['best', 'Best'],
  ['ask', 'Ask'],
] as const;

export const APPLE_LOG_LEVEL_OPTS = [
  ['DEBUG', 'Debug'],
  ['INFO', 'Info'],
  ['WARNING', 'Warning'],
  ['ERROR', 'Error'],
] as const;

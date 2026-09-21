import type { SearchType, QualityOption } from '@/types';

export const SEARCH_TYPE_LABELS: Record<SearchType, string> = {
  track: 'Tracks',
  album: 'Albums',
  playlist: 'Playlists',
  artist: 'Artists',
  video: 'Videos',
  channel: 'Channels',
  podcast: 'Podcasts',
  show: 'Shows',
  episode: 'Episodes',
  musicvideo: 'Music Videos',
  audiobook: 'Audiobooks',
};

export const SKIP_AWARE_PLATFORMS: ReadonlySet<string> = new Set([
  'spotify',
  'applemusic',
  'qobuz',
  'deezer',
  'tidal',
]);

/// Deezer stores its tier as the wire format name rather than the dialog's index,
/// so both directions are needed wherever the saved default meets an option list.
export const DEEZER_TIER_TO_FORMAT: Record<string, string> = {
  '2': 'FLAC',
  '1': 'MP3_320',
  '0': 'MP3_128',
};

const FORMAT_TO_DEEZER_TIER: Record<string, string> = Object.fromEntries(
  Object.entries(DEEZER_TIER_TO_FORMAT).map(([tier, format]) => [format, tier])
);

export function deezerTierOf(format: string | undefined): string {
  return FORMAT_TO_DEEZER_TIER[(format ?? '').toUpperCase()] ?? '2';
}

export const TIDAL_TAG_LABELS: Record<string, string> = {
  HIRES_LOSSLESS: 'Max',
  LOSSLESS: 'Lossless',
  DOLBY_ATMOS: 'Atmos',
  SONY_360RA: '360',
};

const SPOTIFY_NATIVE_QUALITY: QualityOption[] = [
  { value: 'aac-high', label: 'AAC 256 kbps (Premium)' },
  { value: 'aac-medium', label: 'AAC 128 kbps' },
];

const APPLEMUSIC_NATIVE_QUALITY: QualityOption[] = [
  { value: 'atmos', label: 'Dolby Atmos (E-AC-3, ~768 kbps)', group: 'Immersive' },
  { value: 'aac-256', label: 'AAC 256 kbps (Stereo)', group: 'Stereo (AAC)' },
  { value: 'aac-128', label: 'AAC 128 kbps (Stereo)', group: 'Stereo (AAC)' },
  { value: 'aac-he-64', label: 'HE-AAC 64 kbps (Stereo)', group: 'Stereo (AAC)' },
  { value: 'aac-256-binaural', label: 'AAC 256 kbps (Binaural)', group: 'Spatial renderings' },
  { value: 'aac-256-downmix', label: 'AAC 256 kbps (Downmix)', group: 'Spatial renderings' },
];

export const APPLEMUSIC_WRAPPER_QUALITY: QualityOption[] = [
  { value: 'alac', label: 'ALAC (Lossless)', group: 'Lossless' },
  { value: 'ac3', label: 'Dolby Digital (AC-3)', group: 'Immersive' },
  { value: 'aac', label: 'AAC (best available)', group: 'Stereo (AAC)' },
  { value: 'aac-he', label: 'HE-AAC (best available)', group: 'Stereo (AAC)' },
  { value: 'aac-binaural', label: 'AAC Binaural (best available)', group: 'Spatial renderings' },
  { value: 'aac-downmix', label: 'AAC Downmix (best available)', group: 'Spatial renderings' },
];

export const QUALITY_OPTIONS: Record<string, QualityOption[]> = {
  youtube: [
    { value: 'bestvideo+bestaudio', label: 'Best Quality' },
    { value: 'bestvideo[height<=2160][fps>30]+bestaudio', label: '4K60' },
    { value: 'bestvideo[height<=2160]+bestaudio', label: '4K' },
    { value: 'bestvideo[height<=1440][fps>30]+bestaudio', label: '2K60' },
    { value: 'bestvideo[height<=1440]+bestaudio', label: '2K' },
    { value: 'bestvideo[height<=1080][fps>30]+bestaudio', label: '1080p60' },
    { value: 'bestvideo[height<=1080]+bestaudio', label: '1080p' },
    { value: 'bestvideo[height<=720][fps>30]+bestaudio', label: '720p60' },
    { value: 'bestvideo[height<=720]+bestaudio', label: '720p' },
    { value: 'bestvideo[height<=480]+bestaudio', label: '480p' },
    { value: 'bestvideo[height<=360]+bestaudio', label: '360p' },
    { value: 'bestvideo[height<=240]+bestaudio', label: '240p' },
    { value: 'bestvideo[height<=144]+bestaudio', label: '144p' },
    { value: 'bestaudio', label: 'Best Audio' },
  ],
  qobuz: [
    { value: '27', label: 'format_id 27 — FLAC 24-bit (up to 192 kHz)', group: 'Lossless' },
    { value: '7', label: 'format_id 7 — FLAC 24-bit (up to 96 kHz)', group: 'Lossless' },
    { value: '6', label: 'format_id 6 — FLAC 16-bit / 44.1 kHz', group: 'Lossless' },
    { value: '5', label: 'format_id 5 — MP3 320 kbps', group: 'Lossy' },
  ],
  tidal: [
    { value: '3', label: 'HI_RES_LOSSLESS — FLAC 24-bit', group: 'Lossless' },
    { value: '2', label: 'LOSSLESS — FLAC 16-bit / 44.1 kHz', group: 'Lossless' },
    { value: '1', label: 'HIGH — AAC-LC ~320 kbps', group: 'Lossy' },
    { value: '0', label: 'LOW — HE-AAC ~96 kbps', group: 'Lossy' },
  ],
  deezer: [
    { value: '2', label: 'FLAC — 16-bit / 44.1 kHz', group: 'Lossless' },
    { value: '1', label: 'MP3_320 — 320 kbps', group: 'Lossy' },
    { value: '0', label: 'MP3_128 — 128 kbps', group: 'Lossy' },
  ],
  spotify_native: SPOTIFY_NATIVE_QUALITY,
  spotify_votify: [
    { value: 'flac-flac-24', label: 'FLAC 24-bit (Hi-Res)' },
    { value: 'flac-mp4-24', label: 'FLAC MP4 24-bit (Hi-Res)' },
    { value: 'flac-flac', label: 'FLAC (Lossless)' },
    { value: 'flac-mp4', label: 'FLAC MP4 (Lossless)' },
    { value: 'aac-high', label: 'AAC High' },
    { value: 'vorbis-high', label: 'Vorbis High' },
    { value: 'vorbis-medium', label: 'Vorbis Medium' },
    { value: 'aac-medium', label: 'AAC Medium' },
    { value: 'vorbis-low', label: 'Vorbis Low' },
  ],
  spotify: SPOTIFY_NATIVE_QUALITY,

  applemusic_native: APPLEMUSIC_NATIVE_QUALITY,
  applemusic_wrapper: APPLEMUSIC_WRAPPER_QUALITY,
  applemusic_gamdl: [
    { value: 'aac-web', label: 'AAC 256 kbps' },
    { value: 'aac-he-web', label: 'AAC-HE 64 kbps' },
    { value: 'alac', label: 'ALAC (Lossless)' },
    { value: 'aac', label: 'AAC (up to 48 kHz)' },
    { value: 'aac-he', label: 'AAC-HE' },
    { value: 'aac-binaural', label: 'AAC Binaural' },
    { value: 'aac-downmix', label: 'AAC Downmix' },
    { value: 'aac-he-binaural', label: 'AAC-HE Binaural' },
    { value: 'aac-he-downmix', label: 'AAC-HE Downmix' },
    { value: 'atmos', label: 'Dolby Atmos' },
    { value: 'ac3', label: 'AC-3' },
  ],
  applemusic: APPLEMUSIC_NATIVE_QUALITY,
  generic: [
    { value: 'bestvideo+bestaudio/best', label: 'Best Quality' },
    { value: 'bestvideo[height<=1080]+bestaudio/best[height<=1080]/best', label: '1080p' },
    { value: 'bestvideo[height<=960]+bestaudio/best[height<=960]/best', label: '960p' },
    { value: 'bestvideo[height<=720]+bestaudio/best[height<=720]/best', label: '720p' },
    { value: 'bestvideo[height<=540]+bestaudio/best[height<=540]/best', label: '540p' },
    { value: 'bestvideo[height<=480]+bestaudio/best[height<=480]/best', label: '480p' },
    { value: 'bestvideo[height<=360]+bestaudio/best[height<=360]/best', label: '360p' },
    { value: 'bestvideo[height<=240]+bestaudio/best[height<=240]/best', label: '240p' },
    { value: 'bestvideo[height<=144]+bestaudio/best[height<=144]/best', label: '144p' },
    { value: 'bestaudio/best', label: 'Best Audio' },
  ],
};

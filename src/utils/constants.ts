import type { SearchType, QualityOption } from '@/types';

export const SEARCH_TYPE_LABELS: Record<SearchType, string> = {
  track:      'Tracks',
  album:      'Albums',
  playlist:   'Playlists',
  artist:     'Artists',
  video:      'Videos',
  channel:    'Channels',
  podcast:    'Podcasts',
  show:       'Shows',
  episode:    'Episodes',
  musicvideo: 'Music Videos',
  audiobook:  'Audiobooks',
};

export const PLATFORM_IPC_NAME: Partial<Record<string, string>> = {
  youtubemusic: 'youtubeMusic',
};

export const PLAYLIST_PLATFORMS: string[] = ['youtube', 'youtubemusic', 'spotify'];

export const QUALITY_OPTIONS: Record<string, QualityOption[]> = {
  youtube: [
    { value: 'bestvideo+bestaudio',                          label: 'Best Quality' },
    { value: 'bestvideo[height<=2160][fps>30]+bestaudio',    label: '4K60' },
    { value: 'bestvideo[height<=2160]+bestaudio',            label: '4K' },
    { value: 'bestvideo[height<=1440][fps>30]+bestaudio',    label: '2K60' },
    { value: 'bestvideo[height<=1440]+bestaudio',            label: '2K' },
    { value: 'bestvideo[height<=1080][fps>30]+bestaudio',    label: '1080p60' },
    { value: 'bestvideo[height<=1080]+bestaudio',            label: '1080p' },
    { value: 'bestvideo[height<=720][fps>30]+bestaudio',     label: '720p60' },
    { value: 'bestvideo[height<=720]+bestaudio',             label: '720p' },
    { value: 'bestvideo[height<=480]+bestaudio',             label: '480p' },
    { value: 'bestvideo[height<=360]+bestaudio',             label: '360p' },
    { value: 'bestvideo[height<=240]+bestaudio',             label: '240p' },
    { value: 'bestvideo[height<=144]+bestaudio',             label: '144p' },
    { value: 'bestaudio',                                    label: 'Best Audio' },
  ],
  youtubemusic: [
    { value: '0', label: 'Best' },
    { value: '5', label: 'Meh' },
    { value: '9', label: 'Worst' },
  ],
  qobuz: [
    { value: '27', label: '24 bit, ≤ 192 kHz' },
    { value: '7',  label: '24 bit, ≤ 96 kHz' },
    { value: '6',  label: '16 bit, 44.1 kHz (CD)' },
    { value: '5',  label: '320 kbps MP3' },
  ],
  tidal: [
    { value: '3', label: '24 bit, ≤ 96 kHz (MQA)' },
    { value: '2', label: '16 bit, 44.1 kHz (CD)' },
    { value: '1', label: '320 kbps AAC' },
    { value: '0', label: '128 kbps AAC' },
  ],
  deezer: [
    { value: '2', label: '16 bit, 44.1 kHz (CD)' },
    { value: '1', label: '320 kbps MP3' },
    { value: '0', label: '128 kbps MP3' },
  ],
  spotify: [
    { value: 'aac-high',      label: 'AAC High' },
    { value: 'flac',           label: 'FLAC (Lossless)' },
    { value: 'vorbis-high',   label: 'Vorbis High' },
    { value: 'vorbis-medium', label: 'Vorbis Medium' },
    { value: 'aac-medium',    label: 'AAC Medium' },
    { value: 'vorbis-low',    label: 'Vorbis Low' },
  ],
  applemusic: [
    { value: 'aac-legacy',       label: 'AAC 256 kbps' },
    { value: 'aac-he-legacy',    label: 'AAC-HE 64 kbps' },
    { value: 'alac',             label: 'ALAC (Lossless)' },
    { value: 'aac',              label: 'AAC (High Efficiency)' },
    { value: 'aac-he',           label: 'AAC-HE' },
    { value: 'aac-binaural',     label: 'AAC Binaural' },
    { value: 'aac-downmix',      label: 'AAC Downmix' },
    { value: 'aac-he-binaural',  label: 'AAC-HE Binaural' },
    { value: 'aac-he-downmix',   label: 'AAC-HE Downmix' },
    { value: 'atmos',            label: 'Dolby Atmos' },
    { value: 'ac3',              label: 'AC-3' },
  ],
  generic: [
    { value: 'bestvideo+bestaudio', label: 'Best Quality' },
    { value: 'bestvideo[height<=1080]+bestaudio', label: '1080p' },
    { value: 'bestvideo[height<=960]+bestaudio',  label: '960p' },
    { value: 'bestvideo[height<=720]+bestaudio',  label: '720p' },
    { value: 'bestvideo[height<=540]+bestaudio',  label: '540p' },
    { value: 'bestvideo[height<=480]+bestaudio',  label: '480p' },
    { value: 'bestvideo[height<=360]+bestaudio',  label: '360p' },
    { value: 'bestvideo[height<=240]+bestaudio',  label: '240p' },
    { value: 'bestvideo[height<=144]+bestaudio',  label: '144p' },
    { value: 'bestaudio',                         label: 'Best Audio' },
  ],
};


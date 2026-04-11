import type { Platform, OrpheusPlatform, SearchType } from '@/types';

export const PLATFORM_COLORS: Record<string, string> = {
  spotify:      '#1ED760',
  tidal:        '#000000',
  deezer:       '#A238FF',
  qobuz:        '#323232',
  youtube:      '#FF0000',
  youtubemusic: '#FF0000',
  applemusic:   '#FA243C',
  soundcloud:   '#FF5500',
  napster:      '#1DA0C3',
  beatport:     '#01FF95',
  nugs:         '#E8232A',
  kkbox:        '#46D4C5',
  bugs:         '#FF5C35',
  idagio:       '#1A1A2E',
  jiosaavn:     '#2BC5B4',
};

export const PLATFORM_LABELS: Record<string, string> = {
  spotify:      'Spotify',
  tidal:        'Tidal',
  deezer:       'Deezer',
  qobuz:        'Qobuz',
  youtube:      'YouTube',
  youtubemusic: 'YouTube Music',
  applemusic:   'Apple Music',
  generic:      'Generic',
  soundcloud:   'SoundCloud',
  napster:      'Napster',
  beatport:     'Beatport',
  nugs:         'Nugs.net',
  kkbox:        'KKBox',
  bugs:         'Bugs! Music',
  idagio:       'Idagio',
  jiosaavn:     'JioSaavn',
};

export const PLATFORM_LIST: { value: Platform; label: string }[] = [
  { value: 'spotify',      label: 'Spotify' },
  { value: 'tidal',        label: 'Tidal' },
  { value: 'deezer',       label: 'Deezer' },
  { value: 'qobuz',        label: 'Qobuz' },
  { value: 'youtube',      label: 'YouTube' },
  { value: 'youtubemusic', label: 'YouTube Music' },
  { value: 'applemusic',   label: 'Apple Music' },
];

export const PLATFORM_SEARCH_TYPES: Record<Platform, SearchType[]> = {
  spotify:      ['track', 'album', 'playlist', 'artist', 'show', 'episode', 'audiobook'],
  tidal:        ['track', 'album', 'artist', 'playlist', 'video'],
  deezer:       ['track', 'album', 'playlist', 'artist', 'podcast'],
  qobuz:        ['track', 'album', 'playlist', 'artist'],
  youtube:      ['video', 'playlist', 'channel'],
  youtubemusic: ['track', 'album', 'playlist', 'artist', 'podcast'],
  applemusic:   ['track', 'album', 'artist', 'musicvideo'],
};

export function detectPlatform(url: string): Platform | OrpheusPlatform | 'generic' | null {
  if (!url) return null;
  const u = url.toLowerCase();
  if (u.includes('music.youtube.com'))               return 'youtubemusic';
  if (u.includes('youtube.com') || u.includes('youtu.be')) return 'youtube';
  if (u.includes('qobuz.com'))                       return 'qobuz';
  if (u.includes('tidal.com'))                       return 'tidal';
  if (u.includes('deezer.com'))                      return 'deezer';
  if (u.includes('spotify.com'))                     return 'spotify';
  if (u.includes('music.apple.com'))                 return 'applemusic';
  if (u.includes('soundcloud.com'))                  return 'soundcloud';
  if (u.includes('napster.com'))                     return 'napster';
  if (u.includes('beatport.com'))                    return 'beatport';
  if (u.includes('nugs.net'))                        return 'nugs';
  if (u.includes('kkbox.com'))                       return 'kkbox';
  if (u.includes('bugs.co.kr'))                      return 'bugs';
  if (u.includes('idagio.com'))                      return 'idagio';
  if (u.includes('jiosaavn.com') || u.includes('saavn.com')) return 'jiosaavn';
  return 'generic';
}

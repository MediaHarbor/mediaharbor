import type { Platform, OrpheusPlatform, SearchType, MediaKind } from '@/types';

export type { MediaKind };

/** The backend's own key for a service, as stored in `enabledServices`. */
export type ServiceKey =
  'spotify' | 'tidal' | 'qobuz' | 'deezer' | 'apple_music' | 'ytmusic' | 'youtube';

export type DownloadMethod =
  | 'startYouTubeMusic'
  | 'startYouTubeVideo'
  | 'startGenericVideo'
  | 'startSpotify'
  | 'startAppleMusic'
  | 'startQobuz'
  | 'startDeezer'
  | 'startTidal';

/**
 * Everything the frontend knows about one music service, in one place.
 *
 * The three key vocabularies are fields rather than three lookup tables:
 * `id` is what the client uses (`applemusic`), `serviceKey` is what the backend
 * stores (`apple_music`), `ipcKey` is what IPC expects (`youtubeMusic`).
 */
/** The path segment a service URL uses. Wider than `MediaKind`: Spotify has shows
 *  and episodes, which are not library media kinds. */
export type UrlKind = MediaKind | 'episode' | 'show';

export interface ServiceDescriptor {
  id: Platform;
  serviceKey: ServiceKey;
  ipcKey: string;
  /** Settings tab id, when it differs from the platform key. */
  settingsTab?: string;
  label: string;
  color: string;
  /** Needs credentials, so it stays hidden until the user opts in. */
  gated: boolean;
  searchTypes: SearchType[];
  /** `null` when the service has no library view. */
  library: { wip: boolean } | null;
  /** Extra spellings that should resolve to `id`. */
  aliases: string[];
  urlHosts: string[];
  downloadMethod: DownloadMethod;
  defaultQuality: string | number;
  externalUrl: (kind: UrlKind, id: string) => string | null;
  /** The playback-context URI the queue carries; only Spotify has the concept. */
  contextUri: ((kind: MediaKind, id: string) => string) | null;
  /**
   * The inverse of `externalUrl`: the service's own id, back out of one of its URLs.
   * `null` for services whose URLs carry no id we can report a play against.
   */
  idFromUrl: ((url: string, kind: UrlKind) => string | null) | null;
  /** Whether a finished play is reported back to the service. */
  reportsPlayback: boolean;
}

export const SERVICES: Record<Platform, ServiceDescriptor> = {
  spotify: {
    id: 'spotify',
    serviceKey: 'spotify',
    ipcKey: 'spotify',
    label: 'Spotify',
    color: '#1ED760',
    gated: true,
    searchTypes: ['track', 'album', 'playlist', 'artist', 'show', 'episode', 'audiobook'],
    library: { wip: false },
    aliases: [],
    urlHosts: ['spotify.com'],
    downloadMethod: 'startSpotify',
    defaultQuality: 'aac-high',
    externalUrl: (kind, id) => `https://open.spotify.com/${kind}/${id}`,
    contextUri: (kind, id) => `spotify:${kind}:${id}`,
    idFromUrl: (url, kind) =>
      url.match(
        new RegExp(`(?:open\\.spotify\\.com/${kind}/|spotify:${kind}:)([A-Za-z0-9]+)`)
      )?.[1] ?? null,
    reportsPlayback: true,
  },
  tidal: {
    id: 'tidal',
    serviceKey: 'tidal',
    ipcKey: 'tidal',
    label: 'Tidal',
    color: '#000000',
    gated: true,
    searchTypes: ['track', 'album', 'artist', 'playlist', 'video'],
    library: { wip: false },
    aliases: [],
    urlHosts: ['tidal.com'],
    downloadMethod: 'startTidal',
    defaultQuality: 3,
    externalUrl: (kind, id) => `https://tidal.com/browse/${kind}/${id}`,
    contextUri: null,
    idFromUrl: (url) => url.match(/tidal\.com\/track\/(\d+)/)?.[1] ?? null,
    reportsPlayback: true,
  },
  deezer: {
    id: 'deezer',
    serviceKey: 'deezer',
    ipcKey: 'deezer',
    label: 'Deezer',
    color: '#A238FF',
    gated: true,
    searchTypes: ['track', 'album', 'playlist', 'artist', 'podcast'],
    library: { wip: false },
    aliases: [],
    urlHosts: ['deezer.com'],
    downloadMethod: 'startDeezer',
    defaultQuality: 2,
    externalUrl: (kind, id) => `https://www.deezer.com/${kind}/${id}`,
    contextUri: null,
    idFromUrl: (url) => url.match(/deezer\.com\/track\/(\d+)/)?.[1] ?? null,
    reportsPlayback: true,
  },
  qobuz: {
    id: 'qobuz',
    serviceKey: 'qobuz',
    ipcKey: 'qobuz',
    label: 'Qobuz',
    color: '#323232',
    gated: true,
    searchTypes: ['track', 'album', 'playlist', 'artist'],
    library: { wip: false },
    aliases: [],
    urlHosts: ['qobuz.com'],
    downloadMethod: 'startQobuz',
    defaultQuality: 27,
    externalUrl: (kind, id) => `https://play.qobuz.com/${kind}/${id}`,
    contextUri: null,
    idFromUrl: (url) => url.match(/qobuz\.com\/track\/(\d+)/)?.[1] ?? null,
    reportsPlayback: true,
  },
  youtube: {
    id: 'youtube',
    serviceKey: 'youtube',
    ipcKey: 'youtube',
    label: 'YouTube',
    color: '#FF0000',
    gated: false,
    searchTypes: ['video', 'playlist', 'channel'],
    library: { wip: true },
    aliases: [],
    urlHosts: ['youtube.com', 'youtu.be'],
    downloadMethod: 'startYouTubeVideo',
    defaultQuality: 'best',
    externalUrl: (kind, id) =>
      kind === 'playlist'
        ? `https://www.youtube.com/playlist?list=${id}`
        : `https://www.youtube.com/watch?v=${id}`,
    contextUri: null,
    idFromUrl: null,
    reportsPlayback: false,
  },
  youtubemusic: {
    id: 'youtubemusic',
    serviceKey: 'ytmusic',
    ipcKey: 'youtubeMusic',
    settingsTab: 'youtube',
    label: 'YouTube Music',
    color: '#FF0000',
    gated: false,
    searchTypes: ['track', 'album', 'playlist', 'artist', 'podcast'],
    library: { wip: false },
    aliases: ['ytmusic', 'youtubeMusic', 'yt_music'],
    urlHosts: ['music.youtube.com'],
    downloadMethod: 'startYouTubeMusic',
    defaultQuality: '320K',
    externalUrl: (kind, id) => {
      if (kind === 'track') return `https://music.youtube.com/watch?v=${id}`;
      if (kind === 'playlist') return `https://music.youtube.com/playlist?list=${id}`;
      return `https://music.youtube.com/browse/${id}`;
    },
    contextUri: null,
    idFromUrl: (url) => url.match(/music\.youtube\.com\/watch\?v=([\w-]+)/)?.[1] ?? null,
    reportsPlayback: true,
  },
  applemusic: {
    id: 'applemusic',
    serviceKey: 'apple_music',
    ipcKey: 'applemusic',
    settingsTab: 'apple',
    label: 'Apple Music',
    color: '#FA243C',
    gated: true,
    searchTypes: ['track', 'album', 'artist', 'musicvideo'],
    library: { wip: false },
    aliases: ['apple_music', 'appleMusic', 'apple-music'],
    urlHosts: ['music.apple.com'],
    downloadMethod: 'startAppleMusic',
    defaultQuality: 'aac-256',
    externalUrl: (kind, id) => `https://music.apple.com/${kind}/${id}`,
    contextUri: null,
    idFromUrl: (url) =>
      url.match(/music\.apple\.com\/.*[?&]i=(\d+)/)?.[1] ??
      url.match(/music\.apple\.com\/(?:.*\/)?song\/(?:[^/]+\/)?(\d+)/)?.[1] ??
      null,
    reportsPlayback: true,
  },
};

/** Pseudo-platforms that are not services: on-disk files and arbitrary URLs. */
export const AUX_PLATFORMS = {
  local: { label: 'Local', color: '#9aa0a6' },
  generic: {
    label: 'Generic',
    downloadMethod: 'startGenericVideo' as DownloadMethod,
    defaultQuality: 'bestvideo+bestaudio/best' as string | number,
  },
} as const;

export const ORPHEUS_SERVICES: Record<
  OrpheusPlatform,
  { label: string; color: string; urlHosts: string[] }
> = {
  soundcloud: { label: 'SoundCloud', color: '#FF5500', urlHosts: ['soundcloud.com'] },
  napster: { label: 'Napster', color: '#1DA0C3', urlHosts: ['napster.com'] },
  beatport: { label: 'Beatport', color: '#01FF95', urlHosts: ['beatport.com'] },
  nugs: { label: 'Nugs.net', color: '#E8232A', urlHosts: ['nugs.net'] },
  kkbox: { label: 'KKBox', color: '#46D4C5', urlHosts: ['kkbox.com'] },
  bugs: { label: 'Bugs! Music', color: '#FF5C35', urlHosts: ['bugs.co.kr'] },
  idagio: { label: 'Idagio', color: '#1A1A2E', urlHosts: ['idagio.com'] },
  jiosaavn: { label: 'JioSaavn', color: '#2BC5B4', urlHosts: ['jiosaavn.com', 'saavn.com'] },
};

const ALL: ServiceDescriptor[] = Object.values(SERVICES);

/** Display order for the search platform picker. */
const SEARCH_ORDER: Platform[] = [
  'spotify',
  'tidal',
  'deezer',
  'qobuz',
  'youtube',
  'youtubemusic',
  'applemusic',
];

/** Display order for the library source picker; keyed by `serviceKey`. */
const LIBRARY_ORDER: Platform[] = [
  'tidal',
  'qobuz',
  'deezer',
  'spotify',
  'applemusic',
  'youtubemusic',
  'youtube',
];

/**
 * URL sniffing order. `music.youtube.com` must be tested before `youtube.com`,
 * so this is an explicit list rather than object order.
 */
const URL_DETECT_ORDER: (Platform | OrpheusPlatform)[] = [
  'youtubemusic',
  'youtube',
  'qobuz',
  'tidal',
  'deezer',
  'spotify',
  'applemusic',
  'soundcloud',
  'napster',
  'beatport',
  'nugs',
  'kkbox',
  'bugs',
  'idagio',
  'jiosaavn',
];

export const PLATFORM_COLORS: Record<string, string> = {
  local: AUX_PLATFORMS.local.color,
  ...Object.fromEntries(ALL.map((s) => [s.id, s.color])),
  ...Object.fromEntries(Object.entries(ORPHEUS_SERVICES).map(([id, o]) => [id, o.color])),
};

export const PLATFORM_LABELS: Record<string, string> = {
  local: AUX_PLATFORMS.local.label,
  generic: AUX_PLATFORMS.generic.label,
  ...Object.fromEntries(ALL.map((s) => [s.id, s.label])),
  ...Object.fromEntries(Object.entries(ORPHEUS_SERVICES).map(([id, o]) => [id, o.label])),
};

export const PLATFORM_SEARCH_TYPES: Record<Platform, SearchType[]> = Object.fromEntries(
  ALL.map((s) => [s.id, s.searchTypes])
) as Record<Platform, SearchType[]>;

const PLATFORM_LIST: { value: Platform; label: string }[] = SEARCH_ORDER.map((value) => ({
  value,
  label: SERVICES[value].label,
}));

export interface LibrarySource {
  value: ServiceKey;
  wip: boolean;
}

const LIBRARY_SOURCES: LibrarySource[] = LIBRARY_ORDER.filter(
  (p) => SERVICES[p].library !== null
).map((p) => ({ value: SERVICES[p].serviceKey, wip: SERVICES[p].library!.wip }));

/** The services a user opts into during onboarding, in display order. */
export const GATED_SERVICES: { platform: Platform; label: string }[] = SEARCH_ORDER.filter(
  (p) => SERVICES[p].gated
).map((p) => ({ platform: p, label: SERVICES[p].label }));

export function isGatedService(platform: string): boolean {
  const svc = SERVICES[toClientPlatform(platform)];
  return svc ? svc.gated : false;
}

export function searchPlatformsFor(
  enabledServices: string[]
): { value: Platform; label: string }[] {
  const enabled = new Set(enabledServices.map(toClientPlatform));
  return PLATFORM_LIST.filter((p) => !SERVICES[p.value].gated || enabled.has(p.value));
}

export function librarySourcesFor(enabledServices: string[]): LibrarySource[] {
  const enabled = new Set(enabledServices.map((s) => normalizePlatform(s)));
  const alwaysOn = new Set(ALL.filter((s) => !s.gated).map((s) => s.serviceKey));
  return LIBRARY_SOURCES.filter((s) => alwaysOn.has(s.value) || enabled.has(s.value));
}

const CLIENT_ALIASES: Record<string, Platform> = Object.fromEntries(
  ALL.flatMap((s) => [s.id, ...s.aliases].map((a) => [a, s.id]))
);

export function toClientPlatform(s: string): Platform {
  return CLIENT_ALIASES[s] ?? (s as Platform);
}

export function isKnownPlatform(s: string): boolean {
  return s in CLIENT_ALIASES;
}

/** The Settings tab that owns a platform's credentials. */
export function settingsTabOf(s: string): string {
  const client = toClientPlatform(s);
  return SERVICES[client]?.settingsTab ?? client;
}

export function normalizePlatform(s: string): string {
  const client = toClientPlatform(s);
  return SERVICES[client]?.serviceKey ?? client;
}

export function toIpcPlatform(s: string): string {
  const client = toClientPlatform(s);
  return SERVICES[client]?.ipcKey ?? client;
}

const SERVICE_SOURCE_KEYS = new Set<string>(['local', ...ALL.map((s) => s.serviceKey)]);

export function serviceSourceOf(s: string | null | undefined): string | null {
  if (!s) return null;
  const v = s === 'local' ? 'local' : normalizePlatform(s);
  return SERVICE_SOURCE_KEYS.has(v) ? v : null;
}

export function detectPlatform(url: string): Platform | OrpheusPlatform | 'generic' | null {
  if (!url) return null;
  const u = url.toLowerCase();
  for (const id of URL_DETECT_ORDER) {
    const hosts =
      id in SERVICES
        ? SERVICES[id as Platform].urlHosts
        : ORPHEUS_SERVICES[id as OrpheusPlatform].urlHosts;
    if (hosts.some((h) => u.includes(h))) return id;
  }
  return 'generic';
}

export function contextUriFor(
  platform: string,
  kind: MediaKind,
  id?: string | null
): string | null {
  if (!id) return null;
  return SERVICES[toClientPlatform(platform)]?.contextUri?.(kind, id) ?? null;
}

/**
 * The inverse of `externalUrl`: which service a URL belongs to, and its id there.
 *
 * Tries every descriptor rather than taking the platform as an argument, because the
 * callers that need it (scrobbling, episode lookup) have a URL from a restored queue
 * row and may not have a trustworthy platform beside it.
 */
export function serviceIdFromUrl(
  url: string,
  kind: UrlKind = 'track'
): { platform: Platform; id: string } | null {
  for (const svc of ALL) {
    const id = svc.idFromUrl?.(url, kind);
    if (id) return { platform: svc.id, id };
  }
  return null;
}

export function externalUrl(opts: {
  platform: string;
  kind: UrlKind;
  id?: string | null;
  fallback?: string | null;
}): string | null {
  const { kind, id } = opts;
  if (opts.fallback && /^https?:\/\//i.test(opts.fallback)) return opts.fallback;
  if (!id) return null;
  return SERVICES[toClientPlatform(opts.platform)]?.externalUrl(kind, id) ?? null;
}

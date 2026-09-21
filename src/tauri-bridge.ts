import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Settings } from '@/types/settings';
import type { MediaKind, SearchResult, SearchType } from '@/types';

interface DownloadInfoPayload {
  order: number;
  title?: string;
  artist?: string;
  uploader?: string;
  album?: string;
  thumbnail?: string | null;
  platform?: string;
  quality?: string;
}

interface DownloadProgressPayload {
  order: number;
  progress: number;
  title?: string;
  thumbnail?: string | null;
  artist?: string;
  album?: string;
  speed?: string | null;
  eta?: string | null;
  itemIndex?: number | null;
  itemTotal?: number | null;
  currentTrack?: string | null;
  quality?: string | null;
}

interface DownloadCompletePayload {
  order: number;
  title?: string;
  warnings?: string;
  location?: string;
  fullLog?: string;
}

interface DownloadErrorPayload {
  order: number;
  error: string;
  fullLog: string;
  title?: string;
}

interface DownloadFailure {
  id: string;
  label: string;
  reason: string;
}

interface DownloadSummaryPayload {
  download_id: number;
  succeeded: number;
  skipped: number;
  failed: number;
  total: number;
  failures: DownloadFailure[];
  dest_dir: string | null;
}

export interface WrapperProbeResult {
  reachable: boolean;
  authenticated: boolean;
  needsTwoFactor: boolean;
  state: string;
  playbackReady: boolean;
  version: string;
  runtime: string;
  appleId: string | null;
  error: string | null;
}

interface StreamReadyPayload {
  streamUrl: string;
  platform: string;
  durationSec?: number;
  mediaType: 'audio' | 'video';
  isLive?: boolean;
  audioStreamUrl?: string;
}

interface InstallProgressPayload {
  dependency: string;
  percent: number;
  status: string;
}

/** What the native player knew about a stream it could not decode. */
export interface UndecodableStream {
  /** The URL the player was handed, for matching back to the queued track. */
  url: string;
  detail: string;
  container: string | null;
  codec: string | null;
  sampleRate: number | null;
  channels: number | null;
  app: string;
}

interface UndecodableStreamWire {
  url: string;
  detail: string;
  container: string | null;
  codec: string | null;
  sample_rate: number | null;
  channels: number | null;
  app: string;
}
interface ScanProgressPayload {
  directory: string;
  percent: number;
  status: string;
  progress?: number;
  currentFile?: string;
}

interface LibraryScanProgressPayload {
  directory: string;
  done: number;
  total: number;
  current_path: string | null;
  status: string;
}

interface FilesChangedPayload {
  directory: string;
  added?: number;
  updated?: number;
  removed?: number;
}

interface AppErrorPayload {
  message: string;
  context?: string;
  needs_auth?: string;
}

interface BackendLogPayload {
  level: string;
  source: string;
  title: string;
  message: string;
  timestamp: string;
}

interface DownloadData {
  url: string;
  outputDir?: string;
  quality?: string | null;
  title?: string | null;
  artist?: string | null;
  uploader?: string | null;
  album?: string | null;
  thumbnail?: string | null;
  platform?: string;
  forceRedownload?: boolean;
}

interface SpotifyProfile {
  name?: string;
  plan?: string;
  email?: string;
  id?: string;
  [key: string]: unknown;
}

export function isSpotifyFree(profile: SpotifyProfile | null | undefined): boolean {
  const plan = profile?.plan?.toLowerCase();
  return plan === 'free' || plan === 'open';
}

export type SaveKindStr = MediaKind;

export interface MutationCapabilities {
  save_tracks: boolean;
  save_albums: boolean;
  follow_artists: boolean;
  follow_playlists: boolean;
  create_playlists: boolean;
  edit_playlists: boolean;
  reorder_playlists: boolean;
  radio: boolean;
}

export type RecommendationCategory =
  | 'hero'
  | 'daily_mix'
  | 'discovery'
  | 'recently_played'
  | 'new_releases'
  | 'charts'
  | 'genre'
  | 'editorial'
  | 'stations'
  | 'other';

export interface ServiceCapabilities {
  albums: boolean;
  tracks: boolean;
  artists: boolean;
  playlists: boolean;
  videos: boolean;
  recommendations: boolean;
  followers: boolean;
  activity_feed: boolean;
  episode_bookmarks: boolean;
  mutations: MutationCapabilities;
}

export interface AlbumDetail {
  album: Record<string, unknown>;
  tracks: Record<string, unknown>[];
}

export interface ArtistDetail {
  key: string;
  display: string;
  albums: Record<string, unknown>[];
  tracks: Record<string, unknown>[];
}

export interface PlaylistDetail {
  playlist: Record<string, unknown>;
  tracks: Record<string, unknown>[];
}

export interface Shelf {
  id: string;
  title: string;
  subtitle: string | null;
  category: RecommendationCategory;
  items: Array<Record<string, unknown> & { kind: string }>;
}

/** Every shelf-returning command answers with this envelope. */
export interface ShelvesPage {
  shelves: Shelf[];
}

export interface OwnedPlaylistRow {
  platform: string;
  serviceId: string;
  name: string;
  coverId: string | null;
  trackCount: number;
  updatedAt: number;
}

export interface PlaylistMutateResult {
  playlist_id: string;
  library_id: string | null;
  snapshot_id: string | null;
}

export interface RadioResult {
  seed_kind: 'track' | 'album' | 'artist' | 'playlist';
  seed_id: string;
  title: string;
  tracks: Record<string, unknown>[];
  station_id: string | null;
  video_urls?: Record<string, string>;
  /// Token for the next page of an endless station; absent when it has run out.
  continuation?: string | null;
}

export interface ResolvedLink {
  platform: string;
  kind: SearchType;
  id: string;
}

function makeHub<T>() {
  const subs = new Set<(data: T) => void>();
  return {
    on(cb: (data: T) => void): () => void {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    emit(data: T) {
      subs.forEach((cb) => cb(data));
    },
  };
}

const infoHub = makeHub<DownloadInfoPayload>();
const progressHub = makeHub<DownloadProgressPayload>();
const completeHub = makeHub<DownloadCompletePayload>();
const errorHub = makeHub<DownloadErrorPayload>();

const summaries = new Map<number, DownloadSummaryPayload>();

function registerDownloadEvents() {
  listen<{
    download_id: number;
    title?: string;
    artist?: string;
    album?: string;
    thumbnail?: string | null;
    platform?: string;
    quality?: string;
  }>('download-info', (event) => {
    const { download_id, ...meta } = event.payload;
    infoHub.emit({ order: download_id, ...meta });
  });

  listen<DownloadSummaryPayload>('download-summary', (event) => {
    summaries.set(event.payload.download_id, event.payload);
  });

  listen<{
    download_id: number;
    percent: number;
    speed: string | null;
    eta: string | null;
    status: string;
    item_index: number | null;
    item_total: number | null;
    quality?: string | null;
  }>('download-progress', (event) => {
    const { download_id, percent, status } = event.payload;
    const order = download_id;

    if (status === 'completed') {
      const summary = summaries.get(order);
      summaries.delete(order);
      completeHub.emit({
        order,
        location: summary?.dest_dir ?? undefined,
        ...summaryWarnings(summary),
      });
    } else if (status.startsWith('error:')) {
      summaries.delete(order);
      const msg = status.slice(6).trim();
      errorHub.emit({ order, error: msg, fullLog: msg });
    } else {
      const { speed, eta, item_index, item_total, quality } = event.payload;
      const currentTrack = status && status !== 'downloading' ? status : null;
      progressHub.emit({
        order,
        progress: Math.round(percent),
        speed,
        eta,
        itemIndex: item_index,
        itemTotal: item_total,
        currentTrack,
        quality,
      });
    }
  });
}

function summaryWarnings(
  summary: DownloadSummaryPayload | undefined
): Pick<DownloadCompletePayload, 'warnings' | 'fullLog'> {
  if (!summary || summary.failed === 0) return {};
  const headline = `${summary.succeeded} of ${summary.total} tracks downloaded — ${summary.failed} failed`;
  return {
    warnings: headline,
    fullLog: [headline, ...summary.failures.map((f) => `✗ ${f.label}: ${f.reason}`)].join('\n'),
  };
}

const streamReadyHub = makeHub<StreamReadyPayload>();
const installProgressHub = makeHub<InstallProgressPayload>();
const scanProgressHub = makeHub<ScanProgressPayload>();
const filesChangedHub = makeHub<FilesChangedPayload>();
const appErrorHub = makeHub<AppErrorPayload>();
const backendLogHub = makeHub<BackendLogPayload>();
const stdinPromptHub = makeHub<{ downloadId: number; promptLines: string[] }>();
const radioMetadataHub = makeHub<RadioMetadataPayload>();

export interface RadioMetadataPayload {
  stationUuid: string;
  title: string;
}

export type RadioStreamKind = 'direct' | 'hls' | 'playlistFile';

/** The one station shape every directory answers with. */
export interface RadioStation {
  /** `"{source}:{sourceId}"` — the identity used everywhere, including playback. */
  key: string;
  source: string;
  sourceId: string;
  name: string;
  streamUrl: string;
  streamKind: RadioStreamKind;
  altUrls: string[];
  homepage?: string | null;
  favicon?: string | null;
  tags?: string | null;
  country?: string | null;
  countryCode?: string | null;
  language?: string | null;
  codec?: string | null;
  bitrate?: number | null;
  votes?: number | null;
  clickcount?: number | null;
  /** A cover the user supplied, resolved through `library.coverUrl`. */
  coverId?: string | null;
  headers: Record<string, string>;
}

export type RadioFacetKind = 'tags' | 'countries' | 'languages' | 'codecs';

/** Only the fields the user changed; everything absent still tracks the directory. */
export interface RadioStationOverrides {
  name?: string;
  streamUrl?: string;
  streamKind?: RadioStreamKind;
  altUrls?: string[];
  favicon?: string;
  tags?: string;
  country?: string;
  countryCode?: string;
  language?: string;
  codec?: string;
  bitrate?: number;
  homepage?: string;
}

/** An edit as the editor sends it. The backend stores the diff against `base`. */
export interface RadioStationEdit extends RadioStationOverrides {
  headers?: Record<string, string>;
}

export interface RadioStationDetail {
  /** The station as the directory published it. */
  base: RadioStation;
  overrides: RadioStationOverrides;
}

export interface RadioStreamProbe {
  ok: boolean;
  status: number;
  contentType?: string | null;
  codec?: string | null;
  bitrate?: number | null;
  icyName?: string | null;
  /** `icy-genre`, which becomes the imported station's tags. */
  genre?: string | null;
  streamKind: RadioStreamKind;
  message?: string | null;
}

export interface ParsedCurl {
  url: string;
  headers: Record<string, string>;
}

export interface RadioFacet {
  /** What a query filters on — an ISO code for a country. */
  name: string;
  /** What a human reads. */
  label: string;
  stationCount: number;
}

export interface RadioDirectoryCapabilities {
  /** Which browse lists this directory can fill. */
  facets: RadioFacetKind[];
  /** Whether asking for a second page returns anything new. */
  paginates: boolean;
}

export interface RadioDirectorySource {
  id: string;
  label: string;
  capabilities: RadioDirectoryCapabilities;
  enabled: boolean;
  toggleable: boolean;
  /** `loading` while a directory is building its first mirror. */
  status: 'ready' | 'loading';
}

export interface RadioList {
  id: number;
  name: string;
  createdAt: number;
  updatedAt: number;
  stationCount: number;
  favicons: string[];
}

export interface RadioListDetail extends RadioList {
  stations: RadioStation[];
}

export interface RadioSearchRequest {
  name?: string;
  tag?: string;
  countryCode?: string;
  language?: string;
  codec?: string;
  bitrateMin?: number;
  order?: string;
  limit?: number;
  offset?: number;
  sources?: string[];
}

export interface SavedStateChangedPayload {
  platform: string;
  kind: 'track' | 'album' | 'artist' | 'playlist';
  added: string[];
  removed: string[];
}
export interface ServicePlaylistChangedPayload {
  platform: string;
  playlistId: string;
  change: 'created' | 'renamed' | 'deleted' | 'tracks_added' | 'tracks_removed' | 'reordered';
  snapshotId: string | null;
}
const savedStateHub = makeHub<SavedStateChangedPayload>();
const servicePlaylistHub = makeHub<ServicePlaylistChangedPayload>();

export type CredentialStatus =
  | { kind: 'notConfigured' }
  | { kind: 'ok'; expiresAt: number | null }
  | { kind: 'expiringSoon'; expiresAt: number }
  | { kind: 'expired'; since: number | null }
  | { kind: 'unknown'; lastCheck: number; error: string };

export interface CredentialStatusChangedPayload {
  platform: string;
  status: CredentialStatus;
  /** Hash of the credential material; changes when the user rotates it. */
  fingerprint?: string | null;
}

export interface CredentialsHealthSnapshot {
  services: Record<string, CredentialStatus>;
  fingerprints?: Record<string, string>;
  generatedAt: number;
}
const credentialStatusHub = makeHub<CredentialStatusChangedPayload>();

function bindHub<T>(event: string, hub: { emit: (data: T) => void }) {
  listen<T>(event, (e) => hub.emit(e.payload));
}

const lastPercentByDir = new Map<string, number>();

function registerAppEvents() {
  bindHub<StreamReadyPayload>('stream-ready', streamReadyHub);
  bindHub<InstallProgressPayload>('install-progress', installProgressHub);
  bindHub<ScanProgressPayload>('scan-progress', scanProgressHub);
  listen<LibraryScanProgressPayload>('library-scan-progress', (e) => {
    const p = e.payload;
    let percent: number;
    if (p.status === 'done') {
      percent = 100;
      lastPercentByDir.delete(p.directory);
    } else if (p.status === 'scanning') {
      // Emitted before the walk, whose duration is unknown; the bar shows activity
      // rather than sitting at zero looking hung.
      percent = 0;
      lastPercentByDir.delete(p.directory);
    } else if (p.total > 0) {
      percent = Math.round((p.done / p.total) * 100);
      const prev = lastPercentByDir.get(p.directory) ?? 0;
      if (percent < prev) percent = prev;
      lastPercentByDir.set(p.directory, percent);
    } else {
      return;
    }
    scanProgressHub.emit({
      directory: p.directory,
      percent,
      status: p.status,
      progress: percent,
      currentFile: p.current_path ?? '',
    });
  });
  bindHub<FilesChangedPayload>('library-changed', filesChangedHub);
  bindHub<RadioMetadataPayload>('radio-metadata', radioMetadataHub);
  bindHub<AppErrorPayload>('app-error', appErrorHub);
  bindHub<BackendLogPayload>('backend-log', backendLogHub);
  bindHub<{ downloadId: number; promptLines: string[] }>('process-stdin-prompt', stdinPromptHub);
  bindHub<SavedStateChangedPayload>('saved-state-changed', savedStateHub);
  bindHub<ServicePlaylistChangedPayload>('service-playlist-changed', servicePlaylistHub);
  bindHub<CredentialStatusChangedPayload>('credential-status-changed', credentialStatusHub);
}

let bridgeStarted = false;

/// Subscribes the hubs above to their backend events. Called once from
/// `main.tsx`, never at module scope: registering there made merely *importing*
/// this file call `listen()`, which touches `window`, so every Node-side
/// importer threw — the Vitest suite reaches it through `features/library/api.ts`.
export function initTauriEventBridge(): void {
  if (bridgeStarted) return;
  bridgeStarted = true;
  registerDownloadEvents();
  registerAppEvents();
}

async function getOutputDir(): Promise<string> {
  try {
    const resp = await invoke<{ settings: Settings }>('get_settings');
    return resp?.settings?.downloadLocation ?? '';
  } catch {
    return '';
  }
}

function subscribe<P>(event: string, cb: (payload: P) => void) {
  const unlistenP = listen<P>(event, (e) => cb(e.payload));
  return () => {
    unlistenP.then((u) => u()).catch(() => {});
  };
}

function makeLoginWrapper<P>(openCmd: string, capturedEvent: string) {
  return {
    openLoginWindow: async () => {
      await invoke(openCmd);
    },
    onLoginCaptured: (cb: (payload: P) => void) => subscribe<P>(capturedEvent, cb),
  };
}

const setSaved = (platform: string, kind: SaveKindStr, saved: boolean, ids: (string | number)[]) =>
  invoke<SavedStateChangedPayload>('service_library_set_saved', {
    req: { platform, kind, saved, ids: ids.map(String) },
  });

const playlistReq = (
  req: { platform: string; id: string | number },
  extra: Record<string, unknown> = {}
) => ({ platform: req.platform, id: String(req.id), ...extra });

const intQuality = (data: DownloadData) =>
  data.quality != null ? parseInt(String(data.quality), 10) : null;
const strQuality = (data: DownloadData) => (data.quality != null ? String(data.quality) : null);

/** `cmd(platform)` — the shape of every per-service read that needs no other args. */
const byPlatform =
  <Res>(cmd: string) =>
  (platform: string) =>
    invoke<Res>(cmd, { req: { platform } });

/**
 * `cmd(platform, id)` — the same, plus one id. `idKey` covers the commands whose
 * Rust request names that field something other than `id`.
 */
const byPlatformId =
  <Res>(cmd: string, idKey = 'id') =>
  (platform: string, id: string) =>
    invoke<Res>(cmd, { req: { platform, [idKey]: id } });

const makeDownloadInvoker =
  (cmd: string, build: (data: DownloadData) => Record<string, unknown>) =>
  async (data: DownloadData) => {
    const outputDir = data.outputDir || (await getOutputDir());
    invoke(cmd, {
      req: {
        url: data.url,
        outputDir,
        title: data.title ?? null,
        artist: data.artist ?? null,
        album: data.album ?? null,
        thumbnail: data.thumbnail ?? null,
        forceRedownload: data.forceRedownload ?? false,
        ...build(data),
      },
    }).catch(() => {});
  };

export const tauriAPI = {
  updates: {
    getVersion: async () => {
      const r = await invoke<{ version: string }>('get_version');
      return r.version;
    },
    check: async () => {
      const r = await invoke<{
        update_available: boolean;
        latest_version: string | null;
        release_url: string | null;
        release_notes: string | null;
      }>('check_updates');
      return {
        hasUpdate: r.update_available,
        currentVersion: '',
        latestVersion: r.latest_version ?? '',
        releaseNotes: r.release_notes ?? '',
        releaseUrl: r.release_url ?? '',
        publishedAt: '',
      };
    },
    openRelease: async (url: string) => {
      await invoke('open_external', { url });
    },
    checkDeps: async () => invoke<Record<string, boolean>>('check_deps'),
    getDependencyVersions: async () => {
      const r = await invoke<{ versions: Record<string, string> }>('get_dependency_versions');
      return r.versions;
    },
    installDep: async (dep: string, force = false) => {
      const r = await invoke<{ success: boolean; error: string | null }>('install_dep', {
        req: { dependency: dep, force },
      });
      if (!r.success) throw new Error(r.error ?? 'Installation failed');
      return { success: true };
    },
    onInstallProgress: installProgressHub.on.bind(installProgressHub),
  },

  search: {
    perform: async (params: {
      platform: string;
      query: string;
      type: string;
      offset?: number;
      limit?: number;
    }) => {
      return invoke<{ results: SearchResult[]; platform: string }>('perform_search', {
        req: params,
      });
    },
    suggestions: byPlatformId<string[]>('search_suggestions', 'query'),
    resolveShareLink: async (url: string) => {
      return invoke<ResolvedLink>('resolve_share_link', { url });
    },
  },

  downloads: {
    startYouTubeMusic: makeDownloadInvoker('start_yt_music_download', (d) => ({
      quality: d.quality ?? null,
      artist: d.artist ?? d.uploader ?? null,
      platform: 'youtubemusic',
    })),
    startYouTubeVideo: makeDownloadInvoker('start_yt_video_download', (d) => ({
      resolution: d.quality ?? null,
      format: null,
      artist: d.artist ?? d.uploader ?? null,
      platform: 'youtube',
    })),
    startGenericVideo: makeDownloadInvoker('start_yt_video_download', (d) => ({
      resolution: d.quality ?? null,
      format: null,
      isGeneric: true,
      artist: d.artist ?? d.uploader ?? null,
      platform: d.platform ?? 'generic',
    })),
    startSpotify: makeDownloadInvoker('start_spotify_download', (d) => ({
      quality: strQuality(d),
      platform: 'spotify',
    })),
    startAppleMusic: makeDownloadInvoker('start_apple_download', (d) => ({
      quality: strQuality(d),
      platform: 'applemusic',
    })),
    startQobuz: makeDownloadInvoker('start_qobuz_download', (d) => ({
      quality: intQuality(d),
      platform: 'qobuz',
    })),
    startDeezer: makeDownloadInvoker('start_deezer_download', (d) => ({
      quality: intQuality(d),
      platform: 'deezer',
    })),
    startTidal: makeDownloadInvoker('start_tidal_download', (d) => ({
      quality: intQuality(d),
      platform: 'tidal',
    })),
    startOrpheus: makeDownloadInvoker('start_orpheus_download', (d) => ({
      moduleId: d.platform,
    })),
    cancel: (order: number) => {
      invoke('cancel_download', { req: { downloadId: order } }).catch(() => {});
    },
    showItemInFolder: async (filePath: string) => {
      const r = await invoke<{ success: boolean }>('show_item_in_folder', {
        req: { path: filePath },
      });
      return r.success;
    },
    onProgress: progressHub.on.bind(progressHub),
    onInfo: infoHub.on.bind(infoHub),
    onComplete: completeHub.on.bind(completeHub),
    onError: errorHub.on.bind(errorHub),
  },

  settings: {
    get: async () => {
      const r = await invoke<{ settings: Settings }>('get_settings');
      return r.settings;
    },
    set: async (settings: Settings) => {
      const r = await invoke<{ success: boolean; error: string | null }>('set_settings', {
        req: { settings },
      });
      return r;
    },
    openFolder: async () => {
      const r = await invoke<{ path: string | null }>('dialog_open_folder');
      return r.path ?? null;
    },
    openFile: async () => {
      const r = await invoke<{ path: string | null }>('dialog_open_file');
      return r.path ?? null;
    },
  },

  qobuz: makeLoginWrapper<{ userId: string }>('qobuz_open_login_window', 'qobuz-login-captured'),

  radio: {
    sources: async () => invoke<RadioDirectorySource[]>('radio_sources'),
    setSources: async (sources: string[]) => {
      await invoke('radio_set_sources', { req: { sources } });
    },
    search: async (req: RadioSearchRequest) => invoke<RadioStation[]>('radio_search', { req }),
    facets: async (kind: RadioFacetKind, limit?: number, source?: string) =>
      invoke<RadioFacet[]>('radio_facets', { req: { kind, limit, source } }),
    suggest: async (prefix: string, limit?: number) =>
      invoke<RadioFacet[]>('radio_suggest', { req: { prefix, limit } }),
    station: async (key: string) => invoke<RadioStation | null>('radio_station', { req: { key } }),
    favorites: async () => invoke<RadioStation[]>('radio_favorites'),
    recent: async () => invoke<RadioStation[]>('radio_recent'),
    setFavorite: async (key: string, favorite: boolean) => {
      await invoke('radio_set_favorite', { req: { key, favorite } });
    },
    forget: async (key: string) => {
      await invoke('radio_forget', { req: { key } });
    },
    lists: async () => invoke<RadioList[]>('radio_lists'),
    listCreate: async (name: string) =>
      invoke<{ id: number }>('radio_list_create', { req: { name } }),
    listRename: async (id: number, name: string) => {
      await invoke('radio_list_rename', { req: { id, name } });
    },
    listDelete: async (id: number) => {
      await invoke('radio_list_delete', { req: { id } });
    },
    listGet: async (id: number) =>
      invoke<RadioListDetail | null>('radio_list_get', { req: { id } }),
    listAdd: async (id: number, keys: string[]) => {
      await invoke('radio_list_add', { req: { id, keys } });
    },
    listRemove: async (id: number, position: number) => {
      await invoke('radio_list_remove', { req: { id, position } });
    },
    listReorder: async (id: number, from: number, to: number) => {
      await invoke('radio_list_reorder', { req: { id, from, to } });
    },
    importUrl: async (url: string, name?: string, headers?: Record<string, string>) =>
      invoke<RadioStation>('radio_import_url', { req: { url, name, headers } }),
    stationDetail: async (key: string) =>
      invoke<RadioStationDetail | null>('radio_station_detail', { req: { key } }),
    updateStation: async (key: string, edit: RadioStationEdit) => {
      await invoke('radio_update_station', { req: { key, edit } });
    },
    resetStation: async (key: string) => {
      await invoke('radio_reset_station', { req: { key } });
    },
    setCover: async (key: string, jpegBase64: string | null) => {
      await invoke('radio_set_cover', { req: { key, jpegBase64 } });
    },
    testStream: async (url: string, headers?: Record<string, string>) =>
      invoke<RadioStreamProbe>('radio_test_stream', { req: { url, headers } }),
    /** Accepts a bare stream URL or a whole `curl` command copied from DevTools. */
    parseCurl: async (input: string) => invoke<ParsedCurl>('radio_parse_curl', { req: { input } }),
    importPlaylist: async (path: string) =>
      invoke<RadioStation>('radio_import_playlist', { req: { path } }),
    importIcecast: async (host: string) =>
      invoke<RadioStation[]>('radio_import_icecast', { req: { host } }),
    saveStations: async (stations: RadioStation[]) => {
      await invoke('radio_save_stations', { req: { stations } });
    },
    onMetadata: (cb: (e: { stationUuid: string; title: string }) => void) =>
      radioMetadataHub.on(cb),
  },
  library: {
    scanIncremental: async (directory: string, force = false) => {
      await invoke('library_scan', { req: { directory, force } });
    },
    query: async (req: {
      kind?: 'albums' | 'videos' | 'tracks' | 'artists' | 'playlists';
      offset?: number;
      limit?: number;
      sort?: string;
      search?: string;
    }) => {
      return invoke<{ items: Record<string, unknown>[]; total: number }>('library_query', { req });
    },
    getAlbum: async (album_key: string) => {
      return invoke<{
        album: Record<string, unknown>;
        tracks: Record<string, unknown>[];
      } | null>('library_album', { req: { album_key } });
    },
    getArtist: async (key: string) => {
      return invoke<{
        key: string;
        display: string;
        albums: Record<string, unknown>[];
        tracks: Record<string, unknown>[];
      } | null>('library_artist', { req: { key } });
    },
    setWatch: async (roots: string[]) => {
      await invoke('library_set_watch', { req: { roots } });
    },
    coverUrl: async (cover_id: string) => {
      return invoke<string | null>('library_cover_url', { req: { cover_id } });
    },
    writeTags: async (req: Record<string, unknown> & { path: string }) => {
      await invoke('library_write_tags', { req });
    },
    radio: async (path: string) =>
      invoke<Record<string, unknown>[]>('library_radio', { req: { path } }),
    duplicates: async () => invoke<Record<string, unknown>[][]>('library_duplicates'),
    recordPlay: async (path: string) => {
      await invoke('library_record_play', { req: { path } });
    },
    showItemInFolder: async (filePath: string) => {
      const r = await invoke<{ success: boolean }>('show_item_in_folder', {
        req: { path: filePath },
      });
      return r.success;
    },
    playlists: {
      create: async (name: string) => {
        const r = await invoke<{ id: number }>('library_playlist_create', { req: { name } });
        return r.id;
      },
      rename: async (id: number, name: string) => {
        await invoke('library_playlist_rename', { req: { id, name } });
      },
      delete: async (id: number) => {
        await invoke('library_playlist_delete', { req: { id } });
      },
      get: async (id: number) => {
        return invoke<{
          playlist: Record<string, unknown>;
          tracks: Record<string, unknown>[];
        } | null>('library_playlist_get', { req: { id } });
      },
      addTracks: async (id: number, paths: string[]) => {
        await invoke('library_playlist_add_tracks', { req: { id, paths } });
      },
      removeTrack: async (id: number, position: number) => {
        await invoke('library_playlist_remove_track', { req: { id, position } });
      },
      reorder: async (id: number, from: number, to: number) => {
        await invoke('library_playlist_reorder', { req: { id, from, to } });
      },
      importM3u: async (source: string) => {
        const r = await invoke<{ id: number }>('library_playlist_import_m3u', { req: { source } });
        return r.id;
      },
      exportM3u: async (id: number, dest: string) => {
        await invoke('library_playlist_export_m3u', { req: { id, dest } });
      },
    },
    onScanProgress: scanProgressHub.on.bind(scanProgressHub),
    onFilesChanged: filesChangedHub.on.bind(filesChangedHub),
  },

  normalizeCoverImage: (bytes: Uint8Array) =>
    invoke<string>('normalize_cover_image', { bytes: Array.from(bytes) }),

  serviceLibrary: {
    capabilities: byPlatform<ServiceCapabilities>('service_library_capabilities'),
    query: async (req: {
      platform: string;
      kind: 'albums' | 'tracks' | 'artists' | 'playlists' | 'videos';
      offset?: number;
      limit?: number;
    }) => {
      return invoke<{
        items: Record<string, unknown>[];
        total: number | null;
        nextCursor?: string | null;
      }>('service_library_query', { req });
    },
    recommendations: byPlatform<ShelvesPage>('service_library_recommendations'),
    explore: byPlatform<ShelvesPage>('service_library_explore'),
    explorePage: byPlatformId<ShelvesPage>('service_library_explore_page'),
    activityFeed: byPlatform<Record<string, unknown>>('service_library_activity_feed'),
    albumPage: byPlatformId<Record<string, unknown>>('service_library_album_page'),
    artistPage: byPlatformId<Record<string, unknown>>('service_library_artist_page'),
    episodeBookmarks: byPlatform<Record<string, unknown>[]>('service_library_episode_bookmarks'),
    followers: byPlatform<Record<string, unknown>[]>('service_library_followers'),
    following: byPlatform<Record<string, unknown>[]>('service_library_following'),
    album: byPlatformId<AlbumDetail>('service_library_album'),
    artist: byPlatformId<ArtistDetail>('service_library_artist'),
    playlist: byPlatformId<PlaylistDetail>('service_library_playlist'),

    savedStateFor: (platform: string, kind: SaveKindStr) =>
      invoke<string[]>('service_library_saved_state_for', { req: { platform, kind } }),
    refreshSavedState: (platform: string, kinds: SaveKindStr[]) =>
      invoke<void>('service_library_saved_state_refresh', { req: { platform, kinds } }),
    ownedPlaylists: (platform: string) =>
      invoke<OwnedPlaylistRow[]>('service_library_owned_playlists', { req: { platform } }),

    setSaved,
    reportPlayback: (
      platform: string,
      id: string,
      durationSecs: number,
      context?: { contextUri: string; trackIndex: number }
    ) =>
      invoke<void>('service_library_report_playback', {
        req: {
          platform,
          id,
          durationSecs: Math.round(durationSecs),
          contextUri: context?.contextUri ?? null,
          trackIndex: context?.trackIndex ?? null,
        },
      }),
    canvas: byPlatformId<string | null>('service_library_canvas'),
    setCover: (platform: string, id: string, jpegBase64: string) =>
      invoke<void>('service_library_set_cover', { req: { platform, id, jpegBase64 } }),
    transcript: byPlatformId<{
      episodeName?: string;
      showName?: string;
      language?: string;
      lines: { startMs: number; speaker?: string | null; text: string }[];
    } | null>('service_library_transcript'),
    followUser: byPlatformId<void>('service_library_follow_user'),
    unfollowUser: byPlatformId<void>('service_library_unfollow_user'),

    playlists: {
      create: (req: {
        platform: string;
        name: string;
        description?: string;
        isPublic?: boolean;
        isCollaborative?: boolean;
        initialTrackIds?: (string | number)[];
      }) =>
        invoke<PlaylistMutateResult>('service_library_playlist_create', {
          req: {
            platform: req.platform,
            name: req.name,
            description: req.description ?? null,
            isPublic: req.isPublic ?? false,
            isCollaborative: req.isCollaborative ?? false,
            initialTrackIds: (req.initialTrackIds ?? []).map(String),
          },
        }),
      rename: (req: {
        platform: string;
        id: string | number;
        name: string;
        description?: string;
        isPublic?: boolean;
        isCollaborative?: boolean;
      }) =>
        invoke<void>('service_library_playlist_rename', {
          req: playlistReq(req, {
            name: req.name,
            description: req.description ?? null,
            isPublic: req.isPublic ?? null,
            isCollaborative: req.isCollaborative ?? null,
          }),
        }),
      delete: (req: { platform: string; id: string | number }) =>
        invoke<void>('service_library_playlist_delete', { req: playlistReq(req) }),
      addTracks: (req: { platform: string; id: string | number; trackIds: (string | number)[] }) =>
        invoke<PlaylistMutateResult>('service_library_playlist_add_tracks', {
          req: playlistReq(req, { trackIds: req.trackIds.map(String), positions: null }),
        }),
      removeTracks: (req: {
        platform: string;
        id: string | number;
        trackIds: (string | number)[];
        positions?: number[];
      }) =>
        invoke<PlaylistMutateResult>('service_library_playlist_remove_tracks', {
          req: playlistReq(req, {
            trackIds: req.trackIds.map(String),
            positions: req.positions ?? null,
          }),
        }),
      reorder: (req: { platform: string; id: string | number; from: number; to: number }) =>
        invoke<PlaylistMutateResult>('service_library_playlist_reorder', {
          req: playlistReq(req, { from: req.from, to: req.to }),
        }),
    },

    radioFor: (req: {
      platform: string;
      seedKind: 'track' | 'album' | 'artist' | 'playlist';
      seedId: string | number;
    }) =>
      invoke<RadioResult>('service_library_radio_for', {
        req: { platform: req.platform, seedKind: req.seedKind, seedId: String(req.seedId) },
      }),

    radioContinue: (req: { platform: string; continuation: string }) =>
      invoke<RadioResult>('service_library_radio_continue', {
        req: { platform: req.platform, continuation: req.continuation },
      }),

    onSavedStateChanged: savedStateHub.on.bind(savedStateHub),
    onServicePlaylistChanged: servicePlaylistHub.on.bind(servicePlaylistHub),
  },

  credentials: {
    snapshot: () => invoke<CredentialsHealthSnapshot>('credentials_health_snapshot'),
    recheck: (platform?: string) =>
      invoke<CredentialsHealthSnapshot>('credentials_health_recheck', {
        platform: platform ?? null,
      }),
    onStatusChanged: credentialStatusHub.on.bind(credentialStatusHub),
  },

  player: {
    playMedia: async (params: { url: string; platform: string }) => {
      const r = await invoke<{
        stream_url: string;
        platform: string;
        duration_sec: number | null;
        media_type: string | null;
        is_live: boolean;
        audio_stream_url: string | null;
      }>('play_media', { req: params });
      const result = {
        streamUrl: r.stream_url,
        platform: r.platform,
        durationSec: r.duration_sec ?? undefined,
        mediaType: (r.media_type ?? 'audio') as 'audio' | 'video',
        isLive: r.is_live ?? false,
        audioStreamUrl: r.audio_stream_url ?? undefined,
      };
      streamReadyHub.emit(result);
      return result;
    },
    prefetchMedia: async (params: { url: string; platform: string }) => {
      const r = await invoke<{
        stream_url: string;
        platform: string;
        duration_sec: number | null;
        media_type: string | null;
        is_live: boolean;
        audio_stream_url: string | null;
      }>('play_media', { req: params });
      return {
        streamUrl: r.stream_url,
        platform: r.platform,
        durationSec: r.duration_sec ?? undefined,
        mediaType: (r.media_type ?? 'audio') as 'audio' | 'video',
        isLive: r.is_live ?? false,
        audioStreamUrl: r.audio_stream_url ?? undefined,
      };
    },
    pause: async () => {
      await invoke('pause_media');
    },
    onStreamReady: streamReadyHub.on.bind(streamReadyHub),

    native: {
      load: (url: string, mimeType?: string | null, sourcePath?: string | null) =>
        invoke<void>('player_load', {
          req: { url, mime_type: mimeType ?? null, source_path: sourcePath ?? null },
        }),
      play: () => invoke<void>('player_play'),
      pause: () => invoke<void>('player_pause'),
      stop: () => invoke<void>('player_stop'),
      seek: (positionSecs: number) =>
        invoke<void>('player_seek', { req: { position_secs: positionSecs } }),
      crossfadeTo: (url: string, durationSecs: number, mimeType?: string | null) =>
        invoke<void>('player_crossfade_to', {
          req: { url, mime_type: mimeType ?? null, duration_secs: durationSecs },
        }),
      setVolume: (volume: number) => invoke<void>('player_set_volume', { req: { volume } }),
      setMuted: (muted: boolean) => invoke<void>('player_set_muted', { req: { muted } }),
      setSpectrumEnabled: (enabled: boolean) =>
        invoke<void>('player_set_spectrum_enabled', { req: { enabled } }),
      listDevices: async () => {
        const r = await invoke<{ devices: string[] }>('player_list_devices');
        return r.devices;
      },
      onPosition: (cb: (p: { positionSecs: number; durationSecs?: number }) => void) =>
        subscribe<{ position_secs: number; duration_secs: number | null }>('player-position', (p) =>
          cb({ positionSecs: p.position_secs, durationSecs: p.duration_secs ?? undefined })
        ),
      onState: (cb: (s: { playing: boolean; ended: boolean; buffering: boolean }) => void) =>
        subscribe<{ playing: boolean; ended: boolean; buffering: boolean }>('player-state', cb),
      onError: (cb: (e: { message: string; undecodable?: UndecodableStream }) => void) =>
        subscribe<{ message: string; undecodable?: UndecodableStreamWire }>('player-error', (p) =>
          cb({
            message: p.message,
            undecodable: p.undecodable
              ? {
                  url: p.undecodable.url,
                  detail: p.undecodable.detail,
                  container: p.undecodable.container ?? null,
                  codec: p.undecodable.codec ?? null,
                  sampleRate: p.undecodable.sample_rate ?? null,
                  channels: p.undecodable.channels ?? null,
                  app: p.undecodable.app,
                }
              : undefined,
          })
        ),
      onSpectrum: (cb: (bars: number[]) => void) =>
        subscribe<{ bars: number[] }>('audio-spectrum', (p) => cb(p.bars)),
      /** Fires when the decode thread has actually handed over to the queued
       *  track — `crossfadeTo` resolves a whole fade earlier than that. */
      onTrackChanged: (cb: (p: { atSecs: number }) => void) =>
        subscribe<{ at_secs: number }>('player-track-changed', (p) => cb({ atSecs: p.at_secs })),
    },
    setMediaMetadata: (meta: {
      title?: string | null;
      artist?: string | null;
      album?: string | null;
      coverUrl?: string | null;
      durationSecs?: number | null;
    }) => {
      void invoke('media_set_metadata', { meta }).catch(() => {});
    },
    setMediaPlayback: (playback: { playing: boolean; positionSecs?: number | null }) => {
      void invoke('media_set_playback', { playback }).catch(() => {});
    },
    onMediaControl: (cb: (payload: { action: string; seconds?: number }) => void) => {
      const unlisten = listen<{ action: string; seconds?: number }>('media-control', (e) =>
        cb(e.payload)
      );
      return () => {
        void unlisten.then((u) => u());
      };
    },
  },

  spotifyAccount: {
    login: async () => {
      const r = await invoke<{ profile: SpotifyProfile }>('spotify_oauth_login');
      return r.profile;
    },
    logout: () => invoke('spotify_oauth_logout'),
    getStatus: async () => {
      const r = await invoke<{ logged_in: boolean; profile: SpotifyProfile | null }>(
        'spotify_oauth_status'
      );
      return { loggedIn: r.logged_in, profile: r.profile };
    },
    getToken: async () => {
      const r = await invoke<{ token: string | null }>('spotify_get_token');
      return r.token;
    },
  },

  tidal: {
    ...makeLoginWrapper<{
      userId: string;
      countryCode: string;
      accessToken: string;
      refreshToken: string;
      expiryTime: number;
    }>('tidal_open_login_window', 'tidal-login-captured'),
    importToken: async (tokenJson: string) => {
      const r = await invoke<{
        user_id: string;
        country_code: string;
        expiry_time: number;
        access_token: string;
        refresh_token: string;
      }>('tidal_import_token', { req: { tokenJson } });
      return {
        userId: r.user_id,
        countryCode: r.country_code,
        expiryTime: r.expiry_time,
        accessToken: r.access_token,
        refreshToken: r.refresh_token,
      };
    },
    onLoginError: (cb: (payload: { error: string }) => void) =>
      subscribe<{ error: string }>('tidal-login-error', cb),
  },

  app: {
    /**
     * Opens a URL in the system browser.
     *
     * `window.open` is inert inside the Tauri WebView — it is why every
     * "open website" in the app silently did nothing — so this goes through the
     * shell plugin instead.
     */
    openExternal: async (url: string) => {
      await invoke('open_external', { url });
    },
    onError: appErrorHub.on.bind(appErrorHub),
    onBackendLog: backendLogHub.on.bind(backendLogHub),
    onStdinPrompt: stdinPromptHub.on.bind(stdinPromptHub),
    sendProcessStdin: async (downloadId: number, input: string) => {
      await invoke('send_process_stdin', { req: { downloadId, input } });
    },
    probeAppleWrapper: (signIn: boolean, code?: string, signOut = false) =>
      invoke<WrapperProbeResult>('probe_apple_wrapper', {
        req: { signIn, signOut, code: code ?? null },
      }),
  },

  orpheus: {
    checkDeps: () =>
      invoke<{
        orpheus_installed: boolean;
        modules: Array<{ id: string; label: string; installed: boolean }>;
      }>('check_orpheus_deps'),
    installCore: () =>
      invoke<{ success: boolean; error: string | null }>('install_dep', {
        req: { dependency: 'orpheus' },
      }),
    installModule: (moduleId: string, customUrl?: string, label?: string) =>
      invoke<{ success: boolean; error: string | null }>('install_orpheus_module', {
        req: { moduleId, customUrl: customUrl ?? null, label: label ?? null },
      }),
    readSettings: () => invoke<string>('read_orpheus_settings'),
    writeSettings: (content: string) => invoke<void>('write_orpheus_settings', { content }),
    onInstallProgress: installProgressHub.on.bind(installProgressHub),
  },

  lyrics: {
    get: async (req: {
      url: string;
      platform: string;
      title: string;
      artist: string;
      duration?: number;
    }) => {
      return invoke<{ synced: string | null; plain: string | null; wordSynced: string | null }>(
        'get_lyrics',
        { req }
      );
    },
  },
};

/// Whether the Tauri backend is actually reachable. `tauriAPI` always exists as
/// an object, so calling into it is what fails outside the desktop shell — this
/// is the only honest way to ask, and the reason the old `window.electron`
/// truthiness checks never fired.
export function isBackendAvailable(): boolean {
  return isTauri();
}

(window as unknown as { electron: typeof tauriAPI }).electron = tauriAPI;

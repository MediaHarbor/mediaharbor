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
}

interface FilesChangedPayload {
  directory: string;
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
const scanProgressHub   = makeHub<ScanProgressPayload>();
const filesChangedHub   = makeHub<FilesChangedPayload>();
const appErrorHub = makeHub<AppErrorPayload>();
const backendLogHub = makeHub<BackendLogPayload>();
const stdinPromptHub = makeHub<{ downloadId: number; promptLines: string[] }>();

function bindHub<T>(event: string, hub: { emit: (data: T) => void }) {
  listen<T>(event, (e) => hub.emit(e.payload));
}

function registerAppEvents() {
  bindHub<StreamReadyPayload>('stream-ready', streamReadyHub);
  bindHub<InstallProgressPayload>('install-progress', installProgressHub);
  bindHub<AppErrorPayload>('app-error', appErrorHub);
  bindHub<BackendLogPayload>('backend-log', backendLogHub);
  bindHub<{ downloadId: number; promptLines: string[] }>('process-stdin-prompt', stdinPromptHub);
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
    checkDeps: async () => {
      const r = await invoke<Record<string, boolean>>('check_deps');
      return {
        ...r,
        ytdlp: r['yt_dlp'] ?? false,
        apple: r['gamdl'] ?? false,
        spotify: r['votify'] ?? false,
      };
    },
    getDependencyVersions: async (_packages: string[]) => {
      const r = await invoke<{ versions: Record<string, string> }>('get_dependency_versions');
      return r.versions;
    },
    getBinaryVersions: async () => {
      const r = await invoke<{ versions: Record<string, string> }>('get_dependency_versions');
      return {
        python: r.versions['python'] ?? '',
        ffmpeg: r.versions['ffmpeg'] ?? '',
      };
    },
    installDep: async (dep: string) => {
      const r = await invoke<{ success: boolean; error: string | null }>('install_dep', {
        req: { dependency: dep },
      });
      if (!r.success) throw new Error(r.error ?? 'Installation failed');
      return { success: true };
    },
    updateDependencies: (_packages: string[]) => {
    },
    onInstallProgress: installProgressHub.on.bind(installProgressHub),
    onDependencyNotification: (_cb: (data: Record<string, unknown>) => void) => () => {},
    onDependencyLoading:      (_cb: (isLoading: boolean) => void) => () => {},
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
    getAlbumDetails: async (platform: string, albumId: string) => {
      try {
        const r = await invoke<{ data: Record<string, unknown> }>('get_album_details', {
          req: { albumId, platform },
        });
        return { success: true, data: r.data };
      } catch (e: unknown) {
        return { success: false, error: String(e) };
      }
    },
    getPlaylistDetails: async (platform: string, playlistId: string) => {
      try {
        const r = await invoke<{ data: Record<string, unknown> }>('get_playlist_details', {
          req: { playlistId, platform },
        });
        return { success: true, data: r.data };
      } catch (e: unknown) {
        return { success: false, error: String(e) };
      }
    },
    getArtistDetails: async (platform: string, artistId: string) => {
      try {
        const r = await invoke<{ data: Record<string, unknown> }>('get_artist_details', {
          req: { artistId, platform },
        });
        return { success: true, data: r.data };
      } catch (e: unknown) {
        return { success: false, error: String(e) };
      }
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

  library: {
    scan: async (directory: string, force = false) => {
      return invoke<Record<string, unknown>[]>('scan_directory', { req: { directory, force } });
    },
    showItemInFolder: async (filePath: string) => {
      const r = await invoke<{ success: boolean }>('show_item_in_folder', {
        req: { path: filePath },
      });
      return r.success;
    },
    onScanProgress:  scanProgressHub.on.bind(scanProgressHub),
    onFilesChanged:  filesChangedHub.on.bind(filesChangedHub),
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

  tidalAuth: {
    startAuth: async () => {
      const r = await invoke<{ code_verifier: string; auth_url: string }>('tidal_start_auth');
      return { codeVerifier: r.code_verifier, authUrl: r.auth_url };
    },
    exchangeCode: async (data: { redirectUrl: string; codeVerifier: string }) => {
      const r = await invoke<{
        access_token: string;
        refresh_token: string;
        expires_in: number;
        user_id: string;
        country_code: string;
      }>('tidal_exchange_code', {
        req: { redirectUrl: data.redirectUrl, codeVerifier: data.codeVerifier },
      });
      const expiry = String(Math.floor(Date.now() / 1000) + (r.expires_in ?? 86400));
      return {
        tidal_access_token: r.access_token,
        tidal_refresh_token: r.refresh_token ?? '',
        tidal_token_expiry: expiry,
        tidal_user_id: r.user_id ?? '',
        tidal_country_code: r.country_code ?? 'US',
      };
    },
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
      invoke<{ orpheus_installed: boolean; modules: Array<{ id: string; label: string; installed: boolean }> }>(
        'check_orpheus_deps'
      ),
    installCore: () =>
      invoke<{ success: boolean; error: string | null }>('install_dep', { req: { dependency: 'orpheus' } }),
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

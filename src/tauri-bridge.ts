
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

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

const infoHub     = makeHub<any>();
const progressHub = makeHub<any>();
const completeHub = makeHub<any>();
const errorHub    = makeHub<any>();

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

listen<{
  download_id: number;
  percent: number;
  speed: string | null;
  eta: string | null;
  status: string;
  item_index: number | null;
  item_total: number | null;
}>('download-progress', (event) => {
  const { download_id, percent, status } = event.payload;
  const order = download_id;

  if (status === 'completed') {
    completeHub.emit({ order });
  } else if (status.startsWith('error:')) {
    const msg = status.slice(6).trim();
    errorHub.emit({ order, error: msg, fullLog: msg });
  } else {
    progressHub.emit({ order, progress: Math.round(percent) });
  }
});

const streamReadyHub    = makeHub<any>();
const installProgressHub = makeHub<any>();
const scanProgressHub   = makeHub<any>();
const filesChangedHub   = makeHub<any>();
const appErrorHub       = makeHub<any>();
const backendLogHub     = makeHub<any>();

listen<any>('stream-ready',     (e) => streamReadyHub.emit(e.payload));
listen<any>('install-progress', (e) => installProgressHub.emit(e.payload));
listen<any>('scan-progress',    (e) => scanProgressHub.emit(e.payload));
listen<any>('library-changed',  (e) => filesChangedHub.emit(e.payload));
listen<any>('app-error',        (e) => appErrorHub.emit(e.payload));
listen<any>('backend-log',      (e) => backendLogHub.emit(e.payload));

async function getOutputDir(): Promise<string> {
  try {
    const resp = await invoke<any>('get_settings');
    return resp?.settings?.downloadLocation ?? '';
  } catch {
    return '';
  }
}

const tauriAPI = {

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
        git:    r.versions['git']    ?? '',
        ffmpeg: r.versions['ffmpeg'] ?? '',
      };
    },
    installDep: async (dep: string) => {
      const r = await invoke<{ success: boolean; error: string | null }>('install_dep', {
        req: { dependency: dep },
      });
      return { success: r.success };
    },
    updateDependencies: (_packages: string[]) => {
    },
    onInstallProgress: installProgressHub.on.bind(installProgressHub),
    onDependencyNotification: (_cb: (data: any) => void) => () => {},
    onDependencyLoading:      (_cb: (isLoading: boolean) => void) => () => {},
  },

  search: {
    perform: async (params: { platform: string; query: string; type: string }) => {
      return invoke<{ results: any; platform: string }>('perform_search', { req: params });
    },
    getAlbumDetails: async (platform: string, albumId: string) => {
      try {
        const r = await invoke<{ data: any }>('get_album_details', {
          req: { albumId, platform },
        });
        return { success: true, data: r.data };
      } catch (e: any) {
        return { success: false, error: String(e) };
      }
    },
    getPlaylistDetails: async (platform: string, playlistId: string) => {
      try {
        const r = await invoke<{ data: any }>('get_playlist_details', {
          req: { playlistId, platform },
        });
        return { success: true, data: r.data };
      } catch (e: any) {
        return { success: false, error: String(e) };
      }
    },
    getArtistDetails: async (platform: string, artistId: string) => {
      try {
        const r = await invoke<{ data: any }>('get_artist_details', {
          req: { artistId, platform },
        });
        return { success: true, data: r.data };
      } catch (e: any) {
        return { success: false, error: String(e) };
      }
    },
  },

  downloads: {
    startYouTubeMusic: async (data: any) => {
      const outputDir = data.outputDir || await getOutputDir();
      invoke('start_yt_music_download', {
        req: {
          url: data.url, outputDir, quality: data.quality ?? null,
          title: data.title ?? null,
          artist: data.artist ?? data.uploader ?? null,
          album: data.album ?? null,
          thumbnail: data.thumbnail ?? null,
          platform: 'youtubemusic',
        },
      }).catch(() => {});
    },
    startYouTubeVideo: async (data: any) => {
      const outputDir = data.outputDir || await getOutputDir();
      invoke('start_yt_video_download', {
        req: {
          url: data.url, outputDir, resolution: data.quality ?? null, format: null,
          title: data.title ?? null,
          artist: data.artist ?? data.uploader ?? null,
          album: data.album ?? null,
          thumbnail: data.thumbnail ?? null,
          platform: 'youtube',
        },
      }).catch(() => {});
    },
    startGenericVideo: async (data: any) => {
      const outputDir = data.outputDir || await getOutputDir();
      invoke('start_yt_video_download', {
        req: {
          url: data.url, outputDir, resolution: data.quality ?? null, format: null,
          title: data.title ?? null,
          artist: data.artist ?? data.uploader ?? null,
          album: data.album ?? null,
          thumbnail: data.thumbnail ?? null,
          platform: data.platform ?? 'youtube',
        },
      }).catch(() => {});
    },
    startSpotify: async (data: any) => {
      const outputDir = data.outputDir || await getOutputDir();
      invoke('start_spotify_download', {
        req: {
          url: data.url, outputDir,
          title: data.title ?? null,
          artist: data.artist ?? null,
          album: data.album ?? null,
          thumbnail: data.thumbnail ?? null,
          platform: 'spotify',
        },
      }).catch(() => {});
    },
    startAppleMusic: async (data: any) => {
      const outputDir = data.outputDir || await getOutputDir();
      invoke('start_apple_download', {
        req: {
          url: data.url, outputDir,
          title: data.title ?? null,
          artist: data.artist ?? null,
          album: data.album ?? null,
          thumbnail: data.thumbnail ?? null,
          platform: 'applemusic',
        },
      }).catch(() => {});
    },
    startQobuz: async (data: any) => {
      const outputDir = data.outputDir || await getOutputDir();
      const quality = data.quality != null ? parseInt(String(data.quality), 10) : null;
      invoke('start_qobuz_download', {
        req: {
          url: data.url, outputDir, quality,
          title: data.title ?? null,
          artist: data.artist ?? null,
          album: data.album ?? null,
          thumbnail: data.thumbnail ?? null,
          platform: 'qobuz',
        },
      }).catch(() => {});
    },
    startDeezer: async (data: any) => {
      const outputDir = data.outputDir || await getOutputDir();
      const quality = data.quality != null ? parseInt(String(data.quality), 10) : null;
      invoke('start_deezer_download', {
        req: {
          url: data.url, outputDir, quality,
          title: data.title ?? null,
          artist: data.artist ?? null,
          album: data.album ?? null,
          thumbnail: data.thumbnail ?? null,
          platform: 'deezer',
        },
      }).catch(() => {});
    },
    startTidal: async (data: any) => {
      const outputDir = data.outputDir || await getOutputDir();
      invoke('start_tidal_download', {
        req: {
          url: data.url, outputDir,
          title: data.title ?? null,
          artist: data.artist ?? null,
          album: data.album ?? null,
          thumbnail: data.thumbnail ?? null,
          platform: 'tidal',
        },
      }).catch(() => {});
    },
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
    onInfo:     infoHub.on.bind(infoHub),
    onComplete: completeHub.on.bind(completeHub),
    onError:    errorHub.on.bind(errorHub),
  },

  settings: {
    get: async () => {
      const r = await invoke<{ settings: any }>('get_settings');
      return r.settings;
    },
    set: async (settings: any) => {
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
      return invoke<any[]>('scan_directory', { req: { directory, force } });
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
      }>('play_media', { req: params });
      const result = {
        streamUrl: r.stream_url,
        platform: r.platform,
        durationSec: r.duration_sec ?? undefined,
        mediaType: (r.media_type ?? 'audio') as 'audio' | 'video',
      };
      streamReadyHub.emit(result);
      return result;
    },
    pause: async () => {
      await invoke('pause_media');
    },
    onStreamReady: streamReadyHub.on.bind(streamReadyHub),
  },

  spotifyAccount: {
    login: () => invoke('spotify_oauth_login'),
    logout: () => invoke('spotify_oauth_logout'),
    getStatus: async () => {
      const r = await invoke<{ logged_in: boolean; profile: any }>('spotify_oauth_status');
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
    onError:      appErrorHub.on.bind(appErrorHub),
    onBackendLog: backendLogHub.on.bind(backendLogHub),
  },
};

(window as any).electron = tauriAPI;

import { contextBridge, ipcRenderer, IpcRendererEvent } from 'electron';

export interface ElectronAPI {
  updates: {
    getVersion: () => Promise<string>;
    check: () => Promise<{ hasUpdate: boolean; currentVersion: string; latestVersion: string; releaseNotes: string; releaseUrl: string; publishedAt: string }>;
    openRelease: (url: string) => Promise<void>;
    checkDeps: () => Promise<Record<string, boolean>>;
    getDependencyVersions: (packages: string[]) => Promise<Record<string, string>>;
    getBinaryVersions: () => Promise<{ python: string; git: string; ffmpeg: string }>;
    installDep: (dep: string) => Promise<{ success: boolean }>;
    onInstallProgress: (callback: (data: { dependency: string; percent: number; status: string }) => void) => () => void;
  };
  search: {
    perform: (params: { platform: string; query: string; type: string }) => Promise<{ results: any[]; platform: string }>;
    getAlbumDetails: (platform: string, albumId: string) => Promise<{ success: boolean; data?: any; error?: string }>;
    getPlaylistDetails: (platform: string, playlistId: string) => Promise<{ success: boolean; data?: any; error?: string }>;
    getArtistDetails: (platform: string, artistId: string) => Promise<{ success: boolean; data?: any; error?: string }>;
  };
  downloads: {
    startYouTubeMusic: (data: any, playlist?: any) => void;
    startYouTubeVideo: (data: any) => void;
    startSpotify: (command: any) => void;
    startAppleMusic: (command: any) => void;
    startQobuz: (data: any) => void;
    startDeezer: (data: any) => void;
    startTidal: (data: any) => void;
    startGenericVideo: (data: any) => void;
    onProgress: (callback: (data: any) => void) => () => void;
    onInfo: (callback: (data: any) => void) => () => void;
    onComplete: (callback: (data: any) => void) => () => void;
    onError: (callback: (data: any) => void) => () => void;
    cancel: (order: number) => void;
    showItemInFolder: (filePath: string) => Promise<boolean>;
  };
  settings: {
    get: () => Promise<any>;
    set: (settings: any) => Promise<{ success: boolean; error?: string }>;
    openFolder: () => Promise<string | null>;
    openFile: () => Promise<string | null>;
  };
  library: {
    scan: (directory: string, force?: boolean) => Promise<any[]>;
    onScanProgress: (callback: (data: any) => void) => () => void;
    onFilesChanged: (callback: (data: { added: any[]; removedPaths: string[] }) => void) => () => void;
    showItemInFolder: (filePath: string) => Promise<boolean>;
  };
  player: {
    playMedia: (params: { url: string; platform: string }) => Promise<{ streamUrl: string; platform: string }>;
    pause: () => Promise<void>;
    onStreamReady: (callback: (data: { streamUrl: string; platform: string; durationSec?: number; mediaType?: 'audio' | 'video' }) => void) => () => void;
  };
  spotifyAccount: {
    login: () => Promise<any>;
    logout: () => Promise<any>;
    getStatus: () => Promise<{ loggedIn: boolean; profile: any }>;
    getToken: () => Promise<string | null>;
  };
  tidalAuth: {
    startAuth: () => Promise<any>;
    exchangeCode: (data: { redirectUrl: string; codeVerifier: string }) => Promise<any>;
  };
  app: {
    onError: (callback: (data: { message?: string; context?: string; needsAuth?: string }) => void) => () => void;
    onBackendLog: (callback: (data: { level: string; message: string; source?: string; title?: string; timestamp?: string }) => void) => () => void;
  };
}

function makeOnListener(channel: string) {
  return (callback: (data: any) => void): (() => void) => {
    const listener = (_event: IpcRendererEvent, data: any) => callback(data);
    ipcRenderer.on(channel, listener);
    return () => ipcRenderer.removeListener(channel, listener);
  };
}

function makeOnListenerJSON(channel: string) {
  return (callback: (data: any) => void): (() => void) => {
    const listener = (_event: IpcRendererEvent, raw: any) => {
      try {
        callback(typeof raw === 'string' ? JSON.parse(raw) : raw);
      } catch {
        // Ignore parse errors
      }
    };
    ipcRenderer.on(channel, listener);
    return () => ipcRenderer.removeListener(channel, listener);
  };
}

const electronAPI: ElectronAPI = {
  updates: {
    getVersion: () => ipcRenderer.invoke('updates:get-version'),
    check: () => ipcRenderer.invoke('updates:check'),
    openRelease: (url) => ipcRenderer.invoke('updates:open-release', url),
    checkDeps: () => ipcRenderer.invoke('updates:check-deps'),
    getDependencyVersions: (packages) => ipcRenderer.invoke('updates:get-dependency-versions', packages),
    getBinaryVersions: () => ipcRenderer.invoke('updates:get-binary-versions'),
    installDep: (dep) => ipcRenderer.invoke('updates:install-dep', dep),
    onInstallProgress: makeOnListenerJSON('installation-progress'),
  },
  search: {
    perform: (params) => ipcRenderer.invoke('perform-search', params),
    getAlbumDetails: (platform, albumId) => ipcRenderer.invoke('get-album-details', platform, albumId),
    getPlaylistDetails: (platform, playlistId) => ipcRenderer.invoke('get-playlist-details', platform, playlistId),
    getArtistDetails: (platform, artistId) => ipcRenderer.invoke('get-artist-details', platform, artistId),
  },
  downloads: {
    startYouTubeMusic: (data, playlist) => ipcRenderer.send('start-yt-music-download', data, playlist),
    startYouTubeVideo: (data) => ipcRenderer.send('start-yt-video-download', data),
    startSpotify: (command) => ipcRenderer.send('start-spotify-download', command),
    startAppleMusic: (command) => ipcRenderer.send('start-apple-download', command),
    startQobuz: (data) => ipcRenderer.send('start-qobuz-download', data),
    startDeezer: (data) => ipcRenderer.send('start-deezer-download', data),
    startTidal: (data) => ipcRenderer.send('start-tidal-download', data),
    startGenericVideo: (data) => ipcRenderer.send('start-generic-video-download', data),
    onProgress: makeOnListener('download-update'),
    onInfo: makeOnListener('download-info'),
    onComplete: makeOnListener('download-complete'),
    onError: makeOnListener('download-error'),
    cancel: (order: number) => ipcRenderer.send('cancel-download', order),
    showItemInFolder: (filePath: string) => ipcRenderer.invoke('showItemInFolder', filePath),
  },
  settings: {
    get: () => ipcRenderer.invoke('get-settings'),
    set: (settings: any) => ipcRenderer.invoke('set-settings', settings),
    openFolder: async () => {
      const result = await ipcRenderer.invoke('dialog:openFolder');
      return result ?? null;
    },
    openFile: async () => {
      const result = await ipcRenderer.invoke('dialog:openFile');
      return result ?? null;
    },
  },
  library: {
    scan: (directory: string, force = false) => ipcRenderer.invoke('scan-directory', directory, { force }),
    onScanProgress: makeOnListener('scan-progress'),
    onFilesChanged: makeOnListener('library:filesChanged'),
    showItemInFolder: (filePath: string) => ipcRenderer.invoke('showItemInFolder', filePath),
  },
  player: {
    playMedia: (params) => ipcRenderer.invoke('play-media', params),
    pause: () => ipcRenderer.invoke('pause-media'),
    onStreamReady: makeOnListener('stream-ready'),
  },
  spotifyAccount: {
    login: () => ipcRenderer.invoke('spotify-oauth-login'),
    logout: () => ipcRenderer.invoke('spotify-oauth-logout'),
    getStatus: () => ipcRenderer.invoke('spotify-oauth-status'),
    getToken: () => ipcRenderer.invoke('spotify-get-token'),
  },
  tidalAuth: {
    startAuth: () => ipcRenderer.invoke('tidal:start-auth'),
    exchangeCode: (data) => ipcRenderer.invoke('tidal:exchange-code', data),
  },
  app: {
    onError: makeOnListener('app-error'),
    onBackendLog: makeOnListener('backend-log'),
  },
};

contextBridge.exposeInMainWorld('electron', electronAPI);


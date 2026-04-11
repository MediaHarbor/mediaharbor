declare module '*.svg' {
  const content: string;
  export default content;
}

interface DownloadRequest {
  url: string;
  outputDir?: string;
  quality?: string | number | null;
  title?: string | null;
  artist?: string | null;
  uploader?: string | null;
  album?: string | null;
  thumbnail?: string | null;
  platform?: string;
  isPlaylist?: boolean;
}

interface DownloadInfoEvent {
  order: number;
  title?: string;
  artist?: string;
  uploader?: string;
  album?: string;
  thumbnail?: string | null;
  platform?: string;
  quality?: string;
}

interface DownloadProgressEvent {
  order: number;
  progress: number;
  title?: string;
  thumbnail?: string | null;
  artist?: string;
  album?: string;
}

interface DownloadCompleteEvent {
  order: number;
  title?: string;
  warnings?: string;
  location?: string;
  fullLog?: string;
}

interface DownloadErrorEvent {
  order: number;
  error: string;
  fullLog: string;
  title?: string;
}

interface ScanProgressEvent {
  progress?: number;
  currentFile?: string;
}

interface SpotifyProfile {
  name?: string;
  id?: string;
  [key: string]: unknown;
}

interface Window {
  electron?: {
    search: {
      perform: (params: { platform: string; query: string; type: string }) => Promise<{ results: import('./types').SearchResult[]; platform: string }>;
      getAlbumDetails: (platform: string, albumId: string) => Promise<{ success: boolean; data?: import('./types').Album; error?: string }>;
      getPlaylistDetails: (platform: string, playlistId: string) => Promise<{ success: boolean; data?: import('./types').Playlist; error?: string }>;
      getArtistDetails: (platform: string, artistId: string) => Promise<{ success: boolean; data?: import('./types').Artist; error?: string }>;
    };
    downloads: {
      startYouTubeMusic: (data: DownloadRequest, playlist?: boolean) => void;
      startYouTubeVideo: (data: DownloadRequest) => void;
      startGenericVideo: (data: DownloadRequest) => void;
      startSpotify: (command: DownloadRequest) => void;
      startAppleMusic: (command: DownloadRequest) => void;
      startQobuz: (data: DownloadRequest) => void;
      startDeezer: (data: DownloadRequest) => void;
      startTidal: (data: DownloadRequest) => void;
      startOrpheus: (data: DownloadRequest) => void;
      onProgress:  (callback: (data: DownloadProgressEvent) => void) => () => void;
      onInfo:      (callback: (data: DownloadInfoEvent) => void) => () => void;
      onComplete:  (callback: (data: DownloadCompleteEvent) => void) => () => void;
      onError:     (callback: (data: DownloadErrorEvent) => void) => () => void;
      cancel:      (order: number) => void;
      showItemInFolder: (filePath: string) => Promise<boolean>;
    };
    settings: {
      get: () => Promise<import('./types/settings').Settings>;
      set: (settings: Partial<import('./types/settings').Settings>) => Promise<{ success: boolean; error?: string }>;
      openFolder: () => Promise<string | null>;
      openFile: () => Promise<string | null>;
    };
    library: {
      scan: (directory: string, force?: boolean) => Promise<import('./stores/useLibraryStore').MediaItem[]>;
      onScanProgress: (callback: (data: ScanProgressEvent) => void) => () => void;
      onFilesChanged: (callback: (data: { added: import('./stores/useLibraryStore').MediaItem[]; removedPaths: string[] }) => void) => () => void;
      showItemInFolder: (filePath: string) => Promise<boolean>;
    };
    player: {
      playMedia: (params: { url: string; platform: string }) => Promise<{ streamUrl: string; platform: string; mediaType?: string; isLive?: boolean }>;
      prefetchMedia: (params: { url: string; platform: string }) => Promise<{ streamUrl: string; platform: string; mediaType?: string; isLive?: boolean }>;
      pause: () => Promise<void>;
      onStreamReady: (callback: (data: { streamUrl: string; platform: string; durationSec?: number; mediaType?: 'audio' | 'video'; isLive?: boolean }) => void) => () => void;
    };
    tidalAuth: {
      startAuth: () => Promise<{ codeVerifier: string; authUrl: string }>;
      exchangeCode: (data: { redirectUrl: string; codeVerifier: string }) => Promise<Record<string, string>>;
    };
    spotifyAccount: {
      login: () => Promise<SpotifyProfile>;
      logout: () => Promise<void>;
      getStatus: () => Promise<{ loggedIn: boolean; profile: SpotifyProfile }>;
      getToken: () => Promise<string | null>;
    };
    app: {
      onError: (callback: (data: { message: string; context?: string; needsAuth?: string }) => void) => () => void;
      onBackendLog: (callback: (data: { level: string; message: string; source?: string; title?: string; timestamp?: string }) => void) => () => void;
      onStdinPrompt: (callback: (data: { downloadId: number; promptLines: string[] }) => void) => () => void;
      sendProcessStdin: (downloadId: number, input: string) => Promise<void>;
    };
    updates: {
      getVersion: () => Promise<string>;
      getBinaryVersions: () => Promise<{ python: string; git: string; ffmpeg: string }>;
      check: () => Promise<{
        hasUpdate: boolean;
        currentVersion: string;
        latestVersion: string;
        releaseNotes: string;
        releaseUrl: string;
        publishedAt: string;
      }>;
      openRelease: (url: string) => Promise<void>;
      updateDependencies: (packages: string[]) => void;
      getDependencyVersions: (packages: string[]) => Promise<Record<string, string>>;
      onDependencyNotification: (callback: (data: { type: string; message: string }) => void) => () => void;
      onDependencyLoading: (callback: (isLoading: boolean) => void) => () => void;
      checkDeps: () => Promise<Record<string, boolean>>;
      installDep: (dep: string) => Promise<{ success: boolean }>;
      onInstallProgress: (callback: (data: { dependency: string; percent: number; status: string }) => void) => () => void;
    };
    orpheus: {
      checkDeps: () => Promise<{ orpheus_installed: boolean; modules: Array<{ id: string; label: string; installed: boolean }> }>;
      installCore: () => Promise<{ success: boolean; error: string | null }>;
      installModule: (moduleId: string, customUrl?: string, label?: string) => Promise<{ success: boolean; error: string | null }>;
      readSettings: () => Promise<string>;
      writeSettings: (content: string) => Promise<void>;
      onInstallProgress: (callback: (data: { dependency: string; percent: number; status: string }) => void) => () => void;
    };
    lyrics: {
      get: (req: {
        url: string;
        platform: string;
        title: string;
        artist: string;
        duration?: number;
      }) => Promise<{ synced: string | null; plain: string | null; wordSynced: string | null }>;
    };
  };
}

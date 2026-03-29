
interface Window {
  electron?: {
    search: {
      perform: (params: { platform: string; query: string; type: string }) => Promise<{ results: any[]; platform: string }>;
      getAlbumDetails: (platform: string, albumId: string) => Promise<{ success: boolean; data?: any; error?: string }>;
      getPlaylistDetails: (platform: string, playlistId: string) => Promise<{ success: boolean; data?: any; error?: string }>;
      getArtistDetails: (platform: string, artistId: string) => Promise<{ success: boolean; data?: any; error?: string }>;
    };
    downloads: {
      startYouTubeMusic: (data: any, playlist?: any) => void;
      startYouTubeVideo: (data: any) => void;
      startGenericVideo: (data: any) => void;
      startSpotify: (command: any) => void;
      startAppleMusic: (command: any) => void;
      startQobuz: (data: any) => void;
      startDeezer: (data: any) => void;
      startTidal: (data: any) => void;
      onProgress:  (callback: (data: any) => void) => () => void;
      onInfo:      (callback: (data: any) => void) => () => void;
      onComplete:  (callback: (data: any) => void) => () => void;
      onError:     (callback: (data: any) => void) => () => void;
      cancel:      (order: number) => void;
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
    tidalAuth: {
      startAuth: () => Promise<{ codeVerifier: string; authUrl: string }>;
      exchangeCode: (data: { redirectUrl: string; codeVerifier: string }) => Promise<Record<string, string>>;
    };
    spotifyAccount: {
      login: () => Promise<any>;
      logout: () => Promise<any>;
      getStatus: () => Promise<{ loggedIn: boolean; profile: any }>;
      getToken: () => Promise<string | null>;
    };
    app: {
      onError: (callback: (data: { message: string; context?: string; needsAuth?: string }) => void) => () => void;
      onBackendLog: (callback: (data: { level: string; message: string; source?: string; title?: string; timestamp?: string }) => void) => () => void;
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
  };
}

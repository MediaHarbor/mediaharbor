import type { Platform, OrpheusPlatform } from '@/types';

const ORPHEUS_PLATFORMS: ReadonlySet<string> = new Set([
  'soundcloud', 'napster', 'beatport', 'nugs', 'kkbox', 'bugs', 'idagio', 'jiosaavn',
]);

interface DownloadParams {
  platform: Platform | OrpheusPlatform | 'generic';
  url: string;
  quality?: string | number;
  type?: 'track' | 'album' | 'playlist';
  title?: string;
  artist?: string;
  album?: string;
  thumbnail?: string | null;
}

type DownloadFn = keyof typeof window.electron.downloads & `start${string}`;

const PLATFORM_METHOD: Record<string, DownloadFn> = {
  youtube:      'startYouTubeVideo',
  youtubemusic: 'startYouTubeMusic',
  spotify:      'startSpotify',
  applemusic:   'startAppleMusic',
  qobuz:        'startQobuz',
  deezer:       'startDeezer',
  tidal:        'startTidal',
  generic:      'startGenericVideo',
};

const DEFAULT_QUALITY: Record<string, string | number> = {
  youtube:      'best',
  youtubemusic: '320',
  spotify:      9,
  applemusic:   'lossless',
  qobuz:        27,
  deezer:       2,
  tidal:        3,
  generic:      'bestvideo+bestaudio',
};

class DownloadService {
  async startDownload(params: DownloadParams): Promise<void> {
    if (!window.electron) {
      throw new Error('Electron API not available. Please run in Electron mode.');
    }

    const { platform, url, quality, title, artist, album, thumbnail } = params;
    const meta = { title, artist, album, thumbnail };

    try {
      const method = PLATFORM_METHOD[platform];
      if (method) {
        (window.electron.downloads[method] as (d: object) => void)({
          url, quality: quality ?? DEFAULT_QUALITY[platform], ...meta,
        });
      } else if (ORPHEUS_PLATFORMS.has(platform)) {
        window.electron.downloads.startOrpheus({ url, platform, ...meta });
      } else {
        throw new Error(`Unsupported platform: ${platform}`);
      }
    } catch (error) {
      throw new Error(`Download failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

}

export const downloadService = new DownloadService();

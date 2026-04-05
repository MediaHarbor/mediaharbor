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

class DownloadService {
  async startDownload(params: DownloadParams): Promise<void> {
    if (!window.electron) {
      throw new Error('Electron API not available. Please run in Electron mode.');
    }

    const { platform, url, quality, title, artist, album, thumbnail } = params;
    const meta = { title, artist, album, thumbnail };

    try {
      switch (platform) {
        case 'youtube':
          window.electron.downloads.startYouTubeVideo({ url, quality: quality || 'best', ...meta });
          break;

        case 'youtubemusic':
          window.electron.downloads.startYouTubeMusic({ url, quality: quality || '320', ...meta });
          break;

        case 'spotify':
          window.electron.downloads.startSpotify({ url, quality: quality || 9, ...meta });
          break;

        case 'applemusic':
          window.electron.downloads.startAppleMusic({ url, quality: quality || 'lossless', ...meta });
          break;

        case 'qobuz':
          window.electron.downloads.startQobuz({ url, quality: quality || 27, ...meta });
          break;

        case 'deezer':
          window.electron.downloads.startDeezer({ url, quality: quality || 2, ...meta });
          break;

        case 'tidal':
          window.electron.downloads.startTidal({ url, quality: quality || 3, ...meta });
          break;

        case 'generic':
          window.electron.downloads.startGenericVideo({ url, quality: quality || 'bestvideo+bestaudio', ...meta });
          break;

        default:
          if (ORPHEUS_PLATFORMS.has(platform)) {
            window.electron.downloads.startOrpheus({ url, platform, ...meta });
          } else {
            throw new Error(`Unsupported platform: ${platform}`);
          }
      }

    } catch (error) {
      throw new Error(`Download failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

}

export const downloadService = new DownloadService();

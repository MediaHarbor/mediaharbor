import type { Platform } from '@/types';

interface DownloadParams {
  platform: Platform | 'generic';
  url: string;
  quality?: string | number;
  type?: 'track' | 'album' | 'playlist';
}

class DownloadService {
  async startDownload(params: DownloadParams): Promise<void> {
    if (!window.electron) {
      throw new Error('Electron API not available. Please run in Electron mode.');
    }

    const { platform, url, quality } = params;

    try {
      switch (platform) {
        case 'youtube':
          window.electron.downloads.startYouTubeVideo({ url, quality: quality || 'best' });
          break;

        case 'youtubemusic':
          window.electron.downloads.startYouTubeMusic({ url, quality: quality || '320' });
          break;

        case 'spotify':
          window.electron.downloads.startSpotify({ url, quality: quality || 9 });
          break;

        case 'applemusic':
          window.electron.downloads.startAppleMusic({ url, quality: quality || 'lossless' });
          break;

        case 'qobuz':
          window.electron.downloads.startQobuz({ url, quality: quality || 27 });
          break;

        case 'deezer':
          window.electron.downloads.startDeezer({ url, quality: quality || 2 });
          break;

        case 'tidal':
          window.electron.downloads.startTidal({ url, quality: quality || 3 });
          break;

        case 'generic':
          window.electron.downloads.startGenericVideo({ url, quality: quality || 'bestvideo+bestaudio' });
          break;

        default:
          throw new Error(`Unsupported platform: ${platform}`);
      }

    } catch (error) {
      throw new Error(`Download failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

}

export const downloadService = new DownloadService();

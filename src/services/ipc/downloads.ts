import type { Platform, OrpheusPlatform } from '@/types';
import { errorMessage } from '@/utils/errors';
import { isBackendAvailable, tauriAPI } from '@/tauri-bridge';
import { AUX_PLATFORMS, ORPHEUS_SERVICES, SERVICES } from '@/utils/platform-data';

const ORPHEUS_PLATFORMS: ReadonlySet<string> = new Set(Object.keys(ORPHEUS_SERVICES));

interface DownloadParams {
  platform: Platform | OrpheusPlatform | 'generic';
  url: string;
  quality?: string | number;
  type?: 'track' | 'album' | 'playlist';
  title?: string;
  artist?: string;
  album?: string;
  thumbnail?: string | null;
  forceRedownload?: boolean;
}

type DownloadFn = keyof (typeof tauriAPI)['downloads'] & `start${string}`;

const PLATFORM_METHOD: Record<string, DownloadFn> = {
  ...Object.fromEntries(Object.values(SERVICES).map((s) => [s.id, s.downloadMethod as DownloadFn])),
  generic: AUX_PLATFORMS.generic.downloadMethod as DownloadFn,
};

const DEFAULT_QUALITY: Record<string, string | number> = {
  ...Object.fromEntries(Object.values(SERVICES).map((s) => [s.id, s.defaultQuality])),
  generic: AUX_PLATFORMS.generic.defaultQuality,
};

const BITRATE_RE = /^\d+K$/i;

function normalizeBitrate(quality: string | number | undefined, fallback: string): string | number {
  if (quality == null) return fallback;
  if (typeof quality === 'number') return Number.isFinite(quality) ? `${quality}K` : fallback;
  const trimmed = quality.trim();
  if (BITRATE_RE.test(trimmed)) return trimmed.toUpperCase();
  if (/^\d+$/.test(trimmed)) return `${trimmed}K`;
  return fallback;
}

class DownloadService {
  async startDownload(params: DownloadParams): Promise<void> {
    if (!isBackendAvailable()) {
      throw new Error(
        'MediaHarbor backend not available — run the desktop app, not the browser dev server.'
      );
    }

    const { platform, url, quality, title, artist, album, thumbnail, forceRedownload } = params;
    const meta = { title, artist, album, thumbnail };

    try {
      const method = PLATFORM_METHOD[platform];
      if (method) {
        const resolvedQuality =
          platform === 'youtubemusic'
            ? normalizeBitrate(quality, String(DEFAULT_QUALITY.youtubemusic))
            : (quality ?? DEFAULT_QUALITY[platform]);
        (tauriAPI.downloads[method] as (d: object) => void)({
          url,
          quality: resolvedQuality,
          forceRedownload,
          ...meta,
        });
      } else if (ORPHEUS_PLATFORMS.has(platform)) {
        tauriAPI.downloads.startOrpheus({ url, platform, ...meta });
      } else {
        throw new Error(`Unsupported platform: ${platform}`);
      }
    } catch (error) {
      throw new Error(`Download failed: ${errorMessage(error)}`, {
        cause: error,
      });
    }
  }
}

export const downloadService = new DownloadService();

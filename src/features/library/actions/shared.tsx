import { Copy, ExternalLink, Share2 } from 'lucide-react';
import { externalUrl, type MediaKind } from '@/utils/platform-data';
import { resolveCoverUrl } from '@/features/library/api';
import type { MediaType, PlayableTrack } from '@/stores/usePlayerStore';
import type { RadioResult } from '@/tauri-bridge';
import type { ActionItem } from './types';
import { copyText } from '@/utils/clipboard';
import { tauriAPI } from '@/tauri-bridge';

const iconCls = 'h-4 w-4';

export function openExternalUrl(url: string) {
  if (url) void tauriAPI.updates.openRelease(url);
}

export function buildShareItem(opts: {
  platform: string;
  kind: MediaKind;
  id?: string | null;
  fallbackUrl?: string | null;
}): ActionItem {
  const url = externalUrl({
    platform: opts.platform,
    kind: opts.kind,
    id: opts.id,
    fallback: opts.fallbackUrl,
  });
  return {
    id: 'share',
    label: 'Share',
    icon: <Share2 className={iconCls} />,
    hidden: !url,
    submenu: [
      {
        id: 'copy-link',
        label: 'Copy link',
        icon: <Copy className={iconCls} />,
        action: () => {
          void copyText(url ?? '');
        },
      },
      {
        id: 'open-external',
        label: 'Open in browser',
        icon: <ExternalLink className={iconCls} />,
        action: () => openExternalUrl(url ?? ''),
      },
    ],
  };
}

function str(v: unknown): string | undefined {
  return typeof v === 'string' && v ? v : undefined;
}

export async function radioTracksToQueue(
  res: RadioResult,
  platform: string
): Promise<PlayableTrack[]> {
  const videoUrls = res.video_urls ?? {};
  const resolved = await Promise.all(
    res.tracks.map(async (t) => {
      const tt = t as Record<string, unknown>;
      const url = str(tt.path);
      if (!url) return null;
      const coverId = str(tt.cover_id) ?? null;
      const thumbnail = coverId
        ? ((await resolveCoverUrl(coverId).catch(() => null)) ?? undefined)
        : undefined;
      const playable: PlayableTrack = {
        url,
        title: str(tt.title) ?? '',
        artist: str(tt.artist) ?? '',
        album: str(tt.album) ?? null,
        thumbnail,
        coverId,
        albumId: str(tt.album_key) ?? null,
        artistId: str(tt.artist_id) ?? null,
        mediaType: (tt.is_video === true ? 'video' : 'audio') as MediaType,
        videoUrl: videoUrls[url] ?? null,
        platform,
      };
      return playable;
    })
  );
  return resolved.filter((t): t is PlayableTrack => t != null);
}

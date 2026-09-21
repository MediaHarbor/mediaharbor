import { useMemo } from 'react';
import { Download } from 'lucide-react';
import { buildShareItem } from './shared';
import type { ActionContext, ActionItem, PodcastRef } from './types';
import type { DownloadRequest } from './useTrackActionItems';

interface Args {
  podcast: PodcastRef;
  platform: string;
  context: ActionContext;
  copyLink?: string;
  onDownloadRequest?: (req: DownloadRequest) => void;
}

const iconCls = 'h-4 w-4';

export function usePodcastActionItems({
  podcast,
  platform,
  copyLink,
  onDownloadRequest,
}: Args): ActionItem[] {
  return useMemo<ActionItem[]>(() => {
    return [
      buildShareItem({ platform, kind: 'playlist', id: podcast.id, fallbackUrl: copyLink }),
      {
        id: 'download',
        label: 'Download',
        icon: <Download className={iconCls} />,
        hidden: !copyLink || !onDownloadRequest,
        action: () => {
          if (!copyLink) return;
          onDownloadRequest?.({
            url: copyLink,
            kind: 'playlist',
            title: podcast.name,
            thumbnail: podcast.cover_url,
          });
        },
      },
    ];
  }, [podcast.id, podcast.name, podcast.cover_url, platform, copyLink, onDownloadRequest]);
}

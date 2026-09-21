import { useMemo } from 'react';
import { ListPlus, ListEnd, FileText, Download } from 'lucide-react';
import { useOwnedPlaylists } from '@/features/library/hooks/useLibraryMutations';
import { usePlayerStore, type PlayableTrack } from '@/stores/usePlayerStore';
import { useServiceCapabilities } from '@/features/library/hooks/useServiceCapabilities';
import { buildShareItem } from './shared';
import type { ActionContext, ActionItem, EpisodeRef } from './types';
import type { DownloadRequest } from './useTrackActionItems';

interface Args {
  episode: EpisodeRef;
  platform: string;
  context: ActionContext;
  onAddToServicePlaylist?: (playlistId: string) => void;
  onCreatePlaylist?: () => void;
  onSeeDescription?: () => void;
  onDownloadRequest?: (req: DownloadRequest) => void;
}

const iconCls = 'h-4 w-4';

export function useEpisodeActionItems({
  episode,
  platform,
  onAddToServicePlaylist,
  onCreatePlaylist,
  onSeeDescription,
  onDownloadRequest,
}: Args): ActionItem[] {
  const caps = useServiceCapabilities(platform);
  const ownedPlaylistsQ = useOwnedPlaylists(platform);
  const appendToQueue = usePlayerStore((s) => s.appendToQueue);

  return useMemo<ActionItem[]>(() => {
    const canEditPlaylists = caps.mutations?.edit_playlists ?? false;
    const canCreatePlaylists = caps.mutations?.create_playlists ?? false;
    const ownedPlaylists = ownedPlaylistsQ.data ?? [];

    const playable: PlayableTrack = {
      url: episode.url ?? '',
      title: episode.title ?? '',
      artist: episode.show ?? '',
      thumbnail: episode.cover_url ?? undefined,
      platform,
    };

    return [
      {
        id: 'add-to-playlist',
        label: 'Add to playlist',
        icon: <ListPlus className={iconCls} />,
        hidden: !canEditPlaylists,
        submenu: [
          ...ownedPlaylists.map<ActionItem>((p) => ({
            id: `add-to-${p.serviceId}`,
            label: p.name,
            action: () => onAddToServicePlaylist?.(p.serviceId),
          })),
          ...(ownedPlaylists.length && canCreatePlaylists
            ? [{ id: 'sep-1', label: '', separator: true }]
            : []),
          ...(canCreatePlaylists
            ? [{ id: 'new-playlist', label: 'New playlist…', action: onCreatePlaylist }]
            : []),
        ],
      },
      {
        id: 'add-to-queue',
        label: 'Add to queue',
        icon: <ListEnd className={iconCls} />,
        action: () => appendToQueue([playable]),
        hidden: !episode.url,
      },
      {
        id: 'see-description',
        label: 'See Episode Description',
        icon: <FileText className={iconCls} />,
        action: onSeeDescription,
        hidden: !onSeeDescription,
      },
      {
        id: 'download',
        label: 'Download',
        icon: <Download className={iconCls} />,
        hidden: !episode.url || !onDownloadRequest,
        action: () => {
          if (!episode.url) return;
          onDownloadRequest?.({
            url: episode.url,
            kind: 'track',
            title: episode.title,
            artist: episode.show,
            thumbnail: episode.cover_url,
          });
        },
      },
      buildShareItem({ platform, kind: 'track', id: episode.id, fallbackUrl: episode.url }),
    ];
  }, [
    episode.id,
    episode.url,
    episode.title,
    episode.show,
    episode.cover_url,
    platform,
    caps.mutations?.edit_playlists,
    caps.mutations?.create_playlists,
    ownedPlaylistsQ.data,
    onAddToServicePlaylist,
    onCreatePlaylist,
    onSeeDescription,
    onDownloadRequest,
    appendToQueue,
  ]);
}

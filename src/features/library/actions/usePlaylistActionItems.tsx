import { useMemo } from 'react';
import { Play, ListEnd, ListMusic, Heart, Pencil, Trash2, Download, EyeOff } from 'lucide-react';
import {
  useDeleteServicePlaylist,
  useToggleFollowedPlaylist,
  useRenameServicePlaylist,
  useSavedStateHydrate,
} from '@/features/library/hooks/useLibraryMutations';
import { useSavedStateStore } from '@/stores/useSavedStateStore';
import { useServiceCapabilities } from '@/features/library/hooks/useServiceCapabilities';
import { buildShareItem } from './shared';
import type { ActionContext, ActionItem, PlaylistRef } from './types';
import type { DownloadRequest } from './useTrackActionItems';

interface Args {
  playlist: PlaylistRef;
  platform: string;
  context: ActionContext;
  onPlay?: () => void;
  onAddToQueue?: () => void;
  onOpen?: () => void;
  onRename?: () => void;
  onDelete?: () => void;
  copyLink?: string;
  onDownloadRequest?: (req: DownloadRequest) => void;
}

const iconCls = 'h-4 w-4';

export function usePlaylistActionItems({
  playlist,
  platform,
  onPlay,
  onAddToQueue,
  onOpen,
  onRename,
  onDelete,
  copyLink,
  onDownloadRequest,
}: Args): ActionItem[] {
  const caps = useServiceCapabilities(platform);
  const del = useDeleteServicePlaylist();
  const followPlaylist = useToggleFollowedPlaylist(platform);
  const renamePlaylist = useRenameServicePlaylist();
  useSavedStateHydrate(platform, 'playlist');
  const isFollowing = useSavedStateStore((s) => s.isSaved(platform, 'playlist', playlist.id));

  return useMemo<ActionItem[]>(() => {
    const canEdit = caps.mutations?.edit_playlists ?? false;
    const canFollow = caps.mutations?.follow_playlists ?? false;
    const owned = playlist.owned === true;
    const downloadItem: ActionItem = {
      id: 'download',
      label: 'Download',
      icon: <Download className={iconCls} />,
      hidden: !copyLink || !onDownloadRequest,
      action: () => {
        if (!copyLink) return;
        onDownloadRequest?.({
          url: copyLink,
          kind: 'playlist',
          title: playlist.name,
          thumbnail: playlist.cover_url,
        });
      },
    };
    const shareItem = buildShareItem({
      platform,
      kind: 'playlist',
      id: playlist.id,
      fallbackUrl: copyLink,
    });

    if (owned) {
      return [
        {
          id: 'play',
          label: 'Play',
          icon: <Play className={iconCls} />,
          action: onPlay,
          hidden: !onPlay,
        },
        {
          id: 'add-queue',
          label: 'Add to queue',
          icon: <ListEnd className={iconCls} />,
          action: onAddToQueue,
          hidden: !onAddToQueue,
        },
        {
          id: 'edit',
          label: 'Edit details',
          icon: <Pencil className={iconCls} />,
          hidden: !canEdit || !onRename,
          action: onRename,
        },
        {
          id: 'delete',
          label: 'Delete',
          icon: <Trash2 className={iconCls} />,
          destructive: true,
          hidden: !canEdit,
          action: onDelete ?? (() => del.mutate({ platform, id: playlist.id })),
        },
        {
          id: 'make-private',
          label: 'Make private',
          icon: <EyeOff className={iconCls} />,
          hidden: !canEdit || !playlist.name,
          action: () =>
            renamePlaylist.mutate({
              platform,
              id: playlist.id,
              name: playlist.name ?? '',
              isPublic: false,
            }),
        },
        downloadItem,
        shareItem,
      ];
    }

    return [
      {
        id: 'play',
        label: 'Play',
        icon: <Play className={iconCls} />,
        action: onPlay,
        hidden: !onPlay,
      },
      {
        id: 'add-library',
        label: isFollowing ? 'Remove from Your Library' : 'Add to Your Library',
        icon: <Heart className={iconCls} fill={isFollowing ? 'currentColor' : 'none'} />,
        hidden: !canFollow,
        action: () => followPlaylist.mutate({ id: playlist.id, save: !isFollowing }),
      },
      {
        id: 'add-queue',
        label: 'Add to queue',
        icon: <ListEnd className={iconCls} />,
        action: onAddToQueue,
        hidden: !onAddToQueue,
      },
      {
        id: 'open',
        label: 'Open playlist',
        icon: <ListMusic className={iconCls} />,
        action: onOpen,
        hidden: !onOpen,
      },
      downloadItem,
      shareItem,
    ];
  }, [
    playlist.id,
    playlist.owned,
    playlist.name,
    playlist.cover_url,
    platform,
    caps.mutations?.edit_playlists,
    caps.mutations?.follow_playlists,
    isFollowing,
    followPlaylist,
    renamePlaylist,
    onPlay,
    onAddToQueue,
    onOpen,
    onRename,
    onDelete,
    copyLink,
    onDownloadRequest,
    del,
  ]);
}

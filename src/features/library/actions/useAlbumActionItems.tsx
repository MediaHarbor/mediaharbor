import { useMemo } from 'react';
import { Play, ListEnd, Heart, Disc, User, Radio, Download } from 'lucide-react';
import { useSavedStateStore } from '@/stores/useSavedStateStore';
import { useToggleSavedAlbum, useStartRadio } from '@/features/library/hooks/useLibraryMutations';
import { usePlayerStore } from '@/stores/usePlayerStore';
import { useServiceCapabilities } from '@/features/library/hooks/useServiceCapabilities';
import { buildShareItem, radioTracksToQueue } from './shared';
import type { ActionContext, ActionItem, AlbumRef } from './types';
import type { DownloadRequest } from './useTrackActionItems';

interface Args {
  album: AlbumRef;
  platform: string;
  context: ActionContext;
  onPlay?: () => void;
  onAddToQueue?: () => void;
  copyLink?: string;
  onGoToAlbum?: (albumId: string) => void;
  onGoToArtist?: (artistId: string) => void;
  onDownloadRequest?: (req: DownloadRequest) => void;
}

const iconCls = 'h-4 w-4';

export function useAlbumActionItems({
  album,
  platform,
  onPlay,
  onAddToQueue,
  copyLink,
  onGoToAlbum,
  onGoToArtist,
  onDownloadRequest,
}: Args): ActionItem[] {
  const caps = useServiceCapabilities(platform);
  const isSaved = useSavedStateStore((s) => s.isSaved(platform, 'album', album.id));
  const toggleSaved = useToggleSavedAlbum(platform);
  const startRadio = useStartRadio();
  const setQueue = usePlayerStore((s) => s.setQueue);

  return useMemo<ActionItem[]>(() => {
    const canSave = caps.mutations?.save_albums ?? false;
    const canRadio = caps.mutations?.radio ?? false;
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
        id: 'save',
        label: isSaved ? 'Remove from library' : 'Save album',
        icon: <Heart className={iconCls} fill={isSaved ? 'currentColor' : 'none'} />,
        action: () => toggleSaved.mutate({ id: album.id, save: !isSaved }),
        hidden: !canSave,
      },
      { id: 'sep-1', label: '', separator: true },
      {
        id: 'go-album',
        label: 'Go to album',
        icon: <Disc className={iconCls} />,
        hidden: !onGoToAlbum || !album.id,
        action: () => {
          if (album.id) onGoToAlbum?.(album.id);
        },
      },
      {
        id: 'go-artist',
        label: 'Go to artist',
        icon: <User className={iconCls} />,
        hidden: !onGoToArtist || !album.artistId,
        action: () => {
          if (album.artistId) onGoToArtist?.(album.artistId);
        },
      },
      {
        id: 'start-radio',
        label: 'Start album radio',
        icon: <Radio className={iconCls} />,
        hidden: !canRadio,
        action: () =>
          startRadio.mutate(
            { platform, seedKind: 'album', seedId: album.id },
            {
              onSuccess: (res) => {
                void (async () => {
                  const q = await radioTracksToQueue(res, platform);
                  if (!q.length) return;
                  setQueue(q, 0);
                })();
              },
            }
          ),
      },
      { id: 'sep-2', label: '', separator: true },
      {
        id: 'download',
        label: 'Download album',
        icon: <Download className={iconCls} />,
        hidden: !copyLink || !onDownloadRequest,
        action: () => {
          if (!copyLink) return;
          onDownloadRequest?.({
            url: copyLink,
            kind: 'album',
            title: album.title,
            artist: album.artist,
            album: album.title,
            thumbnail: album.cover_url,
          });
        },
      },
      buildShareItem({ platform, kind: 'album', id: album.id, fallbackUrl: copyLink }),
    ];
  }, [
    album.id,
    album.artist,
    album.artistId,
    album.title,
    album.cover_url,
    platform,
    isSaved,
    caps.mutations?.save_albums,
    caps.mutations?.radio,
    onPlay,
    onAddToQueue,
    copyLink,
    onGoToAlbum,
    onGoToArtist,
    onDownloadRequest,
    toggleSaved,
    startRadio,
    setQueue,
  ]);
}

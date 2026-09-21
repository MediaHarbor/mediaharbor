import { useMemo } from 'react';
import { Play, ListEnd, ListPlus, Heart, Disc, User, Radio, Download, Trash2 } from 'lucide-react';
import { useSavedStateStore } from '@/stores/useSavedStateStore';
import {
  useToggleSavedTrack,
  useStartRadio,
  useRemoveTracksFromServicePlaylist,
  useOwnedPlaylists,
} from '@/features/library/hooks/useLibraryMutations';
import { usePlayerStore, type PlayableTrack } from '@/stores/usePlayerStore';
import { useServiceCapabilities } from '@/features/library/hooks/useServiceCapabilities';
import { buildShareItem, radioTracksToQueue } from './shared';
import type { ActionContext, ActionItem, TrackRef } from './types';

export interface DownloadRequest {
  url: string;
  kind: 'track' | 'album' | 'playlist';
  title?: string | null;
  artist?: string | null;
  album?: string | null;
  thumbnail?: string | null;
}

interface UseTrackActionItemsArgs {
  track: TrackRef;
  platform: string;
  context: ActionContext;
  playlistId?: string;
  queueIndex?: number;
  onPlay?: () => void;
  onAddToServicePlaylist?: (playlistId: string) => void;
  onCreatePlaylist?: () => void;
  onGoToAlbum?: (albumId: string) => void;
  onGoToArtist?: (artistId: string) => void;
  onDownloadRequest?: (req: DownloadRequest) => void;
  onMutated?: () => void;
}

const iconCls = 'h-4 w-4';

export function useTrackActionItems({
  track,
  platform,
  context,
  playlistId,
  queueIndex,
  onPlay,
  onAddToServicePlaylist,
  onCreatePlaylist,
  onGoToAlbum,
  onGoToArtist,
  onDownloadRequest,
  onMutated,
}: UseTrackActionItemsArgs): ActionItem[] {
  const caps = useServiceCapabilities(platform);
  const isSaved = useSavedStateStore((s) => s.isSaved(platform, 'track', track.id));
  const toggleSaved = useToggleSavedTrack(platform);
  const startRadio = useStartRadio();
  const removeFromPlaylist = useRemoveTracksFromServicePlaylist();
  const ownedPlaylistsQ = useOwnedPlaylists(platform);

  const insertNext = usePlayerStore((s) => s.insertNext);
  const appendToQueue = usePlayerStore((s) => s.appendToQueue);
  const removeFromQueue = usePlayerStore((s) => s.removeFromQueue);
  const setQueue = usePlayerStore((s) => s.setQueue);

  return useMemo<ActionItem[]>(() => {
    const playable: PlayableTrack = {
      url: track.url ?? '',
      title: track.title ?? '',
      artist: track.artist ?? '',
      thumbnail: track.thumbnail ?? undefined,
      platform,
    };
    const ownedPlaylists = ownedPlaylistsQ.data ?? [];
    const canRadio = caps.mutations?.radio ?? false;

    const items: ActionItem[] = [
      {
        id: 'play',
        label: 'Play',
        icon: <Play className={iconCls} />,
        hidden: !onPlay,
        action: onPlay,
      },
      {
        id: 'add-to-playlist',
        label: 'Add to playlist',
        icon: <ListPlus className={iconCls} />,
        submenu: [
          ...ownedPlaylists.map<ActionItem>((p) => ({
            id: `add-to-${p.serviceId}`,
            label: p.name,
            action: () => onAddToServicePlaylist?.(p.serviceId),
          })),
          ...(ownedPlaylists.length ? [{ id: 'sep-1', label: '', separator: true }] : []),
          {
            id: 'new-playlist',
            label: 'New playlist…',
            action: () => onCreatePlaylist?.(),
          },
        ],
      },
      {
        id: 'remove-from-playlist',
        label: 'Remove from this playlist',
        icon: <Trash2 className={iconCls} />,
        destructive: true,
        hidden: context !== 'playlist' || !playlistId,
        action: () => {
          if (playlistId) {
            removeFromPlaylist.mutate(
              { platform, id: playlistId, trackIds: [track.id] },
              { onSuccess: () => onMutated?.() }
            );
          }
        },
      },
      {
        id: 'save',
        label: isSaved ? 'Remove from your Liked Songs' : 'Save to your Liked Songs',
        icon: <Heart className={iconCls} fill={isSaved ? 'currentColor' : 'none'} />,
        action: () => toggleSaved.mutate({ id: track.id, save: !isSaved }),
      },
      {
        id: 'play-next',
        label: 'Play next',
        icon: <Play className={iconCls} />,
        action: () => insertNext(playable),
      },
      {
        id: 'add-to-queue',
        label: 'Add to queue',
        icon: <ListEnd className={iconCls} />,
        action: () => appendToQueue([playable]),
      },
      {
        id: 'start-radio',
        label: 'Start song radio',
        icon: <Radio className={iconCls} />,
        hidden: !canRadio,
        action: () =>
          startRadio.mutate(
            { platform, seedKind: 'track', seedId: track.id },
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
      {
        id: 'go-to-artist',
        label: 'Go to artist',
        icon: <User className={iconCls} />,
        hidden: !track.artistId,
        action: () => {
          if (track.artistId) onGoToArtist?.(track.artistId);
        },
      },
      {
        id: 'go-to-album',
        label: 'Go to album',
        icon: <Disc className={iconCls} />,
        hidden: !track.albumId || context === 'album',
        action: () => {
          if (track.albumId) onGoToAlbum?.(track.albumId);
        },
      },
      {
        id: 'download',
        label: 'Download',
        icon: <Download className={iconCls} />,
        action: () => {
          if (!track.url) return;
          onDownloadRequest?.({
            url: track.url,
            kind: 'track',
            title: track.title,
            artist: track.artist,
            album: track.album,
            thumbnail: track.thumbnail,
          });
        },
      },
      {
        ...buildShareItem({ platform, kind: 'track', id: track.id, fallbackUrl: track.url }),
        hidden: false,
      },
      {
        id: 'remove-from-queue',
        label: 'Remove from queue',
        icon: <Trash2 className={iconCls} />,
        destructive: true,
        hidden: context !== 'queue' || queueIndex == null,
        action: () => {
          if (queueIndex != null) removeFromQueue(queueIndex);
        },
      },
    ];
    return items;
  }, [
    track.id,
    track.url,
    track.title,
    track.artist,
    track.album,
    track.thumbnail,
    track.albumId,
    track.artistId,
    platform,
    context,
    playlistId,
    queueIndex,
    onPlay,
    isSaved,
    caps.mutations?.radio,
    ownedPlaylistsQ.data,
    toggleSaved,
    startRadio,
    removeFromPlaylist,
    insertNext,
    appendToQueue,
    removeFromQueue,
    setQueue,
    onAddToServicePlaylist,
    onCreatePlaylist,
    onGoToAlbum,
    onGoToArtist,
    onDownloadRequest,
    onMutated,
  ]);
}

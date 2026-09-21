import { useState, type ReactNode } from 'react';
import { ContextMenu, ContextMenuTrigger, ContextMenuContent } from '@/components/ui/context-menu';
import { ActionItems } from './ActionItems';
import { ActionsKebab } from './ActionsKebab';
import { useTrackActionItems, type DownloadRequest } from './useTrackActionItems';
import { useAlbumActionItems } from './useAlbumActionItems';
import { useArtistActionItems } from './useArtistActionItems';
import { usePlaylistActionItems } from './usePlaylistActionItems';
import { usePodcastActionItems } from './usePodcastActionItems';
import { useEpisodeActionItems } from './useEpisodeActionItems';
import { useMediaNavigation } from './useMediaNavigation';
import { useAddTracksToServicePlaylist } from '@/features/library/hooks/useLibraryMutations';
import { useDownloadDialogStore } from '@/stores/useDownloadDialogStore';
import { CreatePlaylistDialog } from '@/features/library/components/CreatePlaylistDialog';
import { externalUrl, toClientPlatform } from '@/utils/platform-data';
import type {
  ActionItem,
  ActionContext,
  TrackRef,
  AlbumRef,
  ArtistRef,
  PlaylistRef,
  PodcastRef,
  EpisodeRef,
} from './types';

interface KebabOpts {
  size?: 'sm' | 'md';
  align?: 'start' | 'center' | 'end';
  className?: string;
}

function useDownloadDefault(platform: string) {
  const requestDownload = useDownloadDialogStore((s) => s.requestDownload);
  return (req: DownloadRequest) =>
    requestDownload({
      platform: toClientPlatform(platform),
      url: req.url,
      kind: req.kind,
      title: req.title,
      artist: req.artist,
      album: req.album,
      thumbnail: req.thumbnail,
    });
}

interface TrackMenuArgs {
  track: TrackRef;
  platform: string;
  context: ActionContext;
  playlistId?: string;
  queueIndex?: number;
  onPlay?: () => void;
  onGoToAlbum?: (albumId: string) => void;
  onGoToArtist?: (artistId: string) => void;
  onDownloadRequest?: (req: DownloadRequest) => void;
  onMutated?: () => void;
}

function useTrackMenu(args: TrackMenuArgs): { items: ActionItem[]; dialog: ReactNode } {
  const {
    track,
    platform,
    context,
    playlistId,
    queueIndex,
    onPlay,
    onGoToAlbum,
    onGoToArtist,
    onDownloadRequest,
    onMutated,
  } = args;
  const [createOpen, setCreateOpen] = useState(false);
  const addTracks = useAddTracksToServicePlaylist();
  const nav = useMediaNavigation();
  const downloadDefault = useDownloadDefault(platform);

  const items = useTrackActionItems({
    track,
    platform,
    context,
    playlistId,
    queueIndex,
    onPlay,
    onAddToServicePlaylist: (targetPlaylistId) => {
      addTracks.mutate(
        { platform, id: targetPlaylistId, trackIds: [track.id] },
        { onSuccess: () => onMutated?.() }
      );
    },
    onCreatePlaylist: () => setCreateOpen(true),
    onGoToAlbum: onGoToAlbum ?? ((albumId) => nav.goToAlbum(platform, albumId)),
    onGoToArtist: onGoToArtist ?? ((artistId) => nav.goToArtist(platform, artistId)),
    onDownloadRequest: onDownloadRequest ?? downloadDefault,
    onMutated,
  });

  const dialog = (
    <CreatePlaylistDialog
      open={createOpen}
      onOpenChange={setCreateOpen}
      platform={platform}
      initialTrackIds={[track.id]}
    />
  );
  return { items, dialog };
}

interface AlbumMenuArgs {
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

function useAlbumMenu(args: AlbumMenuArgs): ActionItem[] {
  const {
    album,
    platform,
    context,
    onPlay,
    onAddToQueue,
    copyLink,
    onGoToAlbum,
    onGoToArtist,
    onDownloadRequest,
  } = args;
  const nav = useMediaNavigation();
  const downloadDefault = useDownloadDefault(platform);
  const link = copyLink ?? externalUrl({ platform, kind: 'album', id: album.id }) ?? undefined;

  return useAlbumActionItems({
    album,
    platform,
    context,
    onPlay,
    onAddToQueue,
    copyLink: link,
    onGoToAlbum: onGoToAlbum ?? ((albumId) => nav.goToAlbum(platform, albumId)),
    onGoToArtist: onGoToArtist ?? ((artistId) => nav.goToArtist(platform, artistId)),
    onDownloadRequest: onDownloadRequest ?? downloadDefault,
  });
}

interface ArtistMenuArgs {
  artist: ArtistRef;
  platform: string;
  context: ActionContext;
  copyLink?: string;
  onGoToArtist?: (artistId: string) => void;
}

function useArtistMenu(args: ArtistMenuArgs): ActionItem[] {
  const { artist, platform, context, copyLink, onGoToArtist } = args;
  const nav = useMediaNavigation();
  const link = copyLink ?? externalUrl({ platform, kind: 'artist', id: artist.id }) ?? undefined;

  return useArtistActionItems({
    artist,
    platform,
    context,
    copyLink: link,
    onGoToArtist: onGoToArtist ?? ((artistId) => nav.goToArtist(platform, artistId)),
  });
}

interface PlaylistMenuArgs {
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

function usePlaylistMenu(args: PlaylistMenuArgs): ActionItem[] {
  const {
    playlist,
    platform,
    context,
    onPlay,
    onAddToQueue,
    onOpen,
    onRename,
    onDelete,
    copyLink,
    onDownloadRequest,
  } = args;
  const nav = useMediaNavigation();
  const downloadDefault = useDownloadDefault(platform);
  const link =
    copyLink ?? externalUrl({ platform, kind: 'playlist', id: playlist.id }) ?? undefined;

  return usePlaylistActionItems({
    playlist,
    platform,
    context,
    onPlay,
    onAddToQueue,
    onRename,
    onDelete,
    copyLink: link,
    onOpen: onOpen ?? (() => nav.goToPlaylist(platform, { serviceId: playlist.id })),
    onDownloadRequest: onDownloadRequest ?? downloadDefault,
  });
}

interface PodcastMenuArgs {
  podcast: PodcastRef;
  platform: string;
  context: ActionContext;
  copyLink?: string;
  onDownloadRequest?: (req: DownloadRequest) => void;
}

function usePodcastMenu(args: PodcastMenuArgs): ActionItem[] {
  const { podcast, platform, context, copyLink, onDownloadRequest } = args;
  const downloadDefault = useDownloadDefault(platform);
  const link = copyLink ?? externalUrl({ platform, kind: 'playlist', id: podcast.id }) ?? undefined;

  return usePodcastActionItems({
    podcast,
    platform,
    context,
    copyLink: link,
    onDownloadRequest: onDownloadRequest ?? downloadDefault,
  });
}

interface EpisodeMenuArgs {
  episode: EpisodeRef;
  platform: string;
  context: ActionContext;
  onSeeDescription?: () => void;
  onDownloadRequest?: (req: DownloadRequest) => void;
}

function useEpisodeMenu(args: EpisodeMenuArgs): { items: ActionItem[]; dialog: ReactNode } {
  const { episode, platform, context, onSeeDescription, onDownloadRequest } = args;
  const [createOpen, setCreateOpen] = useState(false);
  const addTracks = useAddTracksToServicePlaylist();
  const downloadDefault = useDownloadDefault(platform);

  const items = useEpisodeActionItems({
    episode,
    platform,
    context,
    onSeeDescription,
    onAddToServicePlaylist: (targetPlaylistId) => {
      addTracks.mutate({ platform, id: targetPlaylistId, trackIds: [episode.id] });
    },
    onCreatePlaylist: () => setCreateOpen(true),
    onDownloadRequest: onDownloadRequest ?? downloadDefault,
  });

  const dialog = (
    <CreatePlaylistDialog
      open={createOpen}
      onOpenChange={setCreateOpen}
      platform={platform}
      initialTrackIds={[episode.id]}
    />
  );
  return { items, dialog };
}

type MenuOut = ActionItem[] | { items: ActionItem[]; dialog?: ReactNode };

/** Every entity menu is the same shell around a different `use*Menu` hook. */
function menuPair<A extends object>(name: string, useMenu: (args: A) => MenuOut) {
  const split = (r: MenuOut): { items: ActionItem[]; dialog?: ReactNode } =>
    Array.isArray(r) ? { items: r } : r;

  const Menu = ({ children, ...args }: A & { children: ReactNode }) => {
    const { items, dialog } = split(useMenu(args as unknown as A));
    return (
      <>
        <ContextMenu>
          <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
          <ContextMenuContent>
            <ActionItems items={items} menuType="context" />
          </ContextMenuContent>
        </ContextMenu>
        {dialog}
      </>
    );
  };
  Menu.displayName = `${name}ContextMenu`;

  const Kebab = ({ size, align, className, ...args }: A & KebabOpts) => {
    const { items, dialog } = split(useMenu(args as unknown as A));
    return (
      <>
        <ActionsKebab items={items} size={size} align={align} className={className} />
        {dialog}
      </>
    );
  };
  Kebab.displayName = `${name}Kebab`;

  return [Menu, Kebab] as const;
}

export const [TrackContextMenu, TrackKebab] = menuPair<TrackMenuArgs>('Track', useTrackMenu);
export const [AlbumContextMenu, AlbumKebab] = menuPair<AlbumMenuArgs>('Album', useAlbumMenu);
export const [ArtistContextMenu, ArtistKebab] = menuPair<ArtistMenuArgs>('Artist', useArtistMenu);
export const [PlaylistContextMenu, PlaylistKebab] = menuPair<PlaylistMenuArgs>(
  'Playlist',
  usePlaylistMenu
);
export const [PodcastContextMenu, PodcastKebab] = menuPair<PodcastMenuArgs>(
  'Podcast',
  usePodcastMenu
);
export const [EpisodeContextMenu, EpisodeKebab] = menuPair<EpisodeMenuArgs>(
  'Episode',
  useEpisodeMenu
);

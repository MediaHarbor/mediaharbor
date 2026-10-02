import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ArrowLeft, Play, User, Disc3 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { usePlayerStore } from '@/stores/usePlayerStore';
import type { MediaType, PlayableTrack } from '@/stores/usePlayerStore';
import {
  ipcPlatformOf,
  trackIdFromUrl,
  libraryKeys,
  queryArtistPage,
  type ServicePlatform,
} from '@/features/library/api';
import { ServiceHomeView } from '@/features/library/components/ServiceHomeView';
import { formatDurationShort as fmt } from '@/utils/formatters';
import { useVisibleCoverUrls } from '@/features/library/useCoverUrls';
import { SaveToggleHeart } from '@/features/library/components/SaveToggleHeart';
import {
  AlbumContextMenu,
  AlbumKebab,
  TrackContextMenu,
  TrackKebab,
} from '@/features/library/actions/RowContextMenu';
import { ActionItems } from '@/features/library/actions/ActionItems';
import { ActionsKebab } from '@/features/library/actions/ActionsKebab';
import { buildLocalTrackItems } from '@/features/library/actions/localTrackItems';
import type { ActionItem } from '@/features/library/actions/types';
import { ContextMenu, ContextMenuTrigger, ContextMenuContent } from '@/components/ui/context-menu';
import { contextUriFor, normalizePlatform } from '@/utils/platform-data';
import { tauriAPI } from '@/tauri-bridge';

interface ArtistAlbum {
  album_key: string;
  title: string;
  artist: string;
  year?: string | null;
  cover_id?: string | null;
  track_count: number;
}

interface ArtistTrack {
  path: string;
  title?: string | null;
  artist?: string | null;
  album?: string | null;
  album_key?: string | null;
  duration_secs?: number | null;
  cover_id?: string | null;
}

interface ArtistDetailViewProps {
  artistKey: string;
  source?: ServicePlatform;
  onBack: () => void;
  onSelectAlbum: (album_key: string) => void;
  onSelectArtist?: (artistKey: string) => void;
  onSelectPlaylist?: (id: number, serviceId?: string | null) => void;
}

export function ArtistDetailView({
  artistKey,
  source = 'local',
  onBack,
  onSelectAlbum,
  onSelectArtist,
  onSelectPlaylist,
}: ArtistDetailViewProps) {
  const [display, setDisplay] = useState('');
  const [albums, setAlbums] = useState<ArtistAlbum[]>([]);
  const [tracks, setTracks] = useState<ArtistTrack[]>([]);
  const setQueue = usePlayerStore((s) => s.setQueue);
  const insertNext = usePlayerStore((s) => s.insertNext);
  const appendToQueue = usePlayerStore((s) => s.appendToQueue);
  const isLocal = source === 'local';
  const menuPlatform = normalizePlatform(source);

  useEffect(() => {
    let cancelled = false;
    const fetcher = isLocal
      ? tauriAPI.library.getArtist?.(artistKey)
      : tauriAPI.serviceLibrary?.artist?.(source, artistKey);
    fetcher?.then((r) => {
      if (cancelled || !r) return;
      setDisplay(String(r.display ?? ''));
      setAlbums((r.albums ?? []) as unknown as ArtistAlbum[]);
      setTracks((r.tracks ?? []) as unknown as ArtistTrack[]);
    });
    return () => {
      cancelled = true;
    };
  }, [artistKey, source, isLocal]);

  const artistPageQuery = useQuery({
    queryKey: libraryKeys.artistPage(source, artistKey),
    queryFn: () => queryArtistPage(source as Exclude<ServicePlatform, 'local'>, artistKey),
    enabled: !isLocal,
    staleTime: 5 * 60 * 1000,
  });
  const pageShelves = artistPageQuery.data?.shelves ?? [];

  const coverMap = useVisibleCoverUrls([
    ...albums.map((a) => a.cover_id),
    ...tracks.map((t) => t.cover_id),
  ]);

  const playPlatform = isLocal ? 'local' : ipcPlatformOf(source);
  const toPlayable = (t: ArtistTrack): PlayableTrack => ({
    url: t.path,
    title: t.title ?? '',
    artist: t.artist ?? display,
    thumbnail: t.cover_id ? (coverMap[t.cover_id] ?? undefined) : undefined,
    coverId: t.cover_id ?? undefined,
    album: t.album ?? null,
    albumId: t.album_key ?? null,
    artistId: artistKey,
    mediaType: 'audio' as MediaType,
    platform: playPlatform,
  });

  const playFrom = (index: number) => {
    if (tracks.length === 0) return;
    const contextUri = contextUriFor(source, 'artist', artistKey);
    setQueue(tracks.map(toPlayable), Math.min(index, tracks.length - 1), contextUri);
  };

  const playAll = () => playFrom(0);

  const localTrackItems = (t: ArtistTrack, index: number): ActionItem[] =>
    buildLocalTrackItems({
      playable: toPlayable(t),
      path: t.path,
      onPlay: () => playFrom(index),
      insertNext,
      appendToQueue,
    });

  return (
    <div className="space-y-6">
      <Button variant="ghost" size="sm" onClick={onBack} className="gap-2">
        <ArrowLeft className="h-4 w-4" />
        Back
      </Button>

      <div className="flex items-end gap-6">
        <div className="h-40 w-40 rounded-full overflow-hidden bg-muted shrink-0 flex items-center justify-center">
          {albums[0]?.cover_id && coverMap[albums[0].cover_id] ? (
            <img
              src={coverMap[albums[0].cover_id]!}
              alt={display}
              loading="lazy"
              className="w-full h-full object-cover"
            />
          ) : (
            <User className="h-16 w-16 text-muted-foreground/30" />
          )}
        </div>
        <div className="flex-1 min-w-0">
          <p className="text-xs uppercase tracking-wide text-muted-foreground/50">Artist</p>
          <h1 className="text-3xl font-bold truncate">{display}</h1>
          <p className="text-sm text-muted-foreground/60 mt-1">
            {albums.length} album{albums.length !== 1 ? 's' : ''} · {tracks.length} track
            {tracks.length !== 1 ? 's' : ''}
          </p>
          <div className="mt-4 flex items-center gap-3">
            <Button onClick={playAll} className="gap-2">
              <Play className="h-4 w-4" />
              Play
            </Button>
            {!isLocal && (
              <SaveToggleHeart
                platform={ipcPlatformOf(source)}
                kind="artist"
                id={artistKey}
                size="md"
                visibility="always"
                label="Follow artist"
              />
            )}
          </div>
        </div>
      </div>

      <div>
        <h2 className="text-lg font-semibold mb-3">Albums</h2>
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
          {albums.map((a) => {
            const url = a.cover_id ? coverMap[a.cover_id] : null;
            const albumRef = {
              id: a.album_key,
              title: a.title,
              artist: a.artist,
              cover_url: url ?? null,
            };
            return (
              <AlbumContextMenu
                key={a.album_key}
                platform={menuPlatform}
                context="library"
                album={albumRef}
                onPlay={() => onSelectAlbum(a.album_key)}
              >
                <div className="group relative">
                  <div
                    className="absolute top-2 right-2 z-10 opacity-0 group-hover:opacity-100 transition-opacity rounded-md bg-background/70 backdrop-blur-sm"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <AlbumKebab
                      size="sm"
                      platform={menuPlatform}
                      context="library"
                      album={albumRef}
                      onPlay={() => onSelectAlbum(a.album_key)}
                    />
                  </div>
                  <button
                    onClick={() => onSelectAlbum(a.album_key)}
                    className="flex flex-col gap-2 p-2 rounded-lg hover:bg-card/60 transition-colors text-left w-full"
                  >
                    <div className="aspect-square rounded-md overflow-hidden bg-muted">
                      {url ? (
                        <img
                          src={url}
                          alt={a.title}
                          loading="lazy"
                          className="w-full h-full object-cover"
                        />
                      ) : (
                        <div className="w-full h-full flex items-center justify-center">
                          <Disc3 className="h-10 w-10 text-muted-foreground/30" />
                        </div>
                      )}
                    </div>
                    <div className="min-w-0">
                      <p className="text-sm font-medium truncate">{a.title}</p>
                      <p className="text-[11px] text-muted-foreground/50">
                        {a.year ?? ''} · {a.track_count} track{a.track_count !== 1 ? 's' : ''}
                      </p>
                    </div>
                  </button>
                </div>
              </AlbumContextMenu>
            );
          })}
        </div>
      </div>

      <div>
        <h2 className="text-lg font-semibold mb-3">Tracks</h2>
        <div className="flex flex-col gap-0.5">
          {tracks.map((t, i) => {
            const serviceTrack = !isLocal
              ? {
                  id: trackIdFromUrl(t.path),
                  title: t.title ?? null,
                  artist: t.artist ?? display ?? null,
                  album: t.album ?? null,
                  url: t.path,
                  thumbnail: t.cover_id ? (coverMap[t.cover_id] ?? null) : null,
                  albumId: t.album_key ?? null,
                  artistId: artistKey,
                }
              : null;
            const localItems = serviceTrack ? null : localTrackItems(t, i);
            const kebab = serviceTrack ? (
              <TrackKebab
                size="sm"
                platform={menuPlatform}
                context="library"
                track={serviceTrack}
                onPlay={() => playFrom(i)}
              />
            ) : (
              <ActionsKebab size="sm" items={localItems!} />
            );
            const rowEl = (
              <div
                className="group grid grid-cols-[24px_minmax(0,3fr)_minmax(0,2fr)_60px] gap-3 px-3 py-2 items-center text-sm hover:bg-card/50 rounded cursor-pointer"
                onClick={() => playFrom(i)}
              >
                <div className="text-muted-foreground/40">{i + 1}</div>
                <div className="truncate font-medium">{t.title ?? ''}</div>
                <div className="truncate text-muted-foreground/60">{t.album ?? ''}</div>
                <div className="relative flex items-center justify-end text-right text-muted-foreground/50 tabular-nums">
                  <span className="group-hover:opacity-0">{fmt(t.duration_secs)}</span>
                  <div
                    className="absolute inset-y-0 right-0 hidden group-hover:flex items-center"
                    onClick={(e) => e.stopPropagation()}
                  >
                    {kebab}
                  </div>
                </div>
              </div>
            );
            if (serviceTrack) {
              return (
                <TrackContextMenu
                  key={`${t.path}-${i}`}
                  platform={menuPlatform}
                  context="library"
                  track={serviceTrack}
                  onPlay={() => playFrom(i)}
                >
                  {rowEl}
                </TrackContextMenu>
              );
            }
            return (
              <ContextMenu key={`${t.path}-${i}`}>
                <ContextMenuTrigger asChild>{rowEl}</ContextMenuTrigger>
                <ContextMenuContent>
                  <ActionItems items={localItems!} menuType="context" />
                </ContextMenuContent>
              </ContextMenu>
            );
          })}
        </div>
      </div>

      {!isLocal && pageShelves.length > 0 && (
        <div className="pt-2">
          <ServiceHomeView
            source={source as Exclude<ServicePlatform, 'local'>}
            shelvesOverride={pageShelves}
            onSelectAlbum={onSelectAlbum}
            onSelectArtist={onSelectArtist}
            onSelectPlaylist={onSelectPlaylist}
          />
        </div>
      )}
    </div>
  );
}

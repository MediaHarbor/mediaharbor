import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Play, Shuffle, ArrowLeft, FolderOpen, Disc3, Clock } from 'lucide-react';
import { SaveToggleHeart } from '@/features/library/components/SaveToggleHeart';
import { motion } from 'framer-motion';
import { Button } from '@/components/ui/button';
import {
  libraryKeys,
  ipcPlatformOf,
  text,
  trackIdFromUrl,
  queryAlbumPage,
  startLocalRadio,
  type AlbumDetailDto,
  type AlbumDto,
  type TrackDto,
  type ServicePlatform,
} from '@/features/library/api';
import { useLibraryStore } from '@/stores/useLibraryStore';
import { useCoverUrls } from '@/features/library/useCoverUrls';
import { usePlayerStore } from '@/stores/usePlayerStore';
import type { MediaType, PlayableTrack } from '@/stores/usePlayerStore';
import { TrackContextMenu, TrackKebab } from '@/features/library/actions/RowContextMenu';
import { ActionItems } from '@/features/library/actions/ActionItems';
import { ActionsKebab } from '@/features/library/actions/ActionsKebab';
import { buildLocalTrackItems } from '@/features/library/actions/localTrackItems';
import type { ActionItem } from '@/features/library/actions/types';
import { ContextMenu, ContextMenuTrigger, ContextMenuContent } from '@/components/ui/context-menu';
import { contextUriFor, normalizePlatform } from '@/utils/platform-data';
import { EntityLink } from '@/features/library/components/EntityLink';
import { ServiceHomeView } from '@/features/library/components/ServiceHomeView';
import { TrackInfoPanel } from '@/features/library/components/TrackInfoPanel';
import { formatDurationShort as formatDuration, formatTotalDuration } from '@/utils/formatters';
import { tauriAPI } from '@/tauri-bridge';

function randomIndex(length: number): number {
  return Math.floor(Math.random() * length);
}

function formatQuality(album: AlbumDto | null): string | null {
  if (!album?.codec) return null;
  const rate = album.sample_rate
    ? `${(album.sample_rate / 1000).toFixed(1).replace(/\.0$/, '')}`
    : null;
  if (album.bit_depth && rate) return `${album.codec} ${album.bit_depth}/${rate}`;
  if (rate) return `${album.codec} ${rate} kHz`;
  return album.codec;
}

interface AlbumDetailViewProps {
  albumKey: string;
  source?: ServicePlatform;
  onBack: () => void;
  onOpen: (path: string) => void;
}

export function AlbumDetailView({
  albumKey,
  source = 'local',
  onBack,
  onOpen,
}: AlbumDetailViewProps) {
  const [infoTrack, setInfoTrack] = useState<TrackDto | null>(null);
  const beginRadio = async (seed: TrackDto) => {
    const mix = await startLocalRadio(seed);
    if (mix.length > 0) setQueue(mix, 0);
  };
  const setQueue = usePlayerStore((s) => s.setQueue);
  const insertNext = usePlayerStore((s) => s.insertNext);
  const appendToQueue = usePlayerStore((s) => s.appendToQueue);
  const isLocal = source === 'local';
  const menuPlatform = normalizePlatform(source);

  const query = useQuery({
    queryKey: libraryKeys.album(albumKey, source),
    queryFn: async (): Promise<AlbumDetailDto | null> => {
      if (isLocal) {
        const r = (await tauriAPI.library.getAlbum?.(albumKey)) ?? null;
        return r as AlbumDetailDto | null;
      }
      const r = await tauriAPI.serviceLibrary?.album?.(source, albumKey);
      return r as unknown as AlbumDetailDto | null;
    },
  });

  const album = query.data?.album;
  const tracks: TrackDto[] = query.data?.tracks ?? [];

  const pushView = useLibraryStore((s) => s.pushView);
  const albumPageQuery = useQuery({
    queryKey: libraryKeys.albumPage(albumKey, source),
    queryFn: () => queryAlbumPage(source, albumKey),
    enabled: !isLocal,
    staleTime: 5 * 60 * 1000,
  });
  const pageShelves = albumPageQuery.data?.shelves ?? [];

  const coverUrls = useCoverUrls([album?.cover_id, ...tracks.map((t) => t.cover_id)]);
  const albumCover = album?.cover_id ? (coverUrls[album.cover_id] ?? null) : null;

  const totalDuration = tracks.reduce((acc, t) => acc + (t.duration_secs ?? 0), 0);
  const discCount = Math.max(
    new Set(tracks.map((t) => t.disc_no ?? 1)).size,
    album?.disc_total ?? 1
  );
  const multiDisc = discCount > 1;
  const years =
    album?.year && album.year_end && album.year_end !== album.year
      ? `${album.year}\u2013${album.year_end}`
      : (album?.year ?? null);
  const quality = formatQuality(album ?? null);
  const isCollection = album?.album_kind === 'collection';

  const platformForPlay = isLocal ? 'local' : ipcPlatformOf(source);

  const buildPlayable = (t: TrackDto, thumbnail: string | undefined): PlayableTrack => ({
    url: t.path,
    title: text(t.title) ?? '',
    artist: text(t.artist) ?? text(album?.artist) ?? '',
    album: text(album?.title),
    thumbnail,
    albumId: albumKey,
    artistId: text(t.artist_id) ?? text(album?.artist_id) ?? null,
    mediaType: t.is_video ? ('video' as MediaType) : ('audio' as MediaType),
    platform: platformForPlay,
  });

  const play = (startIndex: number) => {
    if (tracks.length === 0) return;
    const playable: PlayableTrack[] = tracks.map((t) => buildPlayable(t, albumCover ?? undefined));
    const contextUri = contextUriFor(source, 'album', albumKey);
    setQueue(playable, Math.min(startIndex, playable.length - 1), contextUri);
  };

  const shuffle = () => {
    if (tracks.length === 0) return;
    play(randomIndex(tracks.length));
  };

  const toPlayable = (t: TrackDto): PlayableTrack =>
    buildPlayable(t, (t.cover_id ? coverUrls[t.cover_id] : null) ?? albumCover ?? undefined);

  const localItemsFor = (t: TrackDto, index: number): ActionItem[] =>
    buildLocalTrackItems({
      playable: toPlayable(t),
      path: t.path,
      onPlay: () => play(index),
      insertNext,
      appendToQueue,
      onShowInFolder: () => onOpen(t.path),
      onShowInfo: () => setInfoTrack(t),
      onStartRadio: () => void beginRadio(t),
    });

  if (query.isPending) {
    return <div className="py-16 text-center text-sm text-muted-foreground/50">Loading…</div>;
  }
  if (!album) {
    return (
      <div className="py-16 text-center text-sm text-muted-foreground/50">Album not found.</div>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0, x: 40 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: -40 }}
      transition={{ duration: 0.25 }}
      className="space-y-6"
    >
      <button
        onClick={onBack}
        className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
      >
        <ArrowLeft className="h-4 w-4" />
        Library
      </button>

      <div className="flex gap-6 items-end">
        <div className="shrink-0">
          {albumCover ? (
            <img
              src={albumCover}
              alt={album.title}
              loading="lazy"
              className="h-48 w-48 rounded-lg object-cover shadow-xl"
            />
          ) : (
            <div className="h-48 w-48 rounded-lg bg-gradient-to-br from-muted to-muted/60 flex items-center justify-center shadow-xl">
              <Disc3 className="h-16 w-16 text-muted-foreground/20" />
            </div>
          )}
        </div>

        <div className="flex-1 min-w-0 pb-1">
          <p className="text-xs font-medium uppercase tracking-widest text-muted-foreground/60 mb-1">
            {isCollection ? 'Playlist' : 'Album'}
          </p>
          <h1 className="text-3xl font-bold truncate leading-tight">{album.title}</h1>
          <p className="text-base text-muted-foreground mt-1.5">
            <EntityLink kind="artist" id={album.artist_id} platform={menuPlatform} source={source}>
              {album.artist}
            </EntityLink>
          </p>
          <div className="flex flex-wrap items-center gap-2 mt-2 text-sm text-muted-foreground/60">
            {years && <span>{years}</span>}
            {years && <span>·</span>}
            <span>
              {tracks.length} song{tracks.length !== 1 ? 's' : ''}
            </span>
            {totalDuration > 0 && (
              <>
                <span>·</span>
                <span>{formatTotalDuration(totalDuration)}</span>
              </>
            )}
            {discCount > 1 && (
              <>
                <span>·</span>
                <span>{discCount} discs</span>
              </>
            )}
            {quality && (
              <>
                <span>·</span>
                <span className="rounded bg-muted/60 px-1.5 py-0.5 text-[11px] font-medium tracking-wide text-muted-foreground/80">
                  {quality}
                </span>
              </>
            )}
          </div>

          <div className="flex items-center gap-3 mt-5">
            <Button
              size="sm"
              className="rounded-full gap-2 px-6 shadow-md"
              onClick={() => play(0)}
              disabled={tracks.length === 0}
            >
              <Play className="h-4 w-4 ml-0.5" />
              Play
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="rounded-full gap-2 px-5"
              onClick={shuffle}
              disabled={tracks.length === 0}
            >
              <Shuffle className="h-3.5 w-3.5" />
              Shuffle
            </Button>
            {!isLocal && (
              <SaveToggleHeart
                platform={platformForPlay}
                kind="album"
                id={albumKey}
                size="md"
                visibility="always"
                label="Save album"
              />
            )}
            {isLocal && (
              <Button
                variant="ghost"
                size="icon"
                className="h-8 w-8 rounded-full"
                onClick={() => onOpen(tracks[0]?.path ?? '')}
                title="Show in folder"
              >
                <FolderOpen className="h-4 w-4" />
              </Button>
            )}
          </div>
        </div>
      </div>

      <div className="rounded-lg overflow-hidden">
        <div className="flex items-center gap-3 px-4 py-2 text-[11px] uppercase tracking-wider text-muted-foreground/40 border-b border-border/20">
          <span className="w-8 text-right">#</span>
          <span className="flex-1">Title</span>
          <Clock className="h-3 w-3 mr-1" />
        </div>

        {tracks.map((track, i) => {
          const trackCover = track.cover_id ? (coverUrls[track.cover_id] ?? null) : null;
          const serviceTrack = !isLocal
            ? {
                id: trackIdFromUrl(track.path),
                title: track.title ?? null,
                artist: album?.artist ?? null,
                album: album?.title ?? null,
                url: track.path,
                thumbnail: trackCover ?? albumCover ?? null,
                albumId: albumKey,
                artistId: track.artist_id ?? album?.artist_id ?? null,
              }
            : null;
          const localItems = serviceTrack ? null : localItemsFor(track, i);
          const kebab = serviceTrack ? (
            <TrackKebab
              size="sm"
              platform={menuPlatform}
              context="album"
              track={serviceTrack}
              onPlay={() => play(i)}
            />
          ) : (
            <ActionsKebab size="sm" items={localItems!} />
          );
          const rowEl = (
            <div
              className="group flex items-center gap-3 px-4 py-2.5 hover:bg-card/60 transition-colors cursor-pointer rounded-md"
              onClick={() => play(i)}
            >
              <div className="w-8 text-right shrink-0">
                <span className="text-[13px] text-muted-foreground/40 tabular-nums group-hover:hidden">
                  {track.track_no ?? i + 1}
                </span>
                <Play className="h-3.5 w-3.5 text-foreground hidden group-hover:inline-block ml-auto" />
              </div>
              <div className="flex-1 min-w-0">
                <p className="text-[13px] truncate font-medium">{track.title ?? ''}</p>
                {track.artist && track.artist !== album?.artist && (
                  <p className="text-[11px] truncate text-muted-foreground/60">{track.artist}</p>
                )}
              </div>
              {track.duration_secs && (
                <span className="text-[12px] text-muted-foreground/40 shrink-0 tabular-nums w-12 text-right">
                  {formatDuration(track.duration_secs)}
                </span>
              )}
              <div
                className="shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
                onClick={(e) => e.stopPropagation()}
              >
                {kebab}
              </div>
            </div>
          );
          const row = serviceTrack ? (
            <TrackContextMenu
              platform={menuPlatform}
              context="album"
              track={serviceTrack}
              onPlay={() => play(i)}
            >
              {rowEl}
            </TrackContextMenu>
          ) : (
            <ContextMenu>
              <ContextMenuTrigger asChild>{rowEl}</ContextMenuTrigger>
              <ContextMenuContent>
                <ActionItems items={localItems!} menuType="context" />
              </ContextMenuContent>
            </ContextMenu>
          );
          const disc = track.disc_no ?? 1;
          const showDiscHeader = multiDisc && (i === 0 || (tracks[i - 1].disc_no ?? 1) !== disc);
          return (
            <div key={track.path}>
              {showDiscHeader && (
                <div className="flex items-center gap-2 px-4 pt-4 pb-1.5 text-[11px] uppercase tracking-wider text-muted-foreground/40">
                  <Disc3 className="h-3 w-3" />
                  <span>Disc {disc}</span>
                </div>
              )}
              {row}
            </div>
          );
        })}
      </div>

      {!isLocal && pageShelves.length > 0 && (
        <div className="pt-2">
          <ServiceHomeView
            source={source as Exclude<ServicePlatform, 'local'>}
            shelvesOverride={pageShelves}
            onSelectAlbum={(key) => pushView({ kind: 'album', albumKey: key })}
            onSelectArtist={(key) => pushView({ kind: 'artist', artistKey: key })}
            onSelectPlaylist={(id, serviceId) =>
              pushView({ kind: 'playlist', id, serviceId: serviceId ?? null })
            }
          />
        </div>
      )}

      <TrackInfoPanel track={infoTrack} onOpenChange={(open) => !open && setInfoTrack(null)} />
    </motion.div>
  );
}

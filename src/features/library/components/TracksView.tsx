import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Disc3, Play } from 'lucide-react';
import {
  queryLibrary,
  ipcPlatformOf,
  type TrackDto,
  type ServicePlatform,
  startLocalRadio,
} from '@/features/library/api';
import { useInfiniteRows, useVisibleGridCovers } from '@/features/library/hooks/useVirtualRows';
import { useLibraryInfiniteQuery } from '@/features/library/hooks/useLibraryInfiniteQuery';
import { renderLibraryQueryState } from '@/features/library/components/LibraryQueryState';
import { usePlayerStore } from '@/stores/usePlayerStore';
import type { MediaType, PlayableTrack } from '@/stores/usePlayerStore';
import { SaveToggleHeart } from '@/features/library/components/SaveToggleHeart';
import { TrackContextMenu, TrackKebab } from '@/features/library/actions/RowContextMenu';
import { ActionItems } from '@/features/library/actions/ActionItems';
import { ActionsKebab } from '@/features/library/actions/ActionsKebab';
import { buildLocalTrackItems } from '@/features/library/actions/localTrackItems';
import { EntityLink } from '@/features/library/components/EntityLink';
import type { ActionItem } from '@/features/library/actions/types';
import { ContextMenu, ContextMenuTrigger, ContextMenuContent } from '@/components/ui/context-menu';
import { normalizePlatform } from '@/utils/platform-data';
import { formatDurationShort as fmt } from '@/utils/formatters';
import { TrackInfoPanel } from '@/features/library/components/TrackInfoPanel';

function LocalTrackMenu({ items, children }: { items: ActionItem[]; children: ReactNode }) {
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent>
        <ActionItems items={items} menuType="context" />
      </ContextMenuContent>
    </ContextMenu>
  );
}

interface TrackRowProps {
  t: TrackDto;
  index: number;
  url: string | null;
  size: number;
  start: number;
  showHeart: boolean;
  gridCols: string;
  platform: string;
  source: ServicePlatform;
  playFrom: (index: number) => void;
  onShowInfo: (t: TrackDto) => void;
  onStartRadio: (t: TrackDto) => void;
  insertNext: (track: PlayableTrack) => void;
  appendToQueue: (tracks: PlayableTrack[]) => void;
}

const TrackRow = memo(function TrackRow({
  t,
  index,
  url,
  size,
  start,
  showHeart,
  gridCols,
  platform,
  source,
  playFrom,
  insertNext,
  appendToQueue,
  onShowInfo,
  onStartRadio,
}: TrackRowProps) {
  const [armed, setArmed] = useState(false);
  const arm = useCallback(() => setArmed(true), []);

  const normalizedPlatform = useMemo(() => normalizePlatform(source), [source]);
  const trackId = useMemo(
    () =>
      showHeart
        ? t.path.includes('?v=')
          ? (t.path.split('?v=').pop()?.split('&')[0] ?? '')
          : (t.path.split('/').pop()?.split('?')[0] ?? '')
        : '',
    [showHeart, t.path]
  );
  const playable = useMemo<PlayableTrack>(
    () => ({
      url: t.path,
      title: t.title ?? '',
      artist: t.artist ?? '',
      album: t.album ?? null,
      thumbnail: url ?? undefined,
      albumId: t.album_key ?? null,
      artistId: t.artist_id ?? null,
      mediaType: (t.is_video ? 'video' : 'audio') as MediaType,
      platform,
    }),
    [t.path, t.title, t.artist, t.album, t.album_key, t.artist_id, t.is_video, url, platform]
  );
  const localItems = useMemo(
    () =>
      armed
        ? buildLocalTrackItems({
            playable,
            path: playable.url,
            onPlay: () => playFrom(index),
            insertNext,
            appendToQueue,
            onShowInfo: source === 'local' ? () => onShowInfo(t) : undefined,
            onStartRadio: source === 'local' ? () => onStartRadio(t) : undefined,
          })
        : [],
    [
      armed,
      playable,
      playFrom,
      index,
      insertNext,
      appendToQueue,
      onShowInfo,
      onStartRadio,
      source,
      t,
    ]
  );

  const rowKebab = !armed ? null : showHeart && trackId ? (
    <TrackKebab
      size="sm"
      platform={normalizedPlatform}
      context="library"
      track={{
        id: trackId,
        title: t.title ?? null,
        artist: t.artist ?? null,
        album: t.album ?? null,
        url: t.path,
        thumbnail: url,
        albumId: t.album_key ?? null,
        artistId: t.artist_id ?? null,
      }}
      onPlay={() => playFrom(index)}
    />
  ) : (
    <ActionsKebab items={localItems} size="sm" />
  );
  const rowInner = (
    <div
      className={`group grid ${gridCols} gap-3 px-3 items-center text-sm hover:bg-card/50 cursor-pointer`}
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        right: 0,
        height: size,
        transform: `translateY(${start}px)`,
        contain: 'layout paint',
      }}
      onDoubleClick={() => playFrom(index)}
      onPointerEnter={arm}
      onContextMenu={(e) => {
        if (!armed) {
          e.preventDefault();
          setArmed(true);
        }
      }}
    >
      <button
        onClick={(e) => {
          e.stopPropagation();
          playFrom(index);
        }}
        className="text-muted-foreground/40 group-hover:text-foreground"
      >
        <span className="group-hover:hidden tabular-nums">{index + 1}</span>
        <Play className="hidden group-hover:block h-3.5 w-3.5" />
      </button>
      {url ? (
        <img
          src={url}
          alt=""
          loading="lazy"
          decoding="async"
          className="h-8 w-8 rounded object-cover"
        />
      ) : (
        <div className="h-8 w-8 rounded bg-muted flex items-center justify-center">
          <Disc3 className="h-4 w-4 text-muted-foreground/30" />
        </div>
      )}
      <div className="truncate font-medium">{t.title ?? ''}</div>
      <EntityLink
        kind="artist"
        id={t.artist_id}
        platform={normalizedPlatform}
        source={source}
        className="text-muted-foreground/70"
      >
        {t.artist ?? ''}
      </EntityLink>
      <EntityLink
        kind="album"
        id={t.album_key}
        platform={normalizedPlatform}
        source={source}
        className="text-muted-foreground/70"
      >
        {t.album ?? ''}
      </EntityLink>
      {showHeart && trackId && (
        <div className="flex justify-end">
          <SaveToggleHeart platform={normalizedPlatform} kind="track" id={trackId} />
        </div>
      )}
      {showHeart && !trackId && <div />}
      <div className="relative flex items-center justify-end text-right text-muted-foreground/50 tabular-nums">
        <span className="group-hover:opacity-0">{fmt(t.duration_secs)}</span>
        <div
          className="absolute inset-y-0 right-0 hidden group-hover:flex items-center"
          onClick={(e) => e.stopPropagation()}
        >
          {rowKebab}
        </div>
      </div>
    </div>
  );
  if (!armed) return rowInner;
  if (showHeart && trackId) {
    return (
      <TrackContextMenu
        platform={normalizedPlatform}
        track={{
          id: trackId,
          title: t.title ?? null,
          artist: t.artist ?? null,
          album: t.album ?? null,
          url: t.path,
          thumbnail: url,
          albumId: t.album_key ?? null,
          artistId: t.artist_id ?? null,
        }}
        context="library"
        onPlay={() => playFrom(index)}
      >
        {rowInner}
      </TrackContextMenu>
    );
  }
  return <LocalTrackMenu items={localItems}>{rowInner}</LocalTrackMenu>;
});

interface TracksViewProps {
  search: string;
  sort: string;
  source?: ServicePlatform;
}

export function TracksView({ search, sort, source = 'local' }: TracksViewProps) {
  const [infoTrack, setInfoTrack] = useState<TrackDto | null>(null);
  const setQueue = usePlayerStore((s) => s.setQueue);
  const beginRadio = useCallback(
    async (seed: TrackDto) => {
      const mix = await startLocalRadio(seed);
      if (mix.length > 0) setQueue(mix, 0);
    },
    [setQueue]
  );
  const hydrateQueue = usePlayerStore((s) => s.hydrateQueue);
  const insertNext = usePlayerStore((s) => s.insertNext);
  const appendToQueue = usePlayerStore((s) => s.appendToQueue);
  const parentRef = useRef<HTMLDivElement>(null);

  const { query, items, total } = useLibraryInfiniteQuery<TrackDto>('tracks', {
    sort,
    search,
    source,
  });

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 48,
    overscan: 6,
  });

  const virtualItems = virtualizer.getVirtualItems();

  useInfiniteRows(query, virtualItems, items.length, 50);

  const coverUrls = useVisibleGridCovers(virtualItems, items, 1);

  const platform = source === 'local' ? 'local' : ipcPlatformOf(source);
  const itemsRef = useRef(items);
  const coverUrlsRef = useRef(coverUrls);
  const totalRef = useRef(total);
  useEffect(() => {
    itemsRef.current = items;
    coverUrlsRef.current = coverUrls;
    totalRef.current = total;
  });
  const toPlayable = useCallback(
    (t: TrackDto, covers: Record<string, string | null>): PlayableTrack => ({
      url: t.path,
      title: t.title ?? '',
      artist: t.artist ?? '',
      thumbnail: t.cover_id ? (covers[t.cover_id] ?? undefined) : undefined,
      coverId: t.cover_id ?? undefined,
      mediaType: (t.is_video ? 'video' : 'audio') as MediaType,
      platform,
    }),
    [platform]
  );
  const playFrom = useCallback(
    (index: number) => {
      const list = itemsRef.current;
      if (list.length === 0) return;
      setQueue(
        list.map((t) => toPlayable(t, coverUrlsRef.current)),
        Math.min(index, list.length - 1)
      );
      const totalCount = totalRef.current;
      if (totalCount && totalCount > list.length) {
        void (async () => {
          const CHUNK = 500;
          const all: TrackDto[] = [];
          for (let offset = 0; offset < totalCount; offset += CHUNK) {
            const page = await queryLibrary<TrackDto>({
              kind: 'tracks',
              offset,
              limit: CHUNK,
              sort,
              search,
              source,
            });
            all.push(...page.items);
            if (page.items.length < CHUNK) break;
          }
          if (all.length > list.length) {
            hydrateQueue(all.map((t) => toPlayable(t, coverUrlsRef.current)));
          }
        })().catch(() => {});
      }
    },
    [setQueue, hydrateQueue, toPlayable, sort, search, source]
  );

  const stateView = renderLibraryQueryState({
    query,
    entity: 'tracks',
    source,
    count: items.length,
  });
  if (stateView) return stateView;

  const showHeart = source !== 'local';
  const gridCols = showHeart
    ? 'grid-cols-[24px_40px_minmax(0,3fr)_minmax(0,2fr)_minmax(0,2fr)_28px_60px]'
    : 'grid-cols-[24px_40px_minmax(0,3fr)_minmax(0,2fr)_minmax(0,2fr)_60px]';

  return (
    <div ref={parentRef} className="h-full overflow-y-auto">
      <div
        className={`grid ${gridCols} gap-3 px-3 py-2 text-[11px] uppercase tracking-wide text-muted-foreground/50 sticky top-0 bg-background z-10 border-b border-border/20`}
      >
        <div>#</div>
        <div></div>
        <div>Title</div>
        <div>Artist</div>
        <div>Album</div>
        {showHeart && <div></div>}
        <div className="text-right">Time</div>
      </div>
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
        {virtualizer.getVirtualItems().map((vi) => {
          const t = items[vi.index];
          if (!t) return null;
          const url = t.cover_id ? (coverUrls[t.cover_id] ?? null) : null;
          return (
            <TrackRow
              key={`${t.path}-${vi.index}`}
              t={t}
              index={vi.index}
              url={url}
              size={vi.size}
              start={vi.start}
              showHeart={showHeart}
              gridCols={gridCols}
              platform={platform}
              source={source}
              playFrom={playFrom}
              insertNext={insertNext}
              appendToQueue={appendToQueue}
              onShowInfo={setInfoTrack}
              onStartRadio={beginRadio}
            />
          );
        })}
      </div>
      <TrackInfoPanel track={infoTrack} onOpenChange={(open) => !open && setInfoTrack(null)} />
    </div>
  );
}

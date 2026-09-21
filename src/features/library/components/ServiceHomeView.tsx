import { useQuery } from '@tanstack/react-query';
import { Play, Copy } from 'lucide-react';
import {
  libraryKeys,
  queryRecommendations,
  queryExplorePage,
  ipcPlatformOf,
  errorMessage,
  text,
  trackIdFromUrl,
  type ServicePlatform,
} from '@/features/library/api';
import { useCoverUrls } from '@/features/library/useCoverUrls';
import { useMemo, type ReactNode } from 'react';
import { usePlayerStore } from '@/stores/usePlayerStore';
import type { MediaType, PlayableTrack } from '@/stores/usePlayerStore';
import {
  AlbumContextMenu,
  ArtistContextMenu,
  PlaylistContextMenu,
  TrackContextMenu,
  EpisodeContextMenu,
  AlbumKebab,
  ArtistKebab,
  PlaylistKebab,
  TrackKebab,
  EpisodeKebab,
} from '@/features/library/actions/RowContextMenu';
import {
  ContextMenu,
  ContextMenuTrigger,
  ContextMenuContent,
  ContextMenuItem,
} from '@/components/ui/context-menu';
import { contextUriFor, externalUrl, normalizePlatform } from '@/utils/platform-data';
import { ShelfSection } from './shelves/ShelfRow';
import { HeroCard, MixTile, RankedRow, ShortcutTile, StandardCard } from './shelves/ShelfCards';
import { copyText } from '@/utils/clipboard';
import { tauriAPI } from '@/tauri-bridge';
import { logWarning } from '@/utils/logger';
import { radioTracksToQueue } from '@/features/library/actions/shared';
import {
  layoutFor,
  pickCoverId,
  pickSubtitle,
  pickTitle,
  type Shelf,
  type ShelfItem,
  type ShelfLayout,
} from './shelves/layouts';

interface ServiceHomeViewProps {
  source: Exclude<ServicePlatform, 'local'>;
  /** When set, render a browse-category page (from an explore PageLink apiPath)
   *  instead of the recommendations home feed. */
  explorePath?: string;
  /** Called when a browse pill (genre/label/mood) is clicked — drills deeper. */
  onOpenCategory?: (apiPath: string, title: string) => void;
  onSelectAlbum?: (albumKey: string) => void;
  onSelectArtist?: (artistKey: string) => void;
  onSelectPlaylist?: (id: number, serviceId?: string | null) => void;
  /** When set, render these shelves directly instead of fetching the home feed
   *  (used by ArtistDetailView to show artist_page shelves with the same renderer). */
  shelvesOverride?: Shelf[];
}

export type { Shelf, ShelfItem };

export function ServiceHomeView({
  source,
  explorePath,
  onOpenCategory,
  onSelectAlbum,
  onSelectArtist,
  onSelectPlaylist,
  shelvesOverride,
}: ServiceHomeViewProps) {
  const setQueue = usePlayerStore((s) => s.setQueue);
  const platform = ipcPlatformOf(source);

  const handleClick = (it: ShelfItem, coverUrl?: string | null) => {
    switch (it.kind) {
      case 'album':
        if (typeof it.album_key === 'string') onSelectAlbum?.(it.album_key);
        break;
      case 'artist':
        if (typeof it.key === 'string') onSelectArtist?.(it.key);
        break;
      case 'playlist':
        if (typeof it.id === 'number') {
          onSelectPlaylist?.(it.id, (it.service_id as string | undefined) ?? null);
        }
        break;
      case 'mix': {
        const mixUrl = typeof it.url === 'string' ? it.url : '';
        if (mixUrl) {
          void tauriAPI.app.openExternal(mixUrl);
          return;
        }
        if (typeof it.id === 'string' && it.id) onSelectPlaylist?.(0, it.id);
        break;
      }
      case 'page_link':
        if (typeof it.api_path === 'string' && it.api_path) {
          onOpenCategory?.(it.api_path, (it.title as string | undefined) ?? '');
        }
        break;
      case 'episode': {
        const uri = typeof it.id === 'string' ? it.id : '';
        const epId = uri.startsWith('spotify:episode:') ? uri.slice('spotify:episode:'.length) : '';
        const epUrl = externalUrl({ platform: 'spotify', kind: 'episode', id: epId });
        if (epUrl) void tauriAPI.app.openExternal(epUrl);
        return;
      }
      case 'track': {
        const path = typeof it.path === 'string' ? it.path : '';
        if (!path) return;
        const albumKey =
          normalizePlatform(source) === 'spotify' && typeof it.album_key === 'string'
            ? it.album_key
            : null;
        const trackNo = typeof it.track_no === 'number' ? it.track_no : null;
        const playable: PlayableTrack = {
          url: path,
          title: (it.title as string | undefined) ?? '',
          artist: (it.artist as string | undefined) ?? '',
          album: (it.album as string | undefined) ?? null,
          thumbnail: coverUrl ?? undefined,
          coverId: (it.cover_id as string | undefined) ?? null,
          albumId: (it.album_key as string | undefined) ?? null,
          artistId: (it.artist_id as string | undefined) ?? null,
          mediaType: 'audio' as MediaType,
          platform,
          contextUri: contextUriFor(platform, 'album', albumKey),
          trackIndex: albumKey && trackNo ? Math.max(0, trackNo - 1) : null,
        };
        setQueue([playable], 0);
        break;
      }
    }
  };

  /// The play badge on a card starts playback; the rest of the card still navigates.
  /// `null` for kinds with nothing to play, which leaves the badge inert.
  const playHandlerFor = (it: ShelfItem, coverUrl?: string | null): (() => void) | undefined => {
    // Album/playlist rows can lack artist and artwork; the header fills them.
    const queueRows = (
      rows: Record<string, unknown>[],
      contextUri: string | null,
      header?: { artist?: string | null; album?: string | null; coverId?: string | null }
    ) => {
      const playable: PlayableTrack[] = rows
        .filter((t) => typeof t.path === 'string' && t.path)
        .map((t) => ({
          url: t.path as string,
          title: text(t.title) ?? '',
          artist: text(t.artist) ?? header?.artist ?? '',
          album: text(t.album) ?? header?.album ?? null,
          coverId: text(t.cover_id) ?? header?.coverId ?? null,
          albumId: text(t.album_key) ?? null,
          artistId: text(t.artist_id) ?? null,
          mediaType: (t.is_video === true ? 'video' : 'audio') as MediaType,
          platform,
        }));
      if (playable.length === 0) return;
      setQueue(playable, 0, contextUri);
    };

    const run = (work: () => Promise<void>) => () => {
      void work().catch((e) =>
        logWarning('playback', 'Could not play this item', `${source}: ${errorMessage(e)}`)
      );
    };

    switch (it.kind) {
      case 'track':
        return () => handleClick(it, coverUrl);
      case 'album': {
        const key = typeof it.album_key === 'string' ? it.album_key : '';
        if (!key) return undefined;
        return run(async () => {
          const detail = await tauriAPI.serviceLibrary.album(source, key);
          const header = detail?.album ?? {};
          queueRows(
            (detail?.tracks ?? []) as Record<string, unknown>[],
            contextUriFor(platform, 'album', key),
            {
              artist: text(header.artist),
              album: text(header.title),
              coverId: text(header.cover_id),
            }
          );
        });
      }
      case 'playlist':
      case 'mix': {
        const id =
          typeof it.service_id === 'string' && it.service_id
            ? it.service_id
            : typeof it.id === 'string'
              ? it.id
              : '';
        if (!id) return undefined;
        return run(async () => {
          const detail = await tauriAPI.serviceLibrary.playlist(source, id);
          const covers = detail?.playlist?.cover_ids;
          queueRows(
            (detail?.tracks ?? []) as Record<string, unknown>[],
            contextUriFor(platform, 'playlist', id),
            { coverId: Array.isArray(covers) ? text(covers[0]) : null }
          );
        });
      }
      case 'artist': {
        const key = typeof it.key === 'string' ? it.key : '';
        if (!key) return undefined;
        return run(async () => {
          const res = await tauriAPI.serviceLibrary.radioFor({
            platform: normalizePlatform(source),
            seedKind: 'artist',
            seedId: key,
          });
          const q = await radioTracksToQueue(res, platform);
          if (q.length) setQueue(q, 0);
        });
      }
      default:
        return undefined;
    }
  };

  const recsQuery = useQuery({
    queryKey: explorePath
      ? libraryKeys.explorePage(source, explorePath)
      : libraryKeys.recommendations(source),
    queryFn: () =>
      explorePath ? queryExplorePage(source, explorePath) : queryRecommendations(source),
    enabled: shelvesOverride === undefined,
  });

  const shelves: Shelf[] = useMemo(
    () => shelvesOverride ?? ((recsQuery.data?.shelves ?? []) as Shelf[]),
    [shelvesOverride, recsQuery.data?.shelves]
  );
  const allCoverIds = useMemo(() => {
    const ids: string[] = [];
    for (const sh of shelves) {
      for (const it of sh.items) {
        const cid = pickCoverId(it);
        if (cid) ids.push(cid);
      }
    }
    return ids;
  }, [shelves]);
  const covers = useCoverUrls(allCoverIds);

  if (shelvesOverride === undefined && recsQuery.isPending) {
    return (
      <div className="py-16 text-center text-sm text-muted-foreground/50">
        Loading {source} home…
      </div>
    );
  }
  if (shelvesOverride === undefined && recsQuery.isError) {
    return (
      <div className="py-16 text-center text-sm text-red-400/80 whitespace-pre-wrap">
        {`Failed to load ${source} home:\n${errorMessage(recsQuery.error)}`}
      </div>
    );
  }
  if (shelves.length === 0) {
    if (shelvesOverride !== undefined) return null;
    return (
      <div className="py-16 text-center text-sm text-muted-foreground/50">
        No recommendations from {source}.
      </div>
    );
  }

  return (
    <div className="space-y-8 pb-12">
      {shelves.map((shelf, shelfIdx) => (
        <ShelfBody
          key={shelf.id || `${shelf.title}-${shelfIdx}`}
          shelf={shelf}
          layout={layoutFor(shelf)}
          covers={covers}
          platform={normalizePlatform(source)}
          onOpenCategory={onOpenCategory}
          onOpenItem={handleClick}
          onPlayItem={playHandlerFor}
        />
      ))}
    </div>
  );
}

interface ShelfBodyProps {
  shelf: Shelf;
  layout: ShelfLayout;
  covers: Record<string, string | null>;
  platform: string;
  onOpenCategory?: (apiPath: string, title: string) => void;
  onOpenItem: (it: ShelfItem, coverUrl?: string | null) => void;
  onPlayItem: (it: ShelfItem, coverUrl?: string | null) => (() => void) | undefined;
}

function ShelfBody({
  shelf,
  layout,
  covers,
  platform,
  onOpenCategory,
  onOpenItem,
  onPlayItem,
}: ShelfBodyProps) {
  const coverOf = (it: ShelfItem) => {
    const key = pickCoverId(it);
    return key ? (covers[key] ?? null) : null;
  };
  const wrap = (it: ShelfItem, i: number, node: ReactNode) => {
    const url = coverOf(it);
    return (
      <ShelfItemMenu
        key={i}
        item={it}
        platform={platform}
        coverUrl={url}
        onOpen={() => onOpenItem(it, url)}
        onPlay={onPlayItem(it, url)}
      >
        {node}
      </ShelfItemMenu>
    );
  };

  if (layout === 'genre-pills') {
    return (
      <ShelfSection title={shelf.title} subtitle={shelf.subtitle} staticBody>
        <div className="flex flex-wrap gap-2 pb-1">
          {shelf.items.map((it, i) => (
            <button
              key={(it.api_path as string) || i}
              type="button"
              onClick={() =>
                onOpenCategory?.((it.api_path as string) ?? '', (it.title as string) ?? '')
              }
              className={
                'inline-flex items-center rounded-full border border-border/60 bg-card/40 ' +
                'px-4 py-2 text-[13px] font-medium text-foreground/80 ' +
                'hover:text-foreground hover:border-[color:var(--service-accent)] hover:bg-card/80 ' +
                'transition-colors duration-150'
              }
            >
              {pickTitle(it)}
            </button>
          ))}
        </div>
      </ShelfSection>
    );
  }

  if (layout === 'shortcuts') {
    return (
      <ShelfSection title={shelf.title} subtitle={shelf.subtitle} staticBody>
        <div className="grid grid-flow-col grid-rows-2 gap-2 overflow-x-auto scrollbar-shelf pb-2 auto-cols-[minmax(240px,1fr)] lg:grid-flow-row lg:auto-cols-auto lg:grid-cols-4">
          {shelf.items.slice(0, 8).map((it, i) => {
            const url = coverOf(it);
            return wrap(
              it,
              i,
              <ShortcutTile
                item={it}
                coverUrl={url}
                onOpen={() => onOpenItem(it, url)}
                onPlay={onPlayItem(it, url)}
              />
            );
          })}
        </div>
      </ShelfSection>
    );
  }

  if (layout === 'ranked') {
    const showRank = shelf.category === 'charts';
    return (
      <ShelfSection title={shelf.title} subtitle={shelf.subtitle} staticBody>
        <div className="grid grid-cols-1 gap-x-6 gap-y-0.5 lg:grid-cols-2">
          {shelf.items.slice(0, 10).map((it, i) => {
            const url = coverOf(it);
            return wrap(
              it,
              i,
              <RankedRow
                item={it}
                coverUrl={url}
                rank={i + 1}
                showRank={showRank}
                onOpen={() => onOpenItem(it, url)}
                onPlay={onPlayItem(it, url)}
              />
            );
          })}
        </div>
      </ShelfSection>
    );
  }

  return (
    <ShelfSection title={shelf.title} subtitle={shelf.subtitle} rowClassName="gap-1">
      {shelf.items.map((it, i) => {
        const url = coverOf(it);
        const open = () => onOpenItem(it, url);
        const play = onPlayItem(it, url);
        let card: ReactNode;
        if (layout === 'hero') {
          card = <HeroCard item={it} coverUrl={url} onOpen={open} onPlay={play} />;
        } else if (layout === 'mix-tiles') {
          card = <MixTile item={it} coverUrl={url} onOpen={open} onPlay={play} />;
        } else {
          card = (
            <StandardCard
              onPlay={play}
              item={it}
              coverUrl={url}
              onOpen={open}
              compact={layout === 'compact'}
              showYear={shelf.category === 'new_releases'}
            />
          );
        }
        return wrap(it, i, card);
      })}
    </ShelfSection>
  );
}

function ShelfItemMenu({
  item,
  platform,
  coverUrl,
  onOpen,
  onPlay,
  children,
}: {
  item: ShelfItem;
  platform: string;
  coverUrl: string | null;
  onOpen: () => void;
  /// Absent when the item has nothing to play; the menu then falls back to opening it.
  onPlay?: () => void;
  children: ReactNode;
}) {
  const id =
    typeof item.album_key === 'string'
      ? item.album_key
      : typeof item.key === 'string'
        ? item.key
        : typeof item.service_id === 'string'
          ? item.service_id
          : typeof item.id === 'string'
            ? item.id
            : null;
  const title = pickTitle(item);

  const withKebab = (kebab: ReactNode) => (
    <div className="group relative">
      {children}
      <div
        className="absolute top-2 right-2 z-10 opacity-0 group-hover:opacity-100 transition-opacity rounded-md bg-background/70 backdrop-blur-sm"
        onClick={(e) => e.stopPropagation()}
      >
        {kebab}
      </div>
    </div>
  );

  if (item.kind === 'album' && id) {
    const album = {
      id,
      title,
      artist: pickSubtitle(item),
      cover_url: coverUrl,
      artistId: typeof item.artist_id === 'string' ? item.artist_id : null,
    };
    return (
      <AlbumContextMenu
        platform={platform}
        album={album}
        context="library"
        onPlay={onPlay ?? onOpen}
      >
        {withKebab(
          <AlbumKebab
            size="sm"
            platform={platform}
            album={album}
            context="library"
            onPlay={onPlay ?? onOpen}
          />
        )}
      </AlbumContextMenu>
    );
  }
  if (item.kind === 'artist' && id) {
    const artist = { id, display: title, cover_url: coverUrl };
    return (
      <ArtistContextMenu platform={platform} artist={artist} context="library">
        {withKebab(<ArtistKebab size="sm" platform={platform} artist={artist} context="library" />)}
      </ArtistContextMenu>
    );
  }
  if (item.kind === 'playlist' && typeof item.service_id === 'string') {
    const playlist = { id: item.service_id, name: title, cover_url: coverUrl, owned: false };
    return (
      <PlaylistContextMenu
        platform={platform}
        playlist={playlist}
        context="library"
        onPlay={onPlay ?? onOpen}
      >
        {withKebab(
          <PlaylistKebab
            size="sm"
            platform={platform}
            playlist={playlist}
            context="library"
            onPlay={onPlay ?? onOpen}
          />
        )}
      </PlaylistContextMenu>
    );
  }
  if (item.kind === 'mix') {
    const playlist = { id: id ?? '', name: title, cover_url: coverUrl, owned: false };
    return (
      <PlaylistContextMenu
        platform={platform}
        playlist={playlist}
        context="library"
        onPlay={onPlay ?? onOpen}
      >
        {withKebab(
          <PlaylistKebab
            size="sm"
            platform={platform}
            playlist={playlist}
            context="library"
            onPlay={onPlay ?? onOpen}
          />
        )}
      </PlaylistContextMenu>
    );
  }
  if (item.kind === 'episode') {
    const episode = {
      id: typeof item.id === 'string' ? item.id : (id ?? ''),
      title,
      show: typeof item.show_name === 'string' ? item.show_name : null,
      url: typeof item.url === 'string' ? item.url : null,
      cover_url: coverUrl,
    };
    return (
      <EpisodeContextMenu platform={platform} episode={episode} context="library">
        {withKebab(
          <EpisodeKebab size="sm" platform={platform} episode={episode} context="library" />
        )}
      </EpisodeContextMenu>
    );
  }
  if (item.kind === 'track') {
    const path = typeof item.path === 'string' ? item.path : '';
    const track = {
      id: trackIdFromUrl(path) || (id ?? ''),
      title,
      artist: pickSubtitle(item),
      url: path || null,
      thumbnail: coverUrl,
      albumId: typeof item.album_key === 'string' ? item.album_key : undefined,
      artistId: typeof item.artist_id === 'string' ? item.artist_id : undefined,
    };
    return (
      <TrackContextMenu
        platform={platform}
        track={track}
        context="library"
        onPlay={onPlay ?? onOpen}
      >
        {withKebab(
          <TrackKebab
            size="sm"
            platform={platform}
            track={track}
            context="library"
            onPlay={onPlay ?? onOpen}
          />
        )}
      </TrackContextMenu>
    );
  }
  const link = typeof item.url === 'string' ? item.url : null;
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div className="relative">{children}</div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={onOpen}>
          <Play className="h-4 w-4 mr-2" /> Open
        </ContextMenuItem>
        {link && (
          <ContextMenuItem
            onSelect={() => {
              void copyText(link);
            }}
          >
            <Copy className="h-4 w-4 mr-2" /> Copy link
          </ContextMenuItem>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}

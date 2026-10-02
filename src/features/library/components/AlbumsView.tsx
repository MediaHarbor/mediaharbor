import React, { useRef, type ReactNode } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Disc3, Play } from 'lucide-react';
import { motion } from 'framer-motion';
import { type AlbumDto, type ServicePlatform } from '@/features/library/api';
import { useInfiniteRows, useVisibleGridCovers } from '@/features/library/hooks/useVirtualRows';
import { useLibraryInfiniteQuery } from '@/features/library/hooks/useLibraryInfiniteQuery';
import { useElementWidth } from '@/features/library/hooks/useElementWidth';
import { renderLibraryQueryState } from '@/features/library/components/LibraryQueryState';
import { AlbumContextMenu, AlbumKebab } from '@/features/library/actions/RowContextMenu';
import { normalizePlatform } from '@/utils/platform-data';

interface AlbumsViewProps {
  view: 'grid' | 'list';
  search: string;
  sort: string;
  source?: ServicePlatform;
  onSelectAlbum: (album_key: string) => void;
  onPlayAlbum: (album_key: string) => void;
}

function columnsFor(width: number): number {
  if (width >= 1536) return 7;
  if (width >= 1280) return 6;
  if (width >= 1024) return 5;
  if (width >= 768) return 4;
  if (width >= 640) return 3;
  return 2;
}

export function AlbumsView({
  view,
  search,
  sort,
  source = 'local',
  onSelectAlbum,
  onPlayAlbum,
}: AlbumsViewProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const width = useElementWidth(parentRef);
  const { query, items } = useLibraryInfiniteQuery<AlbumDto>('albums', {
    sort,
    search,
    source,
  });

  const cols = view === 'list' ? 1 : columnsFor(width);
  const rowCount = view === 'list' ? items.length : Math.ceil(items.length / cols);
  const rowHeight = view === 'list' ? 72 : Math.round(width / cols + 64);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => parentRef.current,
    estimateSize: () => rowHeight,
    overscan: 4,
  });

  const virtualItems = virtualizer.getVirtualItems();

  useInfiniteRows(query, virtualItems, rowCount);

  const coverUrls = useVisibleGridCovers(virtualItems, items, cols);

  const stateView = renderLibraryQueryState({
    query,
    entity: 'albums',
    source,
    count: items.length,
  });
  if (stateView) return stateView;

  return (
    <div ref={parentRef} className="h-full overflow-y-auto">
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
        {virtualizer.getVirtualItems().map((vi) => {
          if (view === 'list') {
            const album = items[vi.index];
            if (!album) return null;
            const coverUrl = album.cover_id ? (coverUrls[album.cover_id] ?? null) : null;
            return (
              <AlbumContextMenu
                key={album.album_key}
                platform={normalizePlatform(source)}
                album={{
                  id: album.album_key,
                  title: album.title,
                  artist: album.artist,
                  cover_url: coverUrl,
                  artistId: album.artist_id ?? null,
                }}
                context="library"
                onPlay={() => onPlayAlbum(album.album_key)}
                copyLink={undefined}
              >
                <ListRow
                  album={album}
                  coverUrl={coverUrl}
                  onSelect={() => onSelectAlbum(album.album_key)}
                  onPlay={() => onPlayAlbum(album.album_key)}
                  menu={
                    <AlbumKebab
                      size="sm"
                      platform={normalizePlatform(source)}
                      context="library"
                      album={{
                        id: album.album_key,
                        title: album.title,
                        artist: album.artist,
                        cover_url: coverUrl,
                        artistId: album.artist_id ?? null,
                      }}
                      onPlay={() => onPlayAlbum(album.album_key)}
                    />
                  }
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    right: 0,
                    height: vi.size,
                    transform: `translateY(${vi.start}px)`,
                  }}
                />
              </AlbumContextMenu>
            );
          }
          const start = vi.index * cols;
          const slice = items.slice(start, start + cols);
          return (
            <div
              key={vi.index}
              className="grid gap-1 px-px"
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                right: 0,
                height: vi.size,
                transform: `translateY(${vi.start}px)`,
                gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`,
              }}
            >
              {slice.map((album, j) => {
                const coverUrl = album.cover_id ? (coverUrls[album.cover_id] ?? null) : null;
                return (
                  <AlbumContextMenu
                    key={album.album_key}
                    platform={normalizePlatform(source)}
                    album={{
                      id: album.album_key,
                      title: album.title,
                      artist: album.artist,
                      cover_url: coverUrl,
                      artistId: album.artist_id ?? null,
                    }}
                    context="library"
                    onPlay={() => onPlayAlbum(album.album_key)}
                    copyLink={undefined}
                  >
                    <GridCard
                      album={album}
                      coverUrl={coverUrl}
                      onSelect={() => onSelectAlbum(album.album_key)}
                      onPlay={() => onPlayAlbum(album.album_key)}
                      index={start + j}
                      menu={
                        <AlbumKebab
                          size="sm"
                          platform={normalizePlatform(source)}
                          context="library"
                          album={{
                            id: album.album_key,
                            title: album.title,
                            artist: album.artist,
                            cover_url: coverUrl,
                            artistId: album.artist_id ?? null,
                          }}
                          onPlay={() => onPlayAlbum(album.album_key)}
                        />
                      }
                    />
                  </AlbumContextMenu>
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
}

const GridCard = React.forwardRef<
  HTMLDivElement,
  {
    album: AlbumDto;
    coverUrl: string | null;
    onSelect: () => void;
    onPlay: () => void;
    index: number;
    menu?: ReactNode;
  } & React.HTMLAttributes<HTMLDivElement>
>(function GridCard({ album, coverUrl, onSelect, onPlay, index, menu, ...rest }, ref) {
  return (
    <div ref={ref} {...rest}>
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.15, delay: Math.min(index * 0.005, 0.1) }}
        className="group p-2 rounded-md cursor-pointer hover:bg-card/50 transition-colors"
        onClick={onSelect}
      >
        <div className="relative aspect-square rounded-md overflow-hidden mb-2 shadow-md shadow-black/20">
          {coverUrl ? (
            <img
              src={coverUrl}
              alt={album.title}
              loading="lazy"
              className="w-full h-full object-cover"
            />
          ) : (
            <div className="w-full h-full flex items-center justify-center bg-muted">
              <Disc3 className="h-10 w-10 text-muted-foreground/20" />
            </div>
          )}
          <button
            className="absolute bottom-2 right-2 h-9 w-9 rounded-full bg-primary text-primary-foreground opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center shadow-lg"
            onClick={(e) => {
              e.stopPropagation();
              onPlay();
            }}
          >
            <Play className="h-4 w-4 ml-0.5" />
          </button>
          {menu && (
            <div
              className="absolute top-1.5 right-1.5 opacity-0 group-hover:opacity-100 transition-opacity rounded-md bg-background/70 backdrop-blur-sm"
              onClick={(e) => e.stopPropagation()}
            >
              {menu}
            </div>
          )}
        </div>
        <p className="text-sm font-medium truncate">{album.title}</p>
        <p className="text-[11px] text-muted-foreground/60 truncate">{album.artist}</p>
        <p className="text-[10px] text-muted-foreground/40 truncate">
          {album.track_count} track{album.track_count !== 1 ? 's' : ''}
          {album.year ? ` · ${album.year}` : ''}
        </p>
      </motion.div>
    </div>
  );
});

const ListRow = React.forwardRef<
  HTMLDivElement,
  {
    album: AlbumDto;
    coverUrl: string | null;
    onSelect: () => void;
    onPlay: () => void;
    style: React.CSSProperties;
    menu?: ReactNode;
  } & React.HTMLAttributes<HTMLDivElement>
>(function ListRow({ album, coverUrl, onSelect, onPlay, style, menu, ...rest }, ref) {
  return (
    <div
      ref={ref}
      style={style}
      className="group flex items-center gap-3 px-3 hover:bg-card/50 cursor-pointer"
      onClick={onSelect}
      {...rest}
    >
      {coverUrl ? (
        <img
          src={coverUrl}
          alt={album.title}
          loading="lazy"
          className="h-12 w-12 rounded object-cover"
        />
      ) : (
        <div className="h-12 w-12 rounded bg-muted flex items-center justify-center">
          <Disc3 className="h-5 w-5 text-muted-foreground/30" />
        </div>
      )}
      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium truncate">{album.title}</p>
        <p className="text-[11px] text-muted-foreground/60 truncate">{album.artist}</p>
      </div>
      <div className="text-[11px] text-muted-foreground/40 tabular-nums shrink-0">
        {album.track_count}
      </div>
      <button
        className="h-8 w-8 rounded-full flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity hover:bg-primary/10"
        onClick={(e) => {
          e.stopPropagation();
          onPlay();
        }}
      >
        <Play className="h-3.5 w-3.5 ml-0.5" />
      </button>
      {menu && (
        <div
          className="opacity-0 group-hover:opacity-100 transition-opacity"
          onClick={(e) => e.stopPropagation()}
        >
          {menu}
        </div>
      )}
    </div>
  );
});

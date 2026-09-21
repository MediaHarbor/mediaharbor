import { memo, useCallback, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { User } from 'lucide-react';
import { type ServicePlatform } from '@/features/library/api';
import { useInfiniteRows, useVisibleGridCovers } from '@/features/library/hooks/useVirtualRows';
import { useLibraryInfiniteQuery } from '@/features/library/hooks/useLibraryInfiniteQuery';
import { useElementWidth } from '@/features/library/hooks/useElementWidth';
import { renderLibraryQueryState } from '@/features/library/components/LibraryQueryState';
import { ArtistContextMenu, ArtistKebab } from '@/features/library/actions/RowContextMenu';
import { normalizePlatform } from '@/utils/platform-data';

export interface LibraryArtist {
  key: string;
  display: string;
  album_count: number;
  track_count: number;
  cover_id?: string | null;
}

interface ArtistsViewProps {
  search: string;
  sort: string;
  source?: ServicePlatform;
  onSelectArtist: (key: string) => void;
}

function colsForWidth(w: number): number {
  if (w < 640) return 2;
  if (w < 768) return 3;
  if (w < 1024) return 4;
  if (w < 1280) return 5;
  return 6;
}

interface ArtistCardProps {
  a: LibraryArtist;
  url: string | null;
  source: ServicePlatform;
  onSelect: (key: string) => void;
}

const ArtistCard = memo(function ArtistCard({ a, url, source, onSelect }: ArtistCardProps) {
  const [armed, setArmed] = useState(false);
  const normalizedPlatform = useMemo(() => normalizePlatform(source), [source]);
  const arm = useCallback(() => setArmed(true), []);
  const artistObj = { id: a.key, display: a.display, cover_url: url ?? null };

  const card = (
    <div
      className="group relative"
      onPointerEnter={arm}
      onContextMenu={(e) => {
        if (!armed) {
          e.preventDefault();
          setArmed(true);
        }
      }}
    >
      {armed && (
        <div
          className="absolute top-2 right-2 z-10 opacity-0 group-hover:opacity-100 transition-opacity rounded-md bg-background/70 backdrop-blur-sm"
          onClick={(e) => e.stopPropagation()}
        >
          <ArtistKebab
            size="sm"
            platform={normalizedPlatform}
            context="library"
            artist={artistObj}
          />
        </div>
      )}
      <button
        onClick={() => onSelect(a.key)}
        className="flex flex-col items-center text-center gap-2 p-3 rounded-xl hover:bg-card/60 transition-colors w-full"
      >
        <div className="h-28 w-28 rounded-full overflow-hidden bg-muted shrink-0">
          {url ? (
            <img
              src={url}
              alt={a.display}
              loading="lazy"
              decoding="async"
              className="w-full h-full object-cover"
            />
          ) : (
            <div className="w-full h-full flex items-center justify-center text-muted-foreground/30">
              <User className="h-10 w-10" />
            </div>
          )}
        </div>
        <div className="min-w-0 w-full">
          <p className="text-sm font-semibold truncate">{a.display}</p>
          <p className="text-[11px] text-muted-foreground/50">
            {a.album_count} album{a.album_count !== 1 ? 's' : ''} · {a.track_count} track
            {a.track_count !== 1 ? 's' : ''}
          </p>
        </div>
      </button>
    </div>
  );

  if (armed) {
    return (
      <ArtistContextMenu platform={normalizedPlatform} artist={artistObj} context="library">
        {card}
      </ArtistContextMenu>
    );
  }
  return card;
});

export function ArtistsView({ search, sort, source = 'local', onSelectArtist }: ArtistsViewProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const cols = colsForWidth(useElementWidth(parentRef));

  const sortKey = sort === 'name' || sort === 'title' ? 'name' : sort;
  const { query, items: visible } = useLibraryInfiniteQuery<LibraryArtist>('artists', {
    sort: sortKey,
    search,
    source,
  });

  const rowCount = Math.ceil(visible.length / cols);
  const rowVirtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 196,
    overscan: 3,
  });
  const virtualRows = rowVirtualizer.getVirtualItems();

  useInfiniteRows(query, virtualRows, rowCount);

  const covers = useVisibleGridCovers(virtualRows, visible, cols);

  const stateView = renderLibraryQueryState({
    query,
    entity: 'artists',
    source,
    count: visible.length,
  });
  if (stateView) return stateView;

  return (
    <div ref={parentRef} className="h-full overflow-y-auto">
      <div style={{ height: rowVirtualizer.getTotalSize(), position: 'relative' }}>
        {virtualRows.map((vr) => {
          const start = vr.index * cols;
          const rowArtists = visible.slice(start, start + cols);
          return (
            <div
              key={vr.key}
              data-index={vr.index}
              ref={rowVirtualizer.measureElement}
              className="grid gap-3"
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                transform: `translateY(${vr.start}px)`,
                gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`,
              }}
            >
              {rowArtists.map((a) => (
                <ArtistCard
                  key={a.key}
                  a={a}
                  url={a.cover_id ? (covers[a.cover_id] ?? null) : null}
                  source={source}
                  onSelect={onSelectArtist}
                />
              ))}
            </div>
          );
        })}
      </div>
      {query.isFetchingNextPage && (
        <div className="py-6 text-center text-xs text-muted-foreground/40">Loading more…</div>
      )}
    </div>
  );
}

import { useMemo, type ReactNode, type RefObject } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Loader2 } from 'lucide-react';
import { useInfiniteRows, useVisibleGridCovers } from '@/features/library/hooks/useVirtualRows';
import { gridStyle } from '@/features/radio/grid';
import { StationSkeletons, StationTile } from '@/features/radio/components/StationCollection';
import { RadioError } from '@/features/radio/components/RadioStates';
import { useRadioStore } from '@/features/radio/stores/useRadioStore';
import type { StationChrome } from '@/features/radio/RadioActions';
import type { RadioStation } from '@/features/radio/api';

const ROW_HEIGHT = 64;

interface Props {
  stations: RadioStation[];
  chrome: StationChrome;
  /** Owned by the parent, which needs the same element to size the columns. */
  scrollRef: RefObject<HTMLDivElement | null>;
  cols: number;
  /** The container's measured width, for the virtualizer's row height. */
  width: number;
  isPending: boolean;
  isFetchingNextPage?: boolean;
  hasNextPage?: boolean;
  fetchNextPage?: () => unknown;
  error?: unknown;
  empty: ReactNode;
}

/**
 * The results surface: virtualized, paged, and drawn as either cards or rows.
 *
 * A populated grid is never blanked to show a spinner — with `keepPreviousData`
 * upstream, `isPending` is only true on the very first load of a key, so
 * changing a filter re-renders the old results until the new ones land.
 */
export function StationGrid({
  stations,
  chrome,
  scrollRef,
  cols,
  width,
  isPending,
  isFetchingNextPage = false,
  hasNextPage = false,
  fetchNextPage,
  error,
  empty,
}: Props) {
  const view = useRadioStore((s) => s.view);

  const showSkeletons = isPending && stations.length === 0;
  const contentRows = Math.ceil(stations.length / cols);
  const rowCount = showSkeletons ? 2 : contentRows + (isFetchingNextPage ? 1 : 0);
  const rowHeight = view === 'list' ? ROW_HEIGHT : Math.round(width / cols + 64);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: 3,
  });
  const rows = virtualizer.getVirtualItems();

  useInfiniteRows(
    { hasNextPage, isFetchingNextPage, fetchNextPage: () => fetchNextPage?.() },
    rows,
    contentRows
  );

  // Resolved for the visible window in one pass, rather than one effect and one
  // promise per mounted tile.
  const coverIds = useMemo(() => stations.map((s) => ({ cover_id: s.coverId })), [stations]);
  const covers = useVisibleGridCovers(rows, coverIds, cols);

  const state =
    error && stations.length === 0 ? (
      <RadioError error={error} />
    ) : !showSkeletons && stations.length === 0 ? (
      empty
    ) : null;

  // The scroll container is rendered in every state on purpose: it carries the
  // ref the width observer attaches to, and that observer only ever attaches
  // once. Swapping it out for a bare empty state left the column count frozen
  // at its initial guess for the rest of the page's life.
  return (
    <div ref={scrollRef} className="h-full overflow-y-auto">
      {state ?? (
        <div
          style={{ height: virtualizer.getTotalSize(), position: 'relative' }}
          className="px-0.5 pb-6"
        >
          {rows.map((row) => (
            <div
              key={row.key}
              className="absolute left-0 top-0 w-full"
              style={{ height: rowHeight, transform: `translateY(${row.start}px)` }}
            >
              {!showSkeletons && row.index >= contentRows ? (
                <div className="flex items-center justify-center gap-2 py-6 text-sm text-muted-foreground/60">
                  <Loader2 className="h-4 w-4 animate-spin" /> Loading more…
                </div>
              ) : (
                <div
                  className={view === 'list' ? 'pb-1.5' : 'grid gap-3 pb-3'}
                  style={view === 'list' ? undefined : gridStyle(cols)}
                >
                  {showSkeletons ? (
                    <StationSkeletons view={view} count={cols} />
                  ) : (
                    Array.from({ length: cols }, (_, c) => {
                      const station = stations[row.index * cols + c];
                      // Filler keys are namespaced by row so React never has to
                      // reconcile a gap against a real station's key.
                      if (!station) return <div key={`gap-${row.index}-${c}`} />;
                      return (
                        <StationTile
                          key={station.key}
                          station={station}
                          chrome={chrome}
                          covers={covers}
                          view={view}
                        />
                      );
                    })
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

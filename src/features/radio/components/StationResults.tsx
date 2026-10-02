import { useRef, type ReactNode } from 'react';
import { StationGrid } from '@/features/radio/components/StationGrid';
import { ViewControls } from '@/features/radio/components/ViewControls';
import { useGridMetrics } from '@/features/radio/grid';
import type { StationChrome } from '@/features/radio/RadioActions';
import type { RadioStation } from '@/features/radio/api';

/** The parts of an infinite station query this surface actually reads. */
export interface StationQueryState {
  isPending: boolean;
  isFetchingNextPage: boolean;
  hasNextPage: boolean;
  fetchNextPage: () => unknown;
  error: unknown;
}

interface Props {
  stations: RadioStation[];
  chrome: StationChrome;
  query: StationQueryState;
  /** Singular noun for the count line — "station", "result". */
  noun: string;
  /** Appended after the count, e.g. `for “jazz”`. */
  suffix?: string;
  /** Toolbar, far left — before the count. */
  leading?: ReactNode;
  /** Toolbar, right — before the view controls. */
  trailing?: ReactNode;
  /** Between the toolbar and the grid. */
  banner?: ReactNode;
  empty: ReactNode;
}

/**
 * Count line, view controls, results.
 *
 * Browse, search and playlists all say the same thing in the same place; only
 * the noun and the extra toolbar chrome differ. Owning the scroll container here
 * is also what lets the column count be read once — it used to travel child →
 * effect → parent state → child on every resize.
 */
export function StationResults({
  stations,
  chrome,
  query,
  noun,
  suffix,
  leading,
  trailing,
  banner,
  empty,
}: Props) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const { width, cols } = useGridMetrics(scrollRef);

  const count = `${stations.length.toLocaleString()}${query.hasNextPage ? '+' : ''}`;
  const plural = stations.length === 1 ? '' : 's';

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2 pb-3">
        {leading}
        <p className="text-xs text-muted-foreground/60">
          {query.isPending && stations.length === 0
            ? 'Searching…'
            : `${count} ${noun}${plural}${suffix ? ` ${suffix}` : ''}`}
        </p>
        <div className="ml-auto flex items-center gap-3">
          {trailing}
          <ViewControls resolvedColumns={cols} />
        </div>
      </div>

      {banner}

      <div className="min-h-0 flex-1">
        <StationGrid
          stations={stations}
          chrome={chrome}
          scrollRef={scrollRef}
          cols={cols}
          width={width}
          isPending={query.isPending}
          isFetchingNextPage={query.isFetchingNextPage}
          hasNextPage={query.hasNextPage}
          fetchNextPage={query.fetchNextPage}
          error={query.error}
          empty={empty}
        />
      </div>
    </div>
  );
}

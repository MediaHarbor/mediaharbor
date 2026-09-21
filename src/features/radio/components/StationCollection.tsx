import type { ReactNode } from 'react';
import { gridStyle } from '@/features/radio/grid';
import { StationCard } from '@/features/radio/components/StationCard';
import { StationRow } from '@/features/radio/components/StationRow';
import {
  coverOf,
  useStationCovers,
  type StationCovers,
} from '@/features/radio/hooks/useStationCover';
import type { StationChrome } from '@/features/radio/RadioActions';
import type { RadioStation } from '@/features/radio/api';
import type { RadioView } from '@/features/radio/stores/useRadioStore';

interface TileProps {
  station: RadioStation;
  chrome: StationChrome;
  covers: StationCovers;
  view: RadioView;
  handle?: ReactNode;
  /** List mode only — the card has nowhere to put it. */
  trailing?: ReactNode;
  onRemoveFromList?: (station: RadioStation) => void;
}

/**
 * One station, drawn in whichever mode the page is in.
 *
 * The card/row switch was written out in both the virtualized grid and the
 * favourites view, which meant the two had to be kept in agreement about props
 * by hand.
 */
export function StationTile({
  station,
  chrome,
  covers,
  view,
  handle,
  trailing,
  onRemoveFromList,
}: TileProps) {
  const shared = {
    station,
    favorite: chrome.favoriteKeys.has(station.key),
    playing: chrome.nowPlayingKey === station.key,
    cover: coverOf(station, covers),
    handle,
    onRemoveFromList,
  };
  return view === 'list' ? (
    <StationRow {...shared} trailing={trailing} />
  ) : (
    <StationCard {...shared} />
  );
}

/**
 * A short bag of stations, laid out but not virtualized.
 *
 * For collections that are short by construction — favourites and history come
 * out of `library.sqlite` a few dozen rows at a time. Anything unbounded goes
 * through `StationGrid` instead.
 */
export function StationCollection({
  stations,
  chrome,
  cols,
  view,
}: {
  stations: RadioStation[];
  chrome: StationChrome;
  cols: number;
  view: RadioView;
}) {
  const covers = useStationCovers(stations);
  return (
    <div
      className={view === 'list' ? 'space-y-1.5' : 'grid gap-3'}
      style={view === 'list' ? undefined : gridStyle(cols)}
    >
      {stations.map((station) => (
        <StationTile
          key={station.key}
          station={station}
          chrome={chrome}
          covers={covers}
          view={view}
        />
      ))}
    </div>
  );
}

/** Same geometry as a real card, so nothing reflows when the data lands. */
export function StationSkeleton() {
  return (
    <div className="overflow-hidden rounded-lg border border-border/40 bg-muted/20">
      <div className="aspect-square w-full animate-pulse bg-muted/40" />
      <div className="space-y-2 p-3">
        <div className="h-3.5 w-4/5 animate-pulse rounded bg-muted/40" />
        <div className="h-2.5 w-2/5 animate-pulse rounded bg-muted/30" />
      </div>
    </div>
  );
}

export function RowSkeleton() {
  return (
    <div className="flex items-center gap-3 rounded-lg border border-border/40 bg-muted/20 p-2">
      <div className="h-10 w-10 shrink-0 animate-pulse rounded bg-muted/40" />
      <div className="flex-1 space-y-2">
        <div className="h-3.5 w-1/3 animate-pulse rounded bg-muted/40" />
        <div className="h-2.5 w-1/5 animate-pulse rounded bg-muted/30" />
      </div>
    </div>
  );
}

export function StationSkeletons({ view, count }: { view: RadioView; count: number }) {
  return (
    <>
      {Array.from({ length: count }, (_, i) =>
        view === 'list' ? <RowSkeleton key={i} /> : <StationSkeleton key={i} />
      )}
    </>
  );
}

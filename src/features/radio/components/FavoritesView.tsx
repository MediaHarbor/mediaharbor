import { useRef } from 'react';
import { Heart, History } from 'lucide-react';
import { StationCollection, StationSkeletons } from '@/features/radio/components/StationCollection';
import { ViewControls } from '@/features/radio/components/ViewControls';
import { useRadioStore } from '@/features/radio/stores/useRadioStore';
import { useGridMetrics } from '@/features/radio/grid';
import { RadioEmpty, RadioError } from '@/features/radio/components/RadioStates';
import { useRadioFavorites, useRadioRecent } from '@/features/radio/hooks/useRadioQueries';
import type { StationChrome } from '@/features/radio/RadioActions';

/**
 * Saved stations and what was on recently.
 *
 * Both come out of `library.sqlite`, so they are short by construction — a few
 * dozen rows — and are laid out as plain grids rather than virtualized ones.
 */
export function FavoritesView(chrome: StationChrome) {
  const favorites = useRadioFavorites();
  const recent = useRadioRecent();
  const parentRef = useRef<HTMLDivElement>(null);
  const view = useRadioStore((s) => s.view);
  const { cols } = useGridMetrics(parentRef);

  const skeletons = (
    <div className={view === 'list' ? 'space-y-1.5' : 'grid gap-3'}>
      <StationSkeletons view={view} count={view === 'list' ? 4 : cols} />
    </div>
  );

  return (
    <div ref={parentRef} className="h-full space-y-8 overflow-y-auto pb-8">
      <section>
        <div className="mb-3 flex items-center gap-2">
          <h3 className="flex items-center gap-2 text-sm font-medium text-muted-foreground">
            <Heart className="h-3.5 w-3.5" /> Favourites
          </h3>
          <div className="ml-auto">
            <ViewControls resolvedColumns={cols} />
          </div>
        </div>
        {favorites.isError ? (
          <RadioError error={favorites.error} what="your favourites" />
        ) : favorites.isPending ? (
          skeletons
        ) : favorites.data.length === 0 ? (
          <RadioEmpty
            icon={Heart}
            title="No saved stations"
            hint="Hover a station anywhere on this page and press the heart. Favourites keep working even when the directory is down."
          />
        ) : (
          <StationCollection stations={favorites.data} chrome={chrome} cols={cols} view={view} />
        )}
      </section>

      {(recent.isPending || (recent.data?.length ?? 0) > 0) && (
        <section>
          <h3 className="mb-3 flex items-center gap-2 text-sm font-medium text-muted-foreground">
            <History className="h-3.5 w-3.5" /> Recently played
          </h3>
          {recent.isPending ? (
            skeletons
          ) : (
            <StationCollection
              stations={recent.data ?? []}
              chrome={chrome}
              cols={cols}
              view={view}
            />
          )}
        </section>
      )}
    </div>
  );
}

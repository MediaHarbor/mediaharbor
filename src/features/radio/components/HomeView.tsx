import { useMemo } from 'react';
import { Radio } from 'lucide-react';
import { ShelfSection } from '@/features/library/components/shelves/ShelfRow';
import { StationTile, StationSkeleton } from '@/features/radio/components/StationCollection';
import { RadioEmpty, RadioError } from '@/features/radio/components/RadioStates';
import { pickSeeded, todaySeed } from '@/features/radio/dailySeed';
import { useStationCovers } from '@/features/radio/hooks/useStationCover';
import type { RadioFacet, RadioStation } from '@/features/radio/api';
import {
  useRadioFacets,
  useRadioFavorites,
  useRadioRecent,
  useStationShelf,
} from '@/features/radio/hooks/useRadioQueries';
import type { StationChrome } from '@/features/radio/RadioActions';
import type { RadioFilters } from '@/features/radio/stores/useRadioStore';
import { NO_FILTERS } from '@/features/radio/stores/useRadioStore';
import type { RadioSearchRequest } from '@/tauri-bridge';

const GENRE_SHELVES = 3;

/** Closes over nothing, so there is no reason to rebuild it per render. */
const SKELETONS = Array.from({ length: 7 }, (_, i) => (
  <div key={i} className="w-[168px] shrink-0">
    <StationSkeleton />
  </div>
));

interface Props extends StationChrome {
  sourceCount: number;
  /** Opens Browse already narrowed to this shelf. */
  onShowAll: (filters: Partial<RadioFilters>) => void;
}

/**
 * The landing page: shelves of actual stations rather than a menu.
 *
 * Each shelf is its own query so they fill in as they arrive — one slow
 * directory must not hold the whole page. Which genres and country appear is
 * decided by a date-derived seed, so the page changes daily without knowing
 * anything about who is looking at it.
 */
export function HomeView({ sourceCount, onShowAll, ...chrome }: Props) {
  const seed = todaySeed();
  const recent = useRadioRecent();
  const favorites = useRadioFavorites();
  const mine = useStationShelf(MINE_REQUEST);
  const genres = useRadioFacets('tags');
  const countries = useRadioFacets('countries');

  const dailyGenres = useMemo(
    () => pickSeeded(genres.data ?? [], GENRE_SHELVES, seed),
    [genres.data, seed]
  );
  const dailyCountry = useMemo(
    () => pickSeeded(countries.data ?? [], 1, seed ^ 0x5bf03635)[0],
    [countries.data, seed]
  );

  if (sourceCount === 0 && (mine.data?.length ?? 0) === 0) {
    return (
      <div className="h-full overflow-y-auto">
        <RadioEmpty
          icon={Radio}
          title="No station sources are switched on"
          hint="Turn one on above, or add a station of your own with the button beside them."
        />
      </div>
    );
  }

  return (
    <div className="h-full space-y-8 overflow-y-auto pb-10">
      {(recent.data?.length ?? 0) > 0 && (
        <ShelfSection title="Continue listening" rowClassName="gap-3">
          <ShelfCards stations={recent.data ?? []} chrome={chrome} />
        </ShelfSection>
      )}

      {(mine.data?.length ?? 0) > 0 && (
        <ShelfSection
          title="Your stations"
          subtitle="Everything you added yourself"
          rowClassName="gap-3"
          onShowAll={() => onShowAll({ source: 'custom' })}
        >
          <ShelfCards stations={mine.data ?? []} chrome={chrome} />
        </ShelfSection>
      )}

      {(favorites.data?.length ?? 0) > 0 && (
        <ShelfSection title="Favourites" rowClassName="gap-3">
          <ShelfCards stations={favorites.data ?? []} chrome={chrome} />
        </ShelfSection>
      )}

      <StationShelf
        title="Trending now"
        subtitle="Gaining listeners this week"
        request={TRENDING_REQUEST}
        filters={TRENDING_FILTERS}
        onShowAll={onShowAll}
        chrome={chrome}
      />

      {genres.isPending && (
        <ShelfSection title="Loading genres…" rowClassName="gap-3">
          {SKELETONS}
        </ShelfSection>
      )}
      {dailyGenres.map((genre: RadioFacet) => (
        <StationShelf
          key={genre.name}
          title={capitalise(genre.label)}
          subtitle={`${genre.stationCount.toLocaleString()} stations`}
          request={{ tag: genre.name, order: 'clickcount' }}
          filters={{ tag: genre.name }}
          onShowAll={onShowAll}
          chrome={chrome}
        />
      ))}

      {dailyCountry && (
        <StationShelf
          title={`Around ${dailyCountry.label}`}
          request={{ countryCode: dailyCountry.name, order: 'clickcount' }}
          filters={{ countryCode: dailyCountry.name }}
          onShowAll={onShowAll}
          chrome={chrome}
        />
      )}

      <StationShelf
        title="Something else entirely"
        subtitle="A different handful every day"
        request={RANDOM_REQUEST}
        filters={RANDOM_FILTERS}
        onShowAll={onShowAll}
        chrome={chrome}
      />
    </div>
  );
}

// Hoisted so the shelf hooks that memoize on them see a stable request.
const MINE_REQUEST: RadioSearchRequest = { sources: ['custom'], order: 'name' };
const TRENDING_REQUEST: RadioSearchRequest = { order: 'clicktrend' };
const TRENDING_FILTERS: Partial<RadioFilters> = { order: 'clicktrend' };
const RANDOM_REQUEST: RadioSearchRequest = { order: 'random' };
const RANDOM_FILTERS: Partial<RadioFilters> = { order: 'random' };

function capitalise(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** The station tiles of one shelf, with their covers resolved together. */
function ShelfCards({ stations, chrome }: { stations: RadioStation[]; chrome: StationChrome }) {
  const covers = useStationCovers(stations);
  return (
    <>
      {stations.map((station) => (
        <div key={station.key} className="w-[168px] shrink-0">
          <StationTile station={station} chrome={chrome} covers={covers} view="grid" />
        </div>
      ))}
    </>
  );
}

function StationShelf({
  title,
  subtitle,
  request,
  filters,
  onShowAll,
  chrome,
}: {
  title: string;
  subtitle?: string;
  request: RadioSearchRequest;
  filters: Partial<RadioFilters>;
  onShowAll: (filters: Partial<RadioFilters>) => void;
  chrome: StationChrome;
}) {
  const query = useStationShelf(request);

  // A shelf that came back empty is not worth a heading; a shelf that failed is,
  // because the reason is the only thing that explains a half-empty page.
  if (query.isError) {
    return (
      <ShelfSection title={title} subtitle={subtitle} staticBody>
        <RadioError error={query.error} what={title.toLowerCase()} />
      </ShelfSection>
    );
  }
  if (!query.isPending && (query.data?.length ?? 0) === 0) return null;

  return (
    <ShelfSection
      title={title}
      subtitle={subtitle}
      rowClassName="gap-3"
      onShowAll={() => onShowAll({ ...NO_FILTERS, ...filters })}
    >
      {query.isPending ? SKELETONS : <ShelfCards stations={query.data ?? []} chrome={chrome} />}
    </ShelfSection>
  );
}

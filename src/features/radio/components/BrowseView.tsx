import { useMemo, useState } from 'react';
import { Radio, SlidersHorizontal, X } from 'lucide-react';
import { cn } from '@/utils/cn';
import type { RadioDirectorySource } from '@/features/radio/api';
import { useStationSearch } from '@/features/radio/hooks/useRadioQueries';
import { FilterRail } from '@/features/radio/components/FilterRail';
import { StationResults } from '@/features/radio/components/StationResults';
import { RadioEmpty } from '@/features/radio/components/RadioStates';
import type { StationChrome } from '@/features/radio/RadioActions';
import {
  activeFilterCount,
  useRadioStore,
  type RadioFilters,
} from '@/features/radio/stores/useRadioStore';
import type { RadioSearchRequest } from '@/tauri-bridge';

interface Props extends StationChrome {
  sources: RadioDirectorySource[];
}

/** Filters as the directories want them. */
function toRequest(filters: RadioFilters): RadioSearchRequest {
  return {
    sources: filters.source ? [filters.source] : undefined,
    tag: filters.tag ?? undefined,
    countryCode: filters.countryCode ?? undefined,
    language: filters.language ?? undefined,
    codec: filters.codec ?? undefined,
    bitrateMin: filters.bitrateMin || undefined,
    order: filters.order,
  };
}

/**
 * Browse: filters on the left, results on the right, nothing in between.
 */
/** The orderings radio-browser accepts, as the menu offers them. */
const RADIO_ORDERS = [
  { value: 'clickcount', label: 'Popular' },
  { value: 'clicktrend', label: 'Trending' },
  { value: 'votes', label: 'Top rated' },
  { value: 'name', label: 'Name' },
  { value: 'random', label: 'Random' },
] as const;

export function BrowseView({ sources, ...chrome }: Props) {
  const filters = useRadioStore((s) => s.filters);
  const setFilter = useRadioStore((s) => s.setFilter);
  const clearFilters = useRadioStore((s) => s.clearFilters);
  const [railOpen, setRailOpen] = useState(false);

  const request = useMemo(() => toRequest(filters), [filters]);
  const { query, stations } = useStationSearch('browse', request);
  const active = activeFilterCount(filters);

  const chips = useMemo(() => {
    const labelFor = (id: string) => sources.find((s) => s.id === id)?.label ?? id;
    return (
      [
        filters.source && { field: 'source' as const, label: labelFor(filters.source) },
        filters.tag && { field: 'tag' as const, label: filters.tag },
        filters.countryCode && { field: 'countryCode' as const, label: filters.countryCode },
        filters.language && { field: 'language' as const, label: filters.language },
        filters.codec && { field: 'codec' as const, label: filters.codec },
        filters.bitrateMin > 0 && {
          field: 'bitrateMin' as const,
          label: `${filters.bitrateMin}k+`,
        },
      ] as const
    ).filter(Boolean) as { field: keyof RadioFilters; label: string }[];
  }, [filters, sources]);

  const filterButton = (
    <button
      type="button"
      onClick={() => setRailOpen((o) => !o)}
      aria-expanded={railOpen}
      className={cn(
        'inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-xs transition-colors lg:hidden',
        active > 0
          ? 'border-[var(--service-accent)] text-foreground'
          : 'border-border/50 text-muted-foreground'
      )}
    >
      <SlidersHorizontal className="h-3.5 w-3.5" />
      Filters{active > 0 ? ` (${active})` : ''}
    </button>
  );

  const orderChips = (
    <div className="flex items-center gap-1" role="group" aria-label="Sort stations">
      {RADIO_ORDERS.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={filters.order === option.value}
          onClick={() => setFilter('order', option.value)}
          className={cn(
            'rounded-full px-2 py-0.5 text-[11px] transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
            filters.order === option.value
              ? 'bg-[var(--service-accent)]/15 text-foreground'
              : 'text-muted-foreground/60 hover:text-foreground'
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );

  const banner = (
    <>
      {railOpen && (
        <div className="mb-3 rounded-lg border border-border/40 p-3 lg:hidden">
          <FilterRail sources={sources} />
        </div>
      )}

      {chips.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5 pb-3">
          {chips.map((chip) => (
            <button
              key={chip.field}
              type="button"
              onClick={() =>
                setFilter(chip.field, (chip.field === 'bitrateMin' ? 0 : null) as never)
              }
              className="inline-flex items-center gap-1 rounded-full border border-[var(--service-accent)] bg-[var(--service-accent)]/10 px-2.5 py-1 text-[11px] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              {chip.label}
              <X className="h-3 w-3" />
            </button>
          ))}
          <button
            type="button"
            onClick={clearFilters}
            className="px-1 text-[11px] text-muted-foreground hover:text-foreground"
          >
            Clear all
          </button>
        </div>
      )}
    </>
  );

  return (
    <div className="flex h-full min-h-0 gap-5">
      <aside className="hidden w-52 shrink-0 overflow-y-auto pr-1 lg:block">
        <FilterRail sources={sources} />
      </aside>

      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <StationResults
          stations={stations}
          chrome={chrome}
          query={query}
          noun="station"
          leading={filterButton}
          trailing={orderChips}
          banner={banner}
          empty={
            <RadioEmpty
              icon={Radio}
              title={active > 0 ? 'Nothing matches those filters' : 'No stations here'}
              hint={
                active > 0
                  ? 'Loosen one of them — a genre and a country together often leave nothing.'
                  : 'The directories have nothing to show right now.'
              }
              action={
                active > 0 ? (
                  <button
                    type="button"
                    onClick={clearFilters}
                    className="text-xs text-[var(--service-accent)] hover:underline"
                  >
                    Clear the filters
                  </button>
                ) : undefined
              }
            />
          }
        />
      </div>
    </div>
  );
}

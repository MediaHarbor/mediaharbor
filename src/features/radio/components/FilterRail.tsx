import { useMemo, useState } from 'react';
import { ChevronDown, Loader2, X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { cn } from '@/utils/cn';
import { errorMessage } from '@/utils/errors';
import { type RadioDirectorySource, type RadioFacetKind } from '@/features/radio/api';
import { facetLabel } from '@/features/radio/format';
import { useRadioFacets } from '@/features/radio/hooks/useRadioQueries';
import {
  activeFilterCount,
  useRadioStore,
  type RadioFilters,
} from '@/features/radio/stores/useRadioStore';

/** Which filter each facet fills in. */
const FACET_FIELD: Record<RadioFacetKind, keyof RadioFilters> = {
  tags: 'tag',
  countries: 'countryCode',
  languages: 'language',
  codecs: 'codec',
};

const BITRATES = [0, 64, 128, 192, 256, 320];
const TOP_N = 12;

/**
 * The filters, always visible.
 *
 * The old page hid these behind a tile and a chip cloud, which meant three
 * clicks before a station appeared. Here they sit beside the results and every
 * change restreams them.
 */
export function FilterRail({ sources }: { sources: RadioDirectorySource[] }) {
  const filters = useRadioStore((s) => s.filters);
  const setFilter = useRadioStore((s) => s.setFilter);
  const clearFilters = useRadioStore((s) => s.clearFilters);
  const active = activeFilterCount(filters);

  /** Facets a directory does not carry are hidden rather than shown empty. */
  const available = useMemo(() => {
    const chosen = filters.source
      ? sources.filter((s) => s.id === filters.source)
      : sources.filter((s) => s.enabled);
    const kinds = new Set<RadioFacetKind>();
    for (const source of chosen) source.capabilities.facets.forEach((f) => kinds.add(f));
    return (['tags', 'countries', 'languages', 'codecs'] as RadioFacetKind[]).filter((k) =>
      kinds.has(k)
    );
  }, [sources, filters.source]);

  return (
    <div className="space-y-4 text-sm">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-[11px] font-semibold uppercase tracking-widest text-muted-foreground/60">
          Filters
        </h2>
        {active > 0 && (
          <button
            type="button"
            onClick={clearFilters}
            className="inline-flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground"
          >
            <X className="h-3 w-3" /> Clear {active}
          </button>
        )}
      </div>

      <Section title="Source" defaultOpen>
        <div className="space-y-0.5">
          <Option
            label="All sources"
            selected={filters.source === null}
            onSelect={() => setFilter('source', null)}
          />
          {sources
            .filter((s) => s.enabled)
            .map((source) => (
              <Option
                key={source.id}
                label={source.label}
                selected={filters.source === source.id}
                onSelect={() =>
                  setFilter('source', filters.source === source.id ? null : source.id)
                }
              />
            ))}
        </div>
      </Section>

      {available.map((kind) => (
        <FacetSection
          key={kind}
          kind={kind}
          source={filters.source}
          selected={filters[FACET_FIELD[kind]] as string | null}
          onSelect={(value) => setFilter(FACET_FIELD[kind], value as never)}
        />
      ))}

      <Section title="Minimum bitrate" defaultOpen>
        <div className="flex flex-wrap gap-1">
          {BITRATES.map((rate) => (
            <button
              key={rate}
              type="button"
              aria-pressed={filters.bitrateMin === rate}
              onClick={() => setFilter('bitrateMin', rate)}
              className={cn(
                'rounded-full border px-2 py-0.5 text-[11px] transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
                filters.bitrateMin === rate
                  ? 'border-[var(--service-accent)] bg-[var(--service-accent)]/10 text-foreground'
                  : 'border-border/50 text-muted-foreground/70 hover:border-border hover:text-foreground'
              )}
            >
              {rate === 0 ? 'Any' : `${rate}k`}
            </button>
          ))}
        </div>
      </Section>
    </div>
  );
}

function FacetSection({
  kind,
  source,
  selected,
  onSelect,
}: {
  kind: RadioFacetKind;
  source: string | null;
  selected: string | null;
  onSelect: (value: string | null) => void;
}) {
  const [filter, setFilter] = useState('');
  const [expanded, setExpanded] = useState(false);
  const facets = useRadioFacets(kind, source);
  const needle = filter.trim().toLowerCase();

  const matching = useMemo(() => {
    const rows = facets.data ?? [];
    if (!needle) return rows;
    return rows.filter((f) => f.label.toLowerCase().includes(needle));
  }, [facets.data, needle]);

  // The selected value stays visible even when it falls outside the top slice,
  // or clearing it would mean scrolling for something you cannot see.
  const visible = useMemo(() => {
    if (expanded || needle) return matching.slice(0, 400);
    const head = matching.slice(0, TOP_N);
    const chosen = matching.find((f) => f.name === selected);
    return chosen && !head.some((f) => f.name === selected) ? [...head, chosen] : head;
  }, [matching, expanded, needle, selected]);

  return (
    <Section title={facetLabel(kind)} defaultOpen={kind === 'tags'}>
      {facets.isPending ? (
        <div className="flex items-center gap-2 py-1 text-[11px] text-muted-foreground/50">
          <Loader2 className="h-3 w-3 animate-spin" /> Loading…
        </div>
      ) : facets.isError ? (
        <p className="py-1 text-[11px] text-destructive/80" title={errorMessage(facets.error)}>
          Unavailable
        </p>
      ) : (
        <div className="space-y-1.5">
          {(facets.data?.length ?? 0) > TOP_N && (
            <Input
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder={`Filter ${facetLabel(kind).toLowerCase()}…`}
              aria-label={`Filter ${facetLabel(kind).toLowerCase()}`}
              className="h-7 border-0 bg-muted/30 text-xs"
            />
          )}
          <div className="space-y-0.5">
            {visible.map((facet) => (
              <Option
                key={facet.name}
                label={facet.label}
                count={facet.stationCount}
                selected={selected === facet.name}
                onSelect={() => onSelect(selected === facet.name ? null : facet.name)}
              />
            ))}
          </div>
          {!needle && matching.length > TOP_N && (
            <button
              type="button"
              onClick={() => setExpanded((e) => !e)}
              className="text-[11px] text-muted-foreground hover:text-foreground"
            >
              {expanded ? 'Show fewer' : `Show all ${matching.length.toLocaleString()}`}
            </button>
          )}
        </div>
      )}
    </Section>
  );
}

function Section({
  title,
  defaultOpen,
  children,
}: {
  title: string;
  defaultOpen?: boolean;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(!!defaultOpen);
  return (
    <div className="border-t border-border/30 pt-3 first:border-t-0 first:pt-0">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="mb-1.5 flex w-full items-center justify-between gap-2 text-left text-xs font-medium text-foreground/80 hover:text-foreground"
      >
        {title}
        <ChevronDown className={cn('h-3.5 w-3.5 transition-transform', open ? '' : '-rotate-90')} />
      </button>
      {open && children}
    </div>
  );
}

function Option({
  label,
  count,
  selected,
  onSelect,
}: {
  label: string;
  count?: number;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={onSelect}
      className={cn(
        'flex w-full items-baseline gap-2 rounded px-1.5 py-1 text-left text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
        selected
          ? 'bg-[var(--service-accent)]/12 font-medium text-foreground'
          : 'text-muted-foreground/80 hover:bg-muted/50 hover:text-foreground'
      )}
    >
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {count !== undefined && (
        <span className="shrink-0 text-[10px] tabular-nums text-muted-foreground/40">
          {count.toLocaleString()}
        </span>
      )}
    </button>
  );
}

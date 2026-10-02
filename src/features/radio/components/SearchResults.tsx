import { useMemo } from 'react';
import { Radio } from 'lucide-react';
import { StationResults } from '@/features/radio/components/StationResults';
import { RadioEmpty } from '@/features/radio/components/RadioStates';
import { useStationSearch } from '@/features/radio/hooks/useRadioQueries';
import { useDebounced } from '@/features/radio/hooks/useDebounced';
import type { StationChrome } from '@/features/radio/RadioActions';

const DEBOUNCE_MS = 200;

interface Props extends StationChrome {
  /** Raw, undebounced text straight from the header box. */
  text: string;
  onClear: () => void;
}

/**
 * What the header search shows, over whichever tab you were on.
 */
export function SearchResults({ text, onClear, ...chrome }: Props) {
  const debounced = useDebounced(text, DEBOUNCE_MS);

  const request = useMemo(
    () => ({ name: debounced.trim() || undefined, order: 'clickcount' }),
    [debounced]
  );
  const { query, stations } = useStationSearch('search', request, !!request.name);

  /**
   * While the ~0.5 s request is out, what is already on screen is filtered
   * locally by what has been typed. It cannot find a station that was never
   * fetched, but it makes the first keystrokes land instantly instead of
   * holding a stale grid.
   */
  const pending = query.isFetching && debounced !== text ? text.trim().toLowerCase() : '';
  const shown = useMemo(() => {
    if (!pending) return stations;
    const local = stations.filter((s) => s.name.toLowerCase().includes(pending));
    return local.length > 0 ? local : stations;
  }, [stations, pending]);

  return (
    <StationResults
      stations={shown}
      chrome={chrome}
      query={query}
      noun="result"
      suffix={`for “${text.trim()}”`}
      empty={
        <RadioEmpty
          icon={Radio}
          title="Nothing matched"
          hint="Try fewer words, or pick a genre from the suggestions as you type."
          action={
            <button
              type="button"
              onClick={onClear}
              className="text-xs text-[var(--service-accent)] hover:underline"
            >
              Clear the search
            </button>
          }
        />
      }
    />
  );
}

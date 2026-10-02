import { useEffect, useMemo, useRef } from 'react';
import { useInfiniteQuery, useQuery, useQueryClient, type QueryKey } from '@tanstack/react-query';
import { PAGE_SIZE, RADIO_FRESH, radioKeys, searchStations } from '@/features/radio/api';
import type { RadioDirectorySource, RadioFacetKind, RadioStation } from '@/features/radio/api';
import { tauriAPI, type RadioSearchRequest } from '@/tauri-bridge';

/** How many stations a home shelf holds. */
export const SHELF_SIZE = 14;

/** How many values a facet list is asked for. */
const FACET_LIMIT = 200;

/** Stable identity, so a still-loading roster does not hand consumers a new array each render. */
const NO_SOURCES: RadioDirectorySource[] = [];
const NO_STATIONS: RadioStation[] = [];

/**
 * The directory roster.
 *
 * One source mirrors a 9.5 MB document in the background, so it can be enabled
 * and not yet able to answer — which is what `warming` reports.
 */
export function useRadioSources() {
  const query = useQuery({
    queryKey: radioKeys.sources(),
    queryFn: () => tauriAPI.radio.sources(),
    refetchInterval: (q) =>
      (q.state.data ?? []).some((s) => s.status === 'loading') ? 3000 : false,
  });

  const sources = query.data ?? NO_SOURCES;
  return { ...query, sources, warming: sources.some((s) => s.status === 'loading') };
}

/**
 * Refetches everything once the slow directory finishes warming up — otherwise a
 * page opened during the warm-up sits empty until the user navigates.
 *
 * Deliberately separate from `useRadioSources`, which is a plain read: the
 * roster hook is mounted by both the radio page and the settings page, and
 * routes stay mounted, so keeping the effect inside it invalidated every radio
 * query twice.
 */
export function useRadioWarmupRefresh() {
  const qc = useQueryClient();
  const { warming } = useRadioSources();
  const wasWarming = useRef(false);

  useEffect(() => {
    if (wasWarming.current && !warming) {
      void qc.invalidateQueries({ queryKey: radioKeys.all });
    }
    wasWarming.current = warming;
  }, [warming, qc]);
}

/**
 * A page of stations, scrolled forever.
 *
 * `placeholderData: keepPreviousData` is what keeps a populated grid on screen
 * while the next filter loads — blanking to a spinner on every chip click reads
 * as slower than the network actually is.
 */
export function useStationSearch(scope: string, req: RadioSearchRequest, enabled = true) {
  const query = useInfiniteQuery({
    queryKey: radioKeys.stations(scope, req),
    enabled,
    initialPageParam: 0,
    queryFn: ({ pageParam }) => searchStations({ ...req, offset: pageParam as number }),
    // A short page means the directory ran out; anything else may have more.
    getNextPageParam: (last, all) => (last.length < PAGE_SIZE ? undefined : all.length * PAGE_SIZE),
    placeholderData: (prev) => prev,
  });
  const stations = useMemo(() => (query.data?.pages ?? []).flat(), [query.data]);
  return { query, stations };
}

/**
 * One home shelf.
 *
 * The request is completed here rather than at the call site, so the cache key
 * is a function of what is actually sent. Shelves used to key on the caller's
 * partial request while the fetch added `limit` — two shelves differing only in
 * an omitted field would have shared an entry.
 */
export function useStationShelf(request: RadioSearchRequest) {
  const req = useMemo(() => ({ ...request, limit: SHELF_SIZE }), [request]);
  return useQuery({
    queryKey: radioKeys.stations('shelf', req),
    queryFn: () => searchStations(req),
    staleTime: RADIO_FRESH.shelf,
  });
}

export function useRadioFacets(kind: RadioFacetKind, source: string | null = null) {
  return useQuery({
    queryKey: radioKeys.facets(kind, source),
    queryFn: () => tauriAPI.radio.facets(kind, FACET_LIMIT, source ?? undefined),
    staleTime: RADIO_FRESH.facets,
  });
}

/** The user's own rows: short, local, and edited from this page. */
function useStoredQuery<T>(key: QueryKey, fetch: () => Promise<T>, enabled = true) {
  return useQuery({ queryKey: key, queryFn: fetch, enabled, staleTime: RADIO_FRESH.list });
}

export function useRadioFavorites() {
  return useStoredQuery(radioKeys.favorites(), tauriAPI.radio.favorites);
}

export function useRadioRecent() {
  return useStoredQuery(radioKeys.recent(), tauriAPI.radio.recent);
}

export function useRadioLists() {
  return useStoredQuery(radioKeys.lists(), tauriAPI.radio.lists);
}

export function useRadioList(id: number | null) {
  return useStoredQuery(
    radioKeys.list(id ?? -1),
    () => tauriAPI.radio.listGet(id as number),
    id !== null
  );
}

export function useFavoriteKeys(): ReadonlySet<string> {
  const { data } = useRadioFavorites();
  return useMemo(() => new Set((data ?? NO_STATIONS).map((s) => s.key)), [data]);
}

/**
 * Refreshes "recently played" when the stream is actually ready.
 *
 * Invalidating on click was a station behind: the backend writes the play row
 * inside `play_media`, so a refetch fired alongside the click read the table as
 * it was before the row landed.
 */
export function useRecentOnStreamReady() {
  const qc = useQueryClient();
  useEffect(() => {
    return tauriAPI.player?.onStreamReady?.((data) => {
      if (data?.platform !== 'radio') return;
      void qc.invalidateQueries({ queryKey: radioKeys.recent() });
    });
  }, [qc]);
}

import { useMemo } from 'react';
import { useInfiniteQuery } from '@tanstack/react-query';
import { queryLibrary, libraryKeys, type ServicePlatform } from '@/features/library/api';

export const LIBRARY_PAGE_SIZE = 200;

export type LibraryKind = 'albums' | 'tracks' | 'videos' | 'artists' | 'playlists';

/**
 * The offset to ask for next, or `undefined` when the list is complete.
 *
 * Counts rows the service has been *asked* for, not rows that survived mapping. A
 * backend that drops entries — Spotify's library lists a `PseudoPlaylist` for Liked
 * Songs among its playlists, which is filtered out — left the mapped count short of
 * `total` forever, so the offset never advanced and the same page was fetched on a loop.
 */
export function nextOffset(
  last: { items: unknown[]; total?: number | null },
  pagesLoaded: number,
  pageSize: number = LIBRARY_PAGE_SIZE
): number | undefined {
  if (last.items.length === 0) return undefined;
  const requested = pagesLoaded * pageSize;
  const total = last.total;
  if (total === null || total === undefined) {
    return last.items.length === pageSize ? requested : undefined;
  }
  return requested < total ? requested : undefined;
}

export function useLibraryInfiniteQuery<T>(
  kind: LibraryKind,
  opts: { sort?: string; search: string; source: ServicePlatform }
) {
  const { sort = '', search, source } = opts;
  const queryKey =
    kind === 'playlists'
      ? libraryKeys.playlists(search, source)
      : libraryKeys[kind](sort, search, source);

  const query = useInfiniteQuery({
    queryKey,
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      queryLibrary<T>({
        kind,
        offset: pageParam as number,
        limit: LIBRARY_PAGE_SIZE,
        sort,
        search,
        source,
      }),
    getNextPageParam: (last, all) => nextOffset(last, all.length),
  });

  const items = useMemo<T[]>(() => (query.data?.pages ?? []).flatMap((p) => p.items), [query.data]);
  const total = query.data?.pages?.[0]?.total ?? 0;

  return { query, items, total };
}

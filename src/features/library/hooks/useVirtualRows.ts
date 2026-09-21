import { useEffect } from 'react';
import type { VirtualItem } from '@tanstack/react-virtual';
import { useVisibleCoverUrls } from '@/features/library/useCoverUrls';

type PagedQuery = {
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  fetchNextPage: () => unknown;
};

/** Fetch the next page once the last row comes within `lookahead` rows of the end. */
export function useInfiniteRows(
  query: PagedQuery,
  rows: VirtualItem[],
  rowCount: number,
  lookahead = 3
) {
  useEffect(() => {
    const last = rows.length > 0 ? rows[rows.length - 1] : undefined;
    if (!last) return;
    if (query.hasNextPage && !query.isFetchingNextPage && last.index >= rowCount - lookahead) {
      query.fetchNextPage();
    }
  }, [rows, rowCount, lookahead, query]);
}

/** Cover URLs for everything on screen; `cols === 1` is the list case. */
export function useVisibleGridCovers(
  rows: VirtualItem[],
  items: { cover_id?: string | null }[],
  cols: number
) {
  return useVisibleCoverUrls(
    rows.flatMap((r) => Array.from({ length: cols }, (_, c) => items[r.index * cols + c]?.cover_id))
  );
}

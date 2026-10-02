import { useMemo } from 'react';
import { errorDetail } from '@/utils/errors';
import { useInfiniteQuery } from '@tanstack/react-query';
import { searchService } from '@/services/ipc';
import { useSearchStore } from '../stores/searchStore';
import { logInfo, logError } from '@/utils/logger';

const PAGE_SIZE = 20;
const PAGINATED_PLATFORMS = new Set(['spotify', 'deezer', 'qobuz']);

export function useSearch(query: string, enabled = true) {
  const platform = useSearchStore((state) => state.selectedPlatform);
  const searchType = useSearchStore((state) => state.searchType);

  const q = useInfiniteQuery({
    queryKey: ['search', platform, searchType, query],
    initialPageParam: 0,
    queryFn: async ({ pageParam }) => {
      const offset = pageParam as number;
      logInfo(
        'search',
        `Searching ${platform}`,
        `Searching for "${query}" (${searchType}) on ${platform} @${offset}`
      );
      try {
        const results = await searchService.performSearch({
          platform,
          query,
          type: searchType,
          offset,
          limit: PAGE_SIZE,
        });
        logInfo(
          'search',
          `Search complete`,
          `Found ${results?.length ?? 0} results for "${query}" on ${platform}`
        );
        return results;
      } catch (err) {
        logError(
          'search',
          'Search failed',
          `Search for "${query}" on ${platform} failed: ${errorDetail(err)}`
        );
        throw err;
      }
    },
    getNextPageParam: (lastPage, allPages) => {
      if (!PAGINATED_PLATFORMS.has(platform)) return undefined;
      if (!lastPage || lastPage.length < PAGE_SIZE) return undefined;
      return allPages.reduce((acc, p) => acc + p.length, 0);
    },
    enabled: enabled && query.length > 0,
    staleTime: 5 * 60 * 1000,
    retry: 1,
  });

  const data = useMemo(() => (q.data?.pages ?? []).flat(), [q.data]);
  return { ...q, data };
}

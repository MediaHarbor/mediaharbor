import { useQuery } from '@tanstack/react-query';
import { searchService } from '@/services/ipc';
import { useSearchStore } from '../stores/searchStore';
import { logInfo, logError } from '@/utils/logger';

export function useSearch(query: string, enabled = true) {
  const platform = useSearchStore((state) => state.selectedPlatform);
  const searchType = useSearchStore((state) => state.searchType);

  return useQuery({
    queryKey: ['search', platform, searchType, query],
    queryFn: async () => {
      logInfo('search', `Searching ${platform}`, `Searching for "${query}" (${searchType}) on ${platform}`);
      try {
        const results = await searchService.performSearch({
          platform,
          query,
          type: searchType,
        });
        logInfo('search', `Search complete`, `Found ${results?.length ?? 0} results for "${query}" on ${platform}`);
        return results;
      } catch (err) {
        logError('search', 'Search failed', `Search for "${query}" on ${platform} failed: ${err instanceof Error ? (err.stack || err.message) : String(err)}`);
        throw err;
      }
    },
    enabled: enabled && query.length > 0,
    staleTime: 5 * 60 * 1000,
    retry: 1,
  });
}


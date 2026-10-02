import { create } from 'zustand';
import type { Platform, SearchType } from '@/types';
import { PLATFORM_SEARCH_TYPES } from '@/utils/platform-data';

interface SearchStore {
  selectedPlatform: Platform;
  setSelectedPlatform: (platform: Platform) => void;
  searchType: SearchType;
  setSearchType: (type: SearchType) => void;
  getAvailableTypes: () => SearchType[];
  pendingQuery: string | null;
  setPendingQuery: (query: string) => void;
  consumePendingQuery: () => string | null;
}

export const useSearchStore = create<SearchStore>((set, get) => ({
  pendingQuery: null,
  setPendingQuery: (query) => set({ pendingQuery: query }),
  consumePendingQuery: () => {
    const q = get().pendingQuery;
    if (q !== null) set({ pendingQuery: null });
    return q;
  },
  selectedPlatform: 'spotify',
  setSelectedPlatform: (platform) => {
    const available = PLATFORM_SEARCH_TYPES[platform] ?? ['track'];
    const current = get().searchType;
    set({
      selectedPlatform: platform,
      searchType: available.includes(current) ? current : available[0],
    });
  },
  searchType: 'track',
  setSearchType: (type) => set({ searchType: type }),
  getAvailableTypes: () => {
    const { selectedPlatform } = get();
    return PLATFORM_SEARCH_TYPES[selectedPlatform] ?? ['track'];
  },
}));

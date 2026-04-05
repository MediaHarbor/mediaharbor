import { create } from 'zustand';
import type { Platform, SearchType } from '@/types';
import { PLATFORM_SEARCH_TYPES } from '@/utils/platform-data';

interface SearchStore {
  selectedPlatform: Platform;
  setSelectedPlatform: (platform: Platform) => void;
  searchType: SearchType;
  setSearchType: (type: SearchType) => void;
  getAvailableTypes: () => SearchType[];
}

export const useSearchStore = create<SearchStore>((set, get) => ({
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

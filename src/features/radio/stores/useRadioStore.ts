import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export type RadioTab = 'home' | 'browse' | 'favorites' | 'playlists';
export type RadioView = 'grid' | 'list';

/**
 * One value per facet rather than a set.
 *
 * The directories take a single `tag` / `countrycode` / `language` / `codec` per
 * query, so offering checkboxes would quietly apply only one of them. Clicking a
 * selected value again clears it.
 */
export interface RadioFilters {
  /** `null` means every source the user has enabled. */
  source: string | null;
  tag: string | null;
  countryCode: string | null;
  language: string | null;
  codec: string | null;
  /** 0 = any. */
  bitrateMin: number;
  order: string;
}

export const NO_FILTERS: RadioFilters = {
  source: null,
  tag: null,
  countryCode: null,
  language: null,
  codec: null,
  bitrateMin: 0,
  order: 'clickcount',
};

/** `0` means "as many as fit", using the shared width breakpoints. */
export const AUTO_COLUMNS = 0;
export const MIN_COLUMNS = 2;
export const MAX_COLUMNS = 10;

interface RadioState {
  tab: RadioTab;
  view: RadioView;
  columns: number;
  filters: RadioFilters;
  /** The playlist being looked at, or `null` for the playlist shelf. */
  openListId: number | null;

  setTab: (tab: RadioTab) => void;
  openList: (id: number | null) => void;
  setView: (view: RadioView) => void;
  setColumns: (columns: number) => void;
  setFilter: <K extends keyof RadioFilters>(key: K, value: RadioFilters[K]) => void;
  clearFilters: () => void;
}

export const useRadioStore = create<RadioState>()(
  persist(
    (set) => ({
      tab: 'home',
      view: 'grid',
      columns: AUTO_COLUMNS,
      filters: { ...NO_FILTERS },
      openListId: null,

      // Leaving the tab closes whatever list was open, so coming back lands on
      // the shelf rather than inside a playlist that may since have been deleted.
      setTab: (tab) => set({ tab, openListId: null }),
      openList: (openListId) => set({ openListId }),
      setView: (view) => set({ view }),
      setColumns: (columns) =>
        set({
          columns:
            columns === AUTO_COLUMNS
              ? AUTO_COLUMNS
              : Math.min(MAX_COLUMNS, Math.max(MIN_COLUMNS, columns)),
        }),
      setFilter: (key, value) => set((s) => ({ filters: { ...s.filters, [key]: value } })),
      clearFilters: () => set({ filters: { ...NO_FILTERS } }),
    }),
    {
      name: 'mh-radio',
      // Filters are deliberately not persisted: coming back to a page silently
      // narrowed to one country reads as an empty directory, not as a filter.
      partialize: (s) => ({ tab: s.tab, view: s.view, columns: s.columns }),
    }
  )
);

/** How many active facet filters there are, for the "clear all" affordance. */
export function activeFilterCount(filters: RadioFilters): number {
  return (
    [filters.source, filters.tag, filters.countryCode, filters.language, filters.codec].filter(
      Boolean
    ).length + (filters.bitrateMin > 0 ? 1 : 0)
  );
}

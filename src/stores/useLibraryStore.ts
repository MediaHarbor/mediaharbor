import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { ServicePlatform } from '@/features/library/api';

export type LibraryTab =
  | 'home'
  | 'tracks'
  | 'albums'
  | 'artists'
  | 'playlists'
  | 'videos'
  | 'following'
  | 'activity'
  | 'bookmarks';

export const LIBRARY_TABS: LibraryTab[] = ['tracks', 'albums', 'artists', 'playlists', 'videos'];

export const SERVICE_LIBRARY_TABS: LibraryTab[] = [
  'home',
  'tracks',
  'albums',
  'artists',
  'playlists',
  'videos',
  'following',
  'activity',
  'bookmarks',
];

export type TabSortKey = string;

export type LibraryView =
  | { kind: 'album'; albumKey: string }
  | { kind: 'artist'; artistKey: string }
  | { kind: 'playlist'; id: number; serviceId: string | null };

function sameView(a: LibraryView, b: LibraryView): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === 'album' && b.kind === 'album') return a.albumKey === b.albumKey;
  if (a.kind === 'artist' && b.kind === 'artist') return a.artistKey === b.artistKey;
  if (a.kind === 'playlist' && b.kind === 'playlist') {
    return a.id === b.id && a.serviceId === b.serviceId;
  }
  return false;
}

const DEFAULT_SORT: Record<LibraryTab, TabSortKey> = {
  home: 'name',
  tracks: 'recent',
  albums: 'recent',
  artists: 'name',
  playlists: 'recent',
  videos: 'recent',
  following: 'name',
  activity: 'recent',
  bookmarks: 'recent',
};

interface LibraryState {
  isScanning: boolean;
  isRefreshing: boolean;
  scanProgress: number;
  scanFile: string;
  lastScanned: number | null;
  downloadDir: string;

  tab: LibraryTab;
  perTabSort: Record<LibraryTab, TabSortKey>;
  activeSource: ServicePlatform;
  viewStack: LibraryView[];
  returnPath: string | null;

  setIsScanning: (v: boolean) => void;
  setIsRefreshing: (v: boolean) => void;
  setScanProgress: (v: number) => void;
  setScanFile: (v: string) => void;
  setLastScanned: (v: number) => void;
  setDownloadDir: (v: string) => void;

  setTab: (t: LibraryTab) => void;
  setSort: (tab: LibraryTab, sort: TabSortKey) => void;
  setActiveSource: (source: ServicePlatform) => void;
  pushView: (view: LibraryView) => void;
  popView: () => void;
  clearViews: () => void;
  setReturnPath: (path: string | null) => void;
}

export const useLibraryStore = create<LibraryState>()(
  persist(
    (set) => ({
      isScanning: false,
      isRefreshing: false,
      scanProgress: 0,
      scanFile: '',
      lastScanned: null,
      downloadDir: '',

      tab: 'albums' as LibraryTab,
      perTabSort: { ...DEFAULT_SORT },
      activeSource: 'local' as ServicePlatform,
      viewStack: [],
      returnPath: null,

      setIsScanning: (v) => set({ isScanning: v }),
      setIsRefreshing: (v) => set({ isRefreshing: v }),
      setScanProgress: (v) => set({ scanProgress: v }),
      setScanFile: (v) => set({ scanFile: v }),
      setLastScanned: (v) => set({ lastScanned: v }),
      setDownloadDir: (v) => set({ downloadDir: v }),

      setTab: (t) => set({ tab: t }),
      setSort: (tab, sort) => set((s) => ({ perTabSort: { ...s.perTabSort, [tab]: sort } })),
      setActiveSource: (source) => set({ activeSource: source }),
      pushView: (view) =>
        set((s) => {
          const top = s.viewStack[s.viewStack.length - 1];
          if (top && sameView(top, view)) return s;
          return { viewStack: [...s.viewStack, view] };
        }),
      popView: () => set((s) => ({ viewStack: s.viewStack.slice(0, -1) })),
      clearViews: () => set({ viewStack: [], returnPath: null }),
      setReturnPath: (path) => set({ returnPath: path }),
    }),
    {
      name: 'mh-library',
      partialize: (s) => ({ tab: s.tab, perTabSort: s.perTabSort }),
    }
  )
);

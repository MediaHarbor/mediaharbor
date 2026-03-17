import { create } from 'zustand';

export interface TrackItem {
  title: string;
  type: 'music';
  size: string;
  date: string;
  path: string;
  duration?: number;
  quality?: string;
  thumbnail?: { data: string; format: string };
  metadata: { artist: string; album: string; year: string | number };
}

export interface AlbumItem {
  type: 'music';
  album: string;
  artist: string;
  year: string | number;
  thumbnail?: { data: string; format: string };
  tracks: TrackItem[];
}

export interface VideoItem {
  title: string;
  type: 'video';
  size: string;
  date: string;
  path: string;
  duration?: number;
  thumbnail?: { data: string; format: string };
  metadata: Record<string, unknown>;
}

export type MediaItem = AlbumItem | VideoItem;

interface LibraryState {
  items: MediaItem[];
  isScanning: boolean;
  isRefreshing: boolean;
  scanProgress: number;
  scanFile: string;
  lastScanned: number | null;
  downloadDir: string;

  setItems: (items: MediaItem[]) => void;
  addItems: (newItems: MediaItem[]) => void;
  removeItemsByPaths: (paths: string[]) => void;
  setIsScanning: (v: boolean) => void;
  setIsRefreshing: (v: boolean) => void;
  setScanProgress: (v: number) => void;
  setScanFile: (v: string) => void;
  setLastScanned: (v: number) => void;
  setDownloadDir: (v: string) => void;
}

export const useLibraryStore = create<LibraryState>()((set) => ({
  items: [],
  isScanning: false,
  isRefreshing: false,
  scanProgress: 0,
  scanFile: '',
  lastScanned: null,
  downloadDir: '',

  setItems: (items) => set({ items }),

  addItems: (newItems) =>
    set((state) => {
      const merged = [...state.items];
      const normalizeKey = (album: string, artist: string) =>
        `${(album || '').trim().toLowerCase().replace(/\s+/g, ' ')}::${(artist || '').trim().toLowerCase().replace(/\s+/g, ' ')}`;

      const videoIndex = new Map<string, number>();
      const albumIndex = new Map<string, number>();
      for (let i = 0; i < merged.length; i++) {
        const item = merged[i];
        if (item.type === 'video') {
          videoIndex.set((item as VideoItem).path, i);
        } else {
          albumIndex.set(normalizeKey((item as AlbumItem).album, (item as AlbumItem).artist), i);
        }
      }

      for (const newItem of newItems) {
        if (newItem.type === 'video') {
          const vi = newItem as VideoItem;
          const idx = videoIndex.get(vi.path);
          if (idx !== undefined) {
            merged[idx] = vi;
          } else {
            videoIndex.set(vi.path, merged.length);
            merged.push(vi);
          }
        } else {
          const ai = newItem as AlbumItem;
          const key = normalizeKey(ai.album, ai.artist);
          const idx = albumIndex.get(key);
          if (idx !== undefined) {
            const existing = merged[idx] as AlbumItem;
            const trackPaths = new Set(existing.tracks.map((t) => t.path));
            const newTracks = ai.tracks.filter((t) => !trackPaths.has(t.path));
            merged[idx] = {
              ...existing,
              tracks: [...existing.tracks, ...newTracks],
              thumbnail: existing.thumbnail || ai.thumbnail,
            };
          } else {
            albumIndex.set(key, merged.length);
            merged.push(ai);
          }
        }
      }
      return { items: merged, lastScanned: Date.now() };
    }),

  removeItemsByPaths: (paths) =>
    set((state) => {
      const pathSet = new Set(paths);
      const updated: MediaItem[] = [];
      for (const item of state.items) {
        if (item.type === 'video') {
          if (!pathSet.has((item as VideoItem).path)) updated.push(item);
        } else {
          const album = item as AlbumItem;
          const remainingTracks = album.tracks.filter((t) => !pathSet.has(t.path));
          if (remainingTracks.length > 0) {
            updated.push({ ...album, tracks: remainingTracks });
          }
        }
      }
      return { items: updated, lastScanned: Date.now() };
    }),

  setIsScanning: (v) => set({ isScanning: v }),
  setIsRefreshing: (v) => set({ isRefreshing: v }),
  setScanProgress: (v) => set({ scanProgress: v }),
  setScanFile: (v) => set({ scanFile: v }),
  setLastScanned: (v) => set({ lastScanned: v }),
  setDownloadDir: (v) => set({ downloadDir: v }),
}));

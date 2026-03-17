import { create } from 'zustand';

type DownloadStatus = 'downloading' | 'complete' | 'error' | 'cancelled';

interface DownloadItem {
  order: number;
  title: string;
  artist: string;
  album?: string;
  thumbnail: string | null;
  progress: number;
  status: DownloadStatus;
  platform?: string;
  quality?: string;
  error?: string;
  location?: string;
}

interface DownloadState {
  items: DownloadItem[];
  addOrUpdate: (item: Partial<DownloadItem> & { order: number }) => void;
  remove: (order: number) => void;
  cancel: (order: number) => void;
  clear: () => void;
}

export const useDownloadStore = create<DownloadState>((set) => ({
  items: [],

  addOrUpdate: (incoming) =>
    set((state) => {
      const patch = Object.fromEntries(
        Object.entries(incoming).filter(([, v]) => v != null)
      ) as Partial<DownloadItem> & { order: number };

      const existing = state.items.find((i) => i.order === incoming.order);
      if (existing) {
        return {
          items: state.items.map((i) =>
            i.order === incoming.order ? { ...i, ...patch } : i
          ),
        };
      }
      return {
        items: [
          ...state.items,
          {
            order: incoming.order,
            title: incoming.title ?? 'Downloading...',
            artist: incoming.artist ?? '',
            thumbnail: incoming.thumbnail ?? null,
            progress: incoming.progress ?? 0,
            status: incoming.status ?? 'downloading',
            error: incoming.error,
          },
        ],
      };
    }),

  remove: (order) =>
    set((state) => ({ items: state.items.filter((i) => i.order !== order) })),

  cancel: (order) => {
    window.electron?.downloads.cancel(order);
    set((state) => ({
      items: state.items.map((i) =>
        i.order === order ? { ...i, status: 'cancelled' as DownloadStatus } : i
      ),
    }));
  },

  clear: () => set({ items: [] }),
}));

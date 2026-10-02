import { create } from 'zustand';
import type { Platform } from '@/types';

export interface DownloadDialogRequest {
  platform: Platform;
  url: string;
  kind: 'track' | 'album' | 'playlist';
  title?: string | null;
  artist?: string | null;
  album?: string | null;
  thumbnail?: string | null;
}

interface DownloadDialogState {
  open: boolean;
  request: DownloadDialogRequest | null;
  requestDownload: (req: DownloadDialogRequest) => void;
  close: () => void;
}

export const useDownloadDialogStore = create<DownloadDialogState>((set) => ({
  open: false,
  request: null,
  requestDownload: (req) => set({ open: true, request: req }),
  close: () => set({ open: false, request: null }),
}));

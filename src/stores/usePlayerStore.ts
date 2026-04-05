import { create } from 'zustand';

export type MediaType = 'audio' | 'video';

export interface PlayableTrack {
  url: string;
  title: string;
  artist: string;
  thumbnail?: string;
  platform?: string;
  mediaType?: MediaType;
}

interface PlayerState {
  streamUrl: string | null;
  title: string;
  artist: string;
  thumbnail: string | null;
  mediaType: MediaType;
  isPlaying: boolean;
  platform: string | null;
  queue: PlayableTrack[];
  queueIndex: number;
  setTrack: (track: {
    streamUrl: string;
    title: string;
    artist: string;
    thumbnail?: string;
    mediaType?: MediaType;
    platform?: string;
  }) => void;
  setQueue: (tracks: PlayableTrack[], startIndex?: number) => void;
  insertNext: (track: PlayableTrack) => void;
  appendToQueue: (tracks: PlayableTrack[]) => void;
  playNext: () => void;
  playPrev: () => void;
  setPlaying: (playing: boolean) => void;
  clear: () => void;
}

export const usePlayerStore = create<PlayerState>((set, get) => ({
  streamUrl: null,
  title: '',
  artist: '',
  thumbnail: null,
  mediaType: 'audio',
  isPlaying: false,
  platform: null,
  queue: [],
  queueIndex: -1,

  setTrack: (track) =>
    set({
      streamUrl: track.streamUrl,
      title: track.title,
      artist: track.artist,
      thumbnail: track.thumbnail ?? null,
      mediaType: track.mediaType ?? 'audio',
      isPlaying: true,
      platform: track.platform ?? null,
      queue: [],
      queueIndex: -1,
    }),

  setQueue: (tracks, startIndex = 0) => {
    if (!tracks.length) return;
    const track = tracks[startIndex];
    set({
      queue: tracks,
      queueIndex: startIndex,
      streamUrl: null,
      title: track.title,
      artist: track.artist,
      thumbnail: track.thumbnail ?? null,
      mediaType: track.mediaType ?? 'audio',
      isPlaying: true,
      platform: track.platform ?? null,
    });
  },

  playNext: () => {
    const { queue, queueIndex } = get();
    const next = queueIndex + 1;
    if (next >= queue.length) return;
    const track = queue[next];
    set({ queueIndex: next, streamUrl: null, title: track.title, artist: track.artist, thumbnail: track.thumbnail ?? null, mediaType: track.mediaType ?? 'audio', isPlaying: true, platform: track.platform ?? null });
  },

  playPrev: () => {
    const { queue, queueIndex } = get();
    const prev = queueIndex - 1;
    if (prev < 0) return;
    const track = queue[prev];
    set({ queueIndex: prev, streamUrl: null, title: track.title, artist: track.artist, thumbnail: track.thumbnail ?? null, mediaType: track.mediaType ?? 'audio', isPlaying: true, platform: track.platform ?? null });
  },

  insertNext: (track) => {
    const { queue, queueIndex } = get();
    if (!queue.length) {
      set({
        queue: [track], queueIndex: 0, streamUrl: null,
        title: track.title, artist: track.artist,
        thumbnail: track.thumbnail ?? null,
        mediaType: track.mediaType ?? 'audio',
        isPlaying: true, platform: track.platform ?? null,
      });
      return;
    }
    const next = [...queue];
    next.splice(queueIndex + 1, 0, track);
    set({ queue: next });
  },

  appendToQueue: (tracks) => {
    const { queue } = get();
    set({ queue: [...queue, ...tracks] });
  },

  setPlaying: (isPlaying) => set({ isPlaying }),

  clear: () =>
    set({
      streamUrl: null, title: '', artist: '', thumbnail: null,
      mediaType: 'audio', isPlaying: false, platform: null,
      queue: [], queueIndex: -1,
    }),
}));

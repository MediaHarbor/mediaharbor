import { create } from 'zustand';

export type MediaType = 'audio' | 'video';
export type RepeatMode = 'off' | 'one' | 'all';
export type NowPlayingView = 'artwork' | 'lyrics';

export interface SyncedLine {
  time: number;
  text: string;
}

export interface WordTiming {
  start: number;
  end: number;
  text: string;
}

export interface WordSyncedLine {
  startTime: number;
  endTime: number;
  text: string;
  words: WordTiming[];
}

export interface PlayableTrack {
  url: string;
  title: string;
  artist: string;
  thumbnail?: string;
  platform?: string;
  mediaType?: MediaType;
}

function applyTrack(track: PlayableTrack) {
  return {
    streamUrl: null,
    title: track.title,
    artist: track.artist,
    thumbnail: track.thumbnail ?? null,
    mediaType: track.mediaType ?? 'audio' as MediaType,
    isLive: false,
    isPlaying: true,
    platform: track.platform ?? null,
    syncedLyrics: null as SyncedLine[] | null,
    plainLyrics: null as string | null,
    wordSyncedLyrics: null as WordSyncedLine[] | null,
    lyricsLoading: false,
  };
}

interface PlayerState {
  streamUrl: string | null;
  title: string;
  artist: string;
  thumbnail: string | null;
  mediaType: MediaType;
  isLive: boolean;
  isPlaying: boolean;
  platform: string | null;
  queue: PlayableTrack[];
  queueIndex: number;
  shuffle: boolean;
  repeat: RepeatMode;
  shuffleHistory: number[];
  nowPlayingOpen: boolean;
  syncedLyrics: SyncedLine[] | null;
  plainLyrics: string | null;
  wordSyncedLyrics: WordSyncedLine[] | null;
  lyricsLoading: boolean;
  activeNowPlayingView: NowPlayingView;
  lyricsEnabled: boolean;
  mediaElement: HTMLMediaElement | null;
  streamCache: Record<string, { streamUrl: string; mediaType?: string; isLive?: boolean }>;
  cacheStream: (trackUrl: string, streamUrl: string, mediaType?: string, isLive?: boolean) => void;
  getCachedStream: (trackUrl: string) => { streamUrl: string; mediaType?: string; isLive?: boolean } | null;
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
  toggleShuffle: () => void;
  cycleRepeat: () => void;
  toggleNowPlaying: () => void;
  miniSidebarOpen: boolean;
  toggleMiniSidebar: () => void;
  miniSidebarView: 'queue' | 'lyrics';
  setMiniSidebarView: (view: 'queue' | 'lyrics') => void;
  playFromQueue: (index: number) => void;
  removeFromQueue: (index: number) => void;
  setActiveNowPlayingView: (view: NowPlayingView) => void;
  toggleLyricsEnabled: () => void;
  fetchLyrics: () => void;
  setMediaElement: (el: HTMLMediaElement | null) => void;
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
  shuffle: false,
  repeat: 'off',
  shuffleHistory: [],
  nowPlayingOpen: false,
  syncedLyrics: null,
  plainLyrics: null,
  wordSyncedLyrics: null,
  lyricsLoading: false,
  activeNowPlayingView: 'artwork' as NowPlayingView,
  lyricsEnabled: true,
  mediaElement: null,
  isLive: false,
  streamCache: {},
  cacheStream: (trackUrl, streamUrl, mediaType, isLive) =>
    set((state) => {
      const updated = { ...state.streamCache, [trackUrl]: { streamUrl, mediaType, isLive } };
      const keys = Object.keys(updated);
      if (keys.length > 5) delete updated[keys[0]];
      return { streamCache: updated };
    }),
  getCachedStream: (trackUrl) => get().streamCache[trackUrl] || null,
  miniSidebarOpen: true,
  miniSidebarView: 'queue' as 'queue' | 'lyrics',

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
      shuffleHistory: [],
    }),

  setQueue: (tracks, startIndex = 0) => {
    if (!tracks.length) return;
    const track = tracks[startIndex];
    set({
      queue: tracks,
      queueIndex: startIndex,
      shuffleHistory: [startIndex],
      ...applyTrack(track),
    });
  },

  playNext: () => {
    const { queue, queueIndex, shuffle, repeat, shuffleHistory } = get();
    if (!queue.length) return;

    if (repeat === 'one') {
      const track = queue[queueIndex];
      if (track) set({ streamUrl: null, isPlaying: true });
      return;
    }

    if (shuffle) {
      const played = new Set(shuffleHistory);
      const remaining = queue
        .map((_, i) => i)
        .filter((i) => !played.has(i));

      if (remaining.length > 0) {
        const next = remaining[Math.floor(Math.random() * remaining.length)];
        const track = queue[next];
        set({
          queueIndex: next,
          shuffleHistory: [...shuffleHistory, next],
          ...applyTrack(track),
        });
        return;
      }

      if (repeat === 'all') {
        const next = Math.floor(Math.random() * queue.length);
        const track = queue[next];
        set({
          queueIndex: next,
          shuffleHistory: [next],
          ...applyTrack(track),
        });
        return;
      }

      set({ isPlaying: false });
      return;
    }

    const next = queueIndex + 1;
    if (next < queue.length) {
      const track = queue[next];
      set({ queueIndex: next, ...applyTrack(track) });
      return;
    }

    if (repeat === 'all') {
      const track = queue[0];
      set({ queueIndex: 0, ...applyTrack(track) });
      return;
    }

    set({ isPlaying: false });
  },

  playPrev: () => {
    const { queue, queueIndex, shuffle, shuffleHistory } = get();

    if (shuffle && shuffleHistory.length > 1) {
      const newHistory = [...shuffleHistory];
      newHistory.pop();
      const prev = newHistory[newHistory.length - 1];
      const track = queue[prev];
      if (track) {
        set({ queueIndex: prev, shuffleHistory: newHistory, ...applyTrack(track) });
      }
      return;
    }

    const prev = queueIndex - 1;
    if (prev < 0) return;
    const track = queue[prev];
    set({ queueIndex: prev, ...applyTrack(track) });
  },

  insertNext: (track) => {
    const { queue, queueIndex } = get();
    if (!queue.length) {
      set({
        queue: [track], queueIndex: 0,
        shuffleHistory: [0],
        ...applyTrack(track),
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

  setPlaying: (isPlaying) => {
    set({ isPlaying });
  },

  toggleShuffle: () => {
    const { shuffle, queueIndex } = get();
    set({
      shuffle: !shuffle,
      shuffleHistory: !shuffle ? [queueIndex] : [],
    });
  },

  cycleRepeat: () => {
    const { repeat } = get();
    const next: RepeatMode = repeat === 'off' ? 'all' : repeat === 'all' ? 'one' : 'off';
    set({ repeat: next });
  },

  toggleNowPlaying: () => {
    set((s) => ({ nowPlayingOpen: !s.nowPlayingOpen }));
  },

  toggleMiniSidebar: () => {
    set((s) => ({ miniSidebarOpen: !s.miniSidebarOpen }));
  },

  setMiniSidebarView: (view) => {
    set({ miniSidebarView: view });
  },

  playFromQueue: (index: number) => {
    const { queue, shuffleHistory } = get();
    if (index < 0 || index >= queue.length) return;
    const track = queue[index];
    set({
      queueIndex: index,
      shuffleHistory: [...shuffleHistory, index],
      ...applyTrack(track),
    });
  },

  removeFromQueue: (index: number) => {
    const { queue, queueIndex } = get();
    if (index < 0 || index >= queue.length) return;
    const next = [...queue];
    next.splice(index, 1);
    let newIndex = queueIndex;
    if (index < queueIndex) newIndex--;
    else if (index === queueIndex && newIndex >= next.length) newIndex = next.length - 1;
    set({ queue: next, queueIndex: newIndex });
  },

  setActiveNowPlayingView: (view: NowPlayingView) => set({ activeNowPlayingView: view }),

  toggleLyricsEnabled: () => set((state) => ({ lyricsEnabled: !state.lyricsEnabled })),

  setMediaElement: (el: HTMLMediaElement | null) => set({ mediaElement: el }),

  fetchLyrics: async () => {
    const { title, artist, platform, queue, queueIndex } = get();
    if (!title) return;
    const currentUrl = queue[queueIndex]?.url ?? '';
    if (!currentUrl) return;

    set({ lyricsLoading: true, syncedLyrics: null, plainLyrics: null, wordSyncedLyrics: null });

    try {
      const result = await window.electron?.lyrics.get({
        url: currentUrl,
        platform: platform ?? '',
        title,
        artist,
        duration: get().mediaElement?.duration || undefined,
      });

      const nowUrl = get().queue[get().queueIndex]?.url ?? '';
      if (nowUrl !== currentUrl) return;

      let synced: SyncedLine[] | null = null;
      if (result?.synced) {
        const lines: SyncedLine[] = [];
        for (const line of result.synced.split('\n')) {
          const match = line.match(/\[(\d{2}):(\d{2})\.(\d{2,3})\](.*)/);
          if (match) {
            const mins = parseInt(match[1], 10);
            const secs = parseInt(match[2], 10);
            const ms = match[3].length === 2 ? parseInt(match[3], 10) * 10 : parseInt(match[3], 10);
            lines.push({ time: mins * 60 + secs + ms / 1000, text: match[4] });
          }
        }
        if (lines.length > 0) synced = lines;
      }

      let wordSynced: WordSyncedLine[] | null = null;
      if (result?.wordSynced) {
        try {
          const parsed = JSON.parse(result.wordSynced);
          if (Array.isArray(parsed) && parsed.length > 0) {
            wordSynced = parsed as WordSyncedLine[];
          }
        } catch {} // eslint-disable-line no-empty
      }

      set({
        syncedLyrics: synced,
        plainLyrics: result?.plain ?? null,
        wordSyncedLyrics: wordSynced,
        lyricsLoading: false,
      });
    } catch {
      const nowUrl = get().queue[get().queueIndex]?.url ?? '';
      if (nowUrl === currentUrl) {
        set({ lyricsLoading: false });
      }
    }
  },

  clear: () =>
    set({
      mediaType: 'audio', isPlaying: false, platform: null,
      queue: [], queueIndex: -1, shuffleHistory: [],
      nowPlayingOpen: false, syncedLyrics: null, plainLyrics: null,
      wordSyncedLyrics: null,
      lyricsLoading: false, activeNowPlayingView: 'artwork' as NowPlayingView,
    }),
}));

let prevLyricsUrl = '';
usePlayerStore.subscribe((state) => {
  const url = state.queue[state.queueIndex]?.url ?? '';
  if (state.title && url && url !== prevLyricsUrl) {
    prevLyricsUrl = url;
    setTimeout(() => usePlayerStore.getState().fetchLyrics(), 50);
  }
});

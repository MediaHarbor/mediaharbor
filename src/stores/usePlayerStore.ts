import { create } from 'zustand';
import { tauriAPI } from '@/tauri-bridge';

/** Resolved stream URLs carry signed CDN links that expire (Qobuz ~2h). Re-resolve
 *  well before then. 50min is safely under every service's signed window. */
const STREAM_CACHE_TTL_MS = 50 * 60 * 1000;

export type MediaType = 'audio' | 'video';
export type RepeatMode = 'off' | 'one' | 'all';
export type NowPlayingView = 'artwork' | 'lyrics' | 'video' | 'canvas' | 'description';

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
  album?: string | null;
  thumbnail?: string;
  coverId?: string | null;
  albumId?: string | null;
  artistId?: string | null;
  platform?: string;
  mediaType?: MediaType;
  videoUrl?: string | null;
  contextUri?: string | null;
  trackIndex?: number | null;
}

function applyTrack(track: PlayableTrack) {
  return {
    streamUrl: null,
    audioStreamUrl: null,
    title: track.title,
    artist: track.artist,
    album: track.album ?? null,
    thumbnail: track.thumbnail ?? null,
    mediaType: track.mediaType ?? ('audio' as MediaType),
    isLive: false,
    liveTitle: null,
    isPlaying: true,
    isBuffering: false,
    platform: track.platform ?? null,
    syncedLyrics: null as SyncedLine[] | null,
    plainLyrics: null as string | null,
    wordSyncedLyrics: null as WordSyncedLine[] | null,
    lyricsLoading: false,
    position: 0,
    duration: 0,
  };
}

function nextIndexState(s: {
  queue: PlayableTrack[];
  queueIndex: number;
  shuffle: boolean;
  repeat: RepeatMode;
  shuffleHistory: number[];
}): { index: number | null; shuffleHistory: number[] } {
  const { queue, queueIndex, shuffle, repeat, shuffleHistory } = s;
  if (!queue.length) return { index: null, shuffleHistory };

  if (shuffle) {
    const played = new Set(shuffleHistory);
    const remaining = queue.map((_, i) => i).filter((i) => !played.has(i));
    if (remaining.length > 0) {
      const next = remaining[Math.floor(Math.random() * remaining.length)];
      return { index: next, shuffleHistory: [...shuffleHistory, next] };
    }
    if (repeat === 'all') {
      const next = Math.floor(Math.random() * queue.length);
      return { index: next, shuffleHistory: [next] };
    }
    return { index: null, shuffleHistory };
  }

  const next = queueIndex + 1;
  if (next < queue.length) return { index: next, shuffleHistory };
  if (repeat === 'all') return { index: 0, shuffleHistory };
  return { index: null, shuffleHistory };
}

interface PlayerState {
  streamUrl: string | null;
  /** Audio served separately from the picture, for video. */
  audioStreamUrl: string | null;
  title: string;
  artist: string;
  album: string | null;
  thumbnail: string | null;
  mediaType: MediaType;
  isLive: boolean;
  liveTitle: string | null;
  isPlaying: boolean;
  isBuffering: boolean;
  platform: string | null;
  queue: PlayableTrack[];
  queueIndex: number;
  queueContextUri: string | null;
  shuffle: boolean;
  repeat: RepeatMode;
  autoplayEnabled: boolean;
  autoplayLastSeed: string | null;
  /// The endless station feeding the queue; `continuation` is its next-page token.
  autoplayStation: { platform: string; continuation: string } | null;
  toggleAutoplay: () => void;
  setAutoplayLastSeed: (seed: string | null) => void;
  setAutoplayStation: (station: { platform: string; continuation: string } | null) => void;
  shuffleHistory: number[];
  nowPlayingOpen: boolean;
  syncedLyrics: SyncedLine[] | null;
  plainLyrics: string | null;
  wordSyncedLyrics: WordSyncedLine[] | null;
  lyricsLoading: boolean;
  activeNowPlayingView: NowPlayingView;
  lyricsEnabled: boolean;
  mediaElement: HTMLMediaElement | null;
  /** Playback position in seconds. Fed by the native player for audio and by
   *  the media element for video, so consumers never poll the DOM. */
  position: number;
  duration: number;
  setPosition: (position: number, duration?: number) => void;
  seekTo: (seconds: number) => void;
  streamCache: Record<
    string,
    { streamUrl: string; mediaType?: string; isLive?: boolean; cachedAt: number }
  >;
  cacheStream: (trackUrl: string, streamUrl: string, mediaType?: string, isLive?: boolean) => void;
  getCachedStream: (
    trackUrl: string
  ) => { streamUrl: string; mediaType?: string; isLive?: boolean } | null;
  setQueue: (tracks: PlayableTrack[], startIndex?: number, contextUri?: string | null) => void;
  hydrateQueue: (tracks: PlayableTrack[]) => void;
  insertNext: (track: PlayableTrack) => void;
  appendToQueue: (tracks: PlayableTrack[]) => void;
  playNext: () => void;
  peekNext: () => { index: number | null; shuffleHistory: number[] };
  playPrev: () => void;
  setPlaying: (playing: boolean) => void;
  setBuffering: (buffering: boolean) => void;
  clear: () => void;
  toggleShuffle: () => void;
  cycleRepeat: () => void;
  toggleNowPlaying: () => void;
  closeNowPlaying: () => void;
  miniSidebarOpen: boolean;
  toggleMiniSidebar: () => void;
  playFromQueue: (index: number) => void;
  removeFromQueue: (index: number) => void;
  setActiveNowPlayingView: (view: NowPlayingView) => void;
  toggleLyricsEnabled: () => void;
  fetchLyrics: () => void;
  setMediaElement: (el: HTMLMediaElement | null) => void;
}

export const usePlayerStore = create<PlayerState>((set, get) => ({
  streamUrl: null,
  audioStreamUrl: null,
  title: '',
  artist: '',
  album: null,
  thumbnail: null,
  mediaType: 'audio',
  isPlaying: false,
  isBuffering: false,
  liveTitle: null,
  platform: null,
  queue: [],
  queueIndex: -1,
  queueContextUri: null,
  shuffle: false,
  repeat: 'off',
  autoplayEnabled: (() => {
    try {
      const v = localStorage.getItem('mh-autoplay-enabled');
      return v === null ? true : v === 'true';
    } catch {
      return true;
    }
  })(),
  autoplayLastSeed: null,
  autoplayStation: null,
  toggleAutoplay: () => {
    const next = !get().autoplayEnabled;
    try {
      localStorage.setItem('mh-autoplay-enabled', String(next));
    } catch {}
    set({ autoplayEnabled: next });
  },
  setAutoplayLastSeed: (seed) => set({ autoplayLastSeed: seed }),
  setAutoplayStation: (station) => set({ autoplayStation: station }),
  shuffleHistory: [],
  nowPlayingOpen: false,
  syncedLyrics: null,
  plainLyrics: null,
  wordSyncedLyrics: null,
  lyricsLoading: false,
  activeNowPlayingView: 'artwork' as NowPlayingView,
  lyricsEnabled: true,
  mediaElement: null,
  position: 0,
  duration: 0,
  isLive: false,
  streamCache: {},
  cacheStream: (trackUrl, streamUrl, mediaType, isLive) =>
    set((state) => {
      const updated = {
        ...state.streamCache,
        [trackUrl]: { streamUrl, mediaType, isLive, cachedAt: Date.now() },
      };
      const keys = Object.keys(updated);
      if (keys.length > 5) delete updated[keys[0]];
      return { streamCache: updated };
    }),
  getCachedStream: (trackUrl) => {
    const entry = get().streamCache[trackUrl];
    if (!entry) return null;
    if (Date.now() - entry.cachedAt > STREAM_CACHE_TTL_MS) {
      set((state) => {
        const { [trackUrl]: _drop, ...rest } = state.streamCache;
        return { streamCache: rest };
      });
      return null;
    }
    return entry;
  },
  miniSidebarOpen: true,

  setQueue: (tracks, startIndex = 0, contextUri = null) => {
    if (!tracks.length) return;
    const track = tracks[startIndex];
    set({
      queue: tracks,
      queueIndex: startIndex,
      queueContextUri: contextUri,
      shuffleHistory: [startIndex],
      autoplayStation: null,
      autoplayLastSeed: null,
      ...applyTrack(track),
    });
  },

  hydrateQueue: (tracks) =>
    set((s) => {
      if (!s.queue.length || !tracks.length) return {};
      const curUrl = s.queue[s.queueIndex]?.url;
      const found = curUrl ? tracks.findIndex((t) => t.url === curUrl) : -1;
      const keep = found >= 0 ? found : Math.min(s.queueIndex, tracks.length - 1);
      return {
        queue: tracks,
        queueIndex: keep,
        shuffleHistory: s.shuffle ? s.shuffleHistory : [keep],
      };
    }),

  playNext: () => {
    const s = get();
    if (!s.queue.length) return;

    if (s.repeat === 'one') {
      const track = s.queue[s.queueIndex];
      if (track) set({ streamUrl: null, isPlaying: true });
      return;
    }

    const { index, shuffleHistory } = nextIndexState(s);
    if (index == null) {
      set({ isPlaying: false });
      return;
    }
    const track = s.queue[index];
    set({ queueIndex: index, shuffleHistory, ...applyTrack(track) });
  },

  peekNext: () => nextIndexState(get()),

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
        queue: [track],
        queueIndex: 0,
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
    set({ isPlaying, ...(isPlaying ? {} : { isBuffering: false }) });
  },

  setBuffering: (isBuffering) => {
    set({ isBuffering });
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

  closeNowPlaying: () => set({ nowPlayingOpen: false }),

  toggleMiniSidebar: () => {
    set((s) => ({ miniSidebarOpen: !s.miniSidebarOpen }));
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

  setPosition: (position: number, duration?: number) =>
    set((state) => ({
      position,
      duration: duration != null && isFinite(duration) ? duration : state.duration,
    })),

  /** Seek the native player, which owns audio for every media type. For video
   *  the element is moved with it so the picture lands on the same frame. */
  seekTo: (seconds: number) => {
    const { mediaType, mediaElement } = get();
    set({ position: seconds });
    if (mediaType === 'video' && mediaElement) {
      mediaElement.currentTime = seconds;
    }
    void tauriAPI.player.native.seek(seconds).catch(() => {});
  },

  fetchLyrics: async () => {
    const { title, artist, platform, queue, queueIndex } = get();
    if (!title) return;
    const currentUrl = queue[queueIndex]?.url ?? '';
    if (!currentUrl) return;

    set({ lyricsLoading: true, syncedLyrics: null, plainLyrics: null, wordSyncedLyrics: null });

    try {
      const result = await tauriAPI.lyrics.get({
        url: currentUrl,
        platform: platform ?? '',
        title,
        artist,
        duration: get().duration || undefined,
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
            const raw = match[4];
            const text = raw.trim() === '' ? '♪' : raw;
            lines.push({ time: mins * 60 + secs + ms / 1000, text });
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
        } catch {}
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
      mediaType: 'audio',
      isPlaying: false,
      isBuffering: false,
      platform: null,
      queue: [],
      queueIndex: -1,
      shuffleHistory: [],
      nowPlayingOpen: false,
      syncedLyrics: null,
      plainLyrics: null,
      wordSyncedLyrics: null,
      lyricsLoading: false,
      activeNowPlayingView: 'artwork' as NowPlayingView,
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

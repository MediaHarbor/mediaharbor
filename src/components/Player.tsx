import { useState, useRef, useEffect } from 'react';
import {
  Play, Pause, SkipBack, SkipForward,
  Volume2, Volume1, VolumeX,
  Music2, Maximize2, Shuffle, Repeat,
} from 'lucide-react';
import { Slider } from '@/components/ui/slider';
import { Button } from '@/components/ui/button';
import { create } from 'zustand';
import { PLATFORM_COLORS } from '@/utils/platforms';
import { logError, logWarning, logInfo } from '@/utils/logger';
import { convertFileSrc } from '@tauri-apps/api/core';

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

export function toAudioUrl(url: string): string {
  if (!url) return url;
  if (/^https?:\/\/|^blob:/.test(url)) return url;

  let filePath = url;
  if (url.startsWith('file:///')) {
    filePath = decodeURIComponent(url.slice('file:///'.length));
  } else if (url.startsWith('file://')) {
    filePath = decodeURIComponent(url.slice('file://'.length));
  }
  const absPath = (filePath.startsWith('/') ? filePath : '/' + filePath).replace(/\\/g, '/');
  return convertFileSrc(absPath);
}

export function Player() {
  const {
    streamUrl, title, artist, thumbnail, mediaType,
    isPlaying, setPlaying, platform,
    queue, queueIndex, playNext, playPrev,
  } = usePlayerStore();

  const platformColor = platform ? PLATFORM_COLORS[platform] : undefined;
  const canPrev = queueIndex > 0;
  const canNext = queueIndex >= 0 && queueIndex < queue.length - 1;

  const mediaRef = useRef<HTMLVideoElement>(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  const [artHovered, setArtHovered] = useState(false);

  const isPlayingRef = useRef(isPlaying);
  isPlayingRef.current = isPlaying;

  const prevQueueKeyRef = useRef('');
  useEffect(() => {
    if (queueIndex < 0 || !queue[queueIndex]) return;
    const track = queue[queueIndex];
    const key = `${queueIndex}:${track.url}`;
    if (key === prevQueueKeyRef.current) return;
    prevQueueKeyRef.current = key;
    window.electron?.player.playMedia({
      url: track.url,
      platform: track.platform === 'youtubemusic' ? 'youtubeMusic' : (track.platform ?? ''),
    }).catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err ?? 'Unknown error');
      logError('playback', `Failed to play "${track.title}"`, msg);
    });
  }, [queueIndex, queue]);

  useEffect(() => {
    const cleanup = window.electron?.player?.onStreamReady?.((data) => {
      if (!data?.streamUrl) return;
      usePlayerStore.setState((s) => ({
        ...s,
        streamUrl: data.streamUrl,
        isPlaying: true,
        ...(data.mediaType ? { mediaType: data.mediaType } : {}),
      }));
      if (data.durationSec) setDuration(data.durationSec);
      logInfo('playback', 'Stream ready', `Now playing stream from ${data.platform || 'unknown'}`);
    });
    return () => cleanup?.();
  }, []);

  useEffect(() => {
    const el = mediaRef.current;
    if (!el || !streamUrl) return;

    const resolvedUrl = toAudioUrl(streamUrl);
    const onReady = () => {
      if (isPlayingRef.current) el.play().catch(() => {
        setPlaying(false);
        logWarning('playback', 'Autoplay blocked', 'Browser blocked autoplay — click play to start');
      });
    };

    el.addEventListener('canplay', onReady, { once: true });
    el.addEventListener('loadeddata', onReady, { once: true });
    el.src = resolvedUrl;
    el.load();
    if (isPlayingRef.current) el.play().catch(() => {
      logWarning('playback', 'Autoplay blocked', 'Browser blocked initial autoplay');
    });

    return () => {
      el.removeEventListener('canplay', onReady);
      el.removeEventListener('loadeddata', onReady);
    };
  }, [streamUrl]);

  useEffect(() => {
    const el = mediaRef.current;
    if (!el || !el.src) return;
    if (isPlaying) el.play().catch(() => {
      logWarning('playback', 'Play failed', 'Could not resume playback');
    });
    else el.pause();
  }, [isPlaying]);

  useEffect(() => {
    if (!('mediaSession' in navigator)) return;
    navigator.mediaSession.metadata = new MediaMetadata({
      title: title || 'Unknown',
      artist: artist || '',
      artwork: thumbnail ? [{ src: thumbnail, sizes: '512x512', type: 'image/jpeg' }] : [],
    });
  }, [title, artist, thumbnail]);

  useEffect(() => {
    if (!('mediaSession' in navigator)) return;
    navigator.mediaSession.playbackState = isPlaying ? 'playing' : 'paused';
    navigator.mediaSession.setActionHandler('play', () => setPlaying(true));
    navigator.mediaSession.setActionHandler('pause', () => setPlaying(false));
    navigator.mediaSession.setActionHandler('nexttrack', canNext ? playNext : null);
    navigator.mediaSession.setActionHandler('previoustrack', canPrev ? playPrev : null);
  }, [isPlaying, canNext, canPrev]);

  const handleTimeUpdate = () => {
    const el = mediaRef.current;
    if (!el) return;
    setCurrentTime(el.currentTime);
    if (el.duration && isFinite(el.duration)) setDuration(el.duration);
  };

  const handleSeek = (val: number[]) => {
    const el = mediaRef.current;
    if (!el) return;
    el.currentTime = val[0];
    setCurrentTime(val[0]);
  };

  const handleVolume = (val: number[]) => {
    const el = mediaRef.current;
    if (!el) return;
    const v = val[0];
    el.volume = v;
    setVolume(v);
    if (v > 0 && muted) { el.muted = false; setMuted(false); }
  };

  const toggleMute = () => {
    const el = mediaRef.current;
    if (!el) return;
    const next = !muted;
    el.muted = next;
    setMuted(next);
  };

  const handleEnded = () => {
    if (canNext) playNext();
    else setPlaying(false);
  };

  const handleFullscreen = () => {
    mediaRef.current?.requestFullscreen?.();
  };

  const fmt = (s: number) => {
    if (!isFinite(s) || isNaN(s)) return '0:00';
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${sec.toString().padStart(2, '0')}`;
  };

  const VolumeIcon = muted || volume === 0 ? VolumeX : volume < 0.5 ? Volume1 : Volume2;

  if (!streamUrl && !title) return null;

  return (
    <div
      className="h-[88px] border-t border-border bg-card shrink-0 flex items-center px-4 gap-4 relative"
      style={platformColor ? { borderTopColor: platformColor } : undefined}
    >

      <div className="flex items-center gap-3 w-[28%] min-w-0">
        <div
          className="relative shrink-0 rounded-md overflow-hidden bg-muted"
          style={{ width: 56, height: 56 }}
          onMouseEnter={() => setArtHovered(true)}
          onMouseLeave={() => setArtHovered(false)}
        >
          <video
            ref={mediaRef}
            className="w-full h-full object-cover"
            style={{ display: mediaType === 'video' ? 'block' : 'none' }}
            onTimeUpdate={handleTimeUpdate}
            onLoadedMetadata={handleTimeUpdate}
            onEnded={handleEnded}
            onError={(e) => {
              setPlaying(false);
              const el = e.target as HTMLVideoElement;
              const mediaErr = el.error;
              const srcUrl = el.src || streamUrl || '(none)';

              const emit = (netDetail?: string) => {
                const base = `MediaError ${mediaErr?.code ?? '?'}: ${mediaErr?.message || '(no message)'}`;
                logError('playback', netDetail ?? base, netDetail ? `${base}\n${netDetail}` : base);
              };

              if (srcUrl !== '(none)') {
                fetch(srcUrl)
                  .then((res) => res.text().then((body) => emit(res.ok ? undefined : `HTTP ${res.status}: ${body.trim()}`)))
                  .catch((err) => emit(`Net error: ${err.message}`));
              } else {
                emit();
              }
            }}
          />

          {mediaType === 'audio' && (

            thumbnail
              ? <img src={thumbnail} alt={title} className="absolute inset-0 w-full h-full object-cover" />
              : <div className="absolute inset-0 flex items-center justify-center">
                  <Music2 className="h-6 w-6 text-muted-foreground/30" />
                </div>
          )}

          {mediaType === 'video' && artHovered && (
            <button
              onClick={handleFullscreen}
              className="absolute inset-0 flex items-center justify-center bg-black/50 transition-opacity"
              title="Fullscreen"
            >
              <Maximize2 className="h-5 w-5 text-white drop-shadow" />
            </button>
          )}
        </div>

        <div className="min-w-0 flex-1">
          <p className="text-sm font-semibold truncate leading-snug">{title || '—'}</p>
          {artist && (
            <p className="text-xs text-muted-foreground truncate mt-0.5">{artist}</p>
          )}
          {queue.length > 1 && queueIndex >= 0 && (
            <p className="text-[10px] text-muted-foreground/50 mt-0.5 select-none">
              {queueIndex + 1} / {queue.length}
            </p>
          )}
        </div>
      </div>

      <div className="flex flex-col items-center justify-center gap-1.5 flex-1 min-w-0">
        <div className="flex items-center gap-0.5">
          <Button
            variant="ghost" size="icon"
            className="h-8 w-8 text-muted-foreground/40"
            disabled
            title="Shuffle (coming soon)"
          >
            <Shuffle className="h-3.5 w-3.5" />
          </Button>

          <Button
            variant="ghost" size="icon"
            className="h-8 w-8"
            onClick={playPrev}
            disabled={!canPrev}
            title="Previous"
          >
            <SkipBack className="h-4 w-4" />
          </Button>

          <button
            onClick={() => setPlaying(!isPlaying)}
            className="h-9 w-9 rounded-full bg-foreground text-background flex items-center justify-center hover:scale-105 active:scale-95 transition-transform shrink-0 mx-1"
          >
            {isPlaying
              ? <Pause className="h-[18px] w-[18px] fill-current" />
              : <Play  className="h-[18px] w-[18px] fill-current translate-x-px" />
            }
          </button>

          <Button
            variant="ghost" size="icon"
            className="h-8 w-8"
            onClick={playNext}
            disabled={!canNext}
            title="Next"
          >
            <SkipForward className="h-4 w-4" />
          </Button>

          <Button
            variant="ghost" size="icon"
            className="h-8 w-8 text-muted-foreground/40"
            disabled
            title="Repeat (coming soon)"
          >
            <Repeat className="h-3.5 w-3.5" />
          </Button>
        </div>

        <div className="flex items-center gap-2 w-full max-w-[480px]">
          <span className="text-[11px] text-muted-foreground tabular-nums w-8 text-right shrink-0 select-none">
            {fmt(currentTime)}
          </span>
          <Slider
            min={0}
            max={duration || 100}
            step={0.5}
            value={[currentTime]}
            onValueChange={handleSeek}
            className="flex-1"
            trackColor={platformColor}
          />
          <span className="text-[11px] text-muted-foreground tabular-nums w-8 shrink-0 select-none">
            {fmt(duration)}
          </span>
        </div>
      </div>

      <div className="flex items-center justify-end gap-2 w-[28%]">
        <Button
          variant="ghost" size="icon"
          className="h-8 w-8 shrink-0 text-muted-foreground hover:text-foreground"
          onClick={toggleMute}
        >
          <VolumeIcon className="h-4 w-4" />
        </Button>
        <Slider
          min={0}
          max={1}
          step={0.02}
          value={[muted ? 0 : volume]}
          onValueChange={handleVolume}
          className="w-24"
        />
      </div>

    </div>
  );
}

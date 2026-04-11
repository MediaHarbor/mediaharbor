import { useState, useRef, useEffect, useCallback } from 'react';
import {
  Play, Pause, SkipBack, SkipForward,
  Volume2, Volume1, VolumeX,
  Music2, Maximize2, Shuffle, Repeat, Repeat1, ListMusic,
} from 'lucide-react';
import { Slider } from '@/components/ui/slider';
import { Button } from '@/components/ui/button';
import { usePlayerStore } from '@/stores/usePlayerStore';
import { PLATFORM_COLORS } from '@/utils/platform-data';
import { logError, logWarning, logInfo } from '@/utils/logger';
import { crossfadeEngine } from '@/utils/AudioCrossfadeEngine';
import { convertFileSrc } from '@tauri-apps/api/core';

function toAudioUrl(url: string): string {
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
    streamUrl, title, artist, thumbnail, mediaType, isLive,
    isPlaying, setPlaying, platform,
    queue, queueIndex, playNext, playPrev,
    shuffle, toggleShuffle, shuffleHistory,
    repeat, cycleRepeat,
    nowPlayingOpen, toggleNowPlaying,
    setMediaElement,
  } = usePlayerStore();

  const platformColor = platform ? PLATFORM_COLORS[platform] : undefined;
  const canPrev = shuffle ? shuffleHistory.length > 1 : queueIndex > 0;
  const canNext = queue.length > 1;

  const mediaRef = useRef<HTMLVideoElement | null>(null);
const crossfadeAudioRef = useRef<HTMLAudioElement | null>(null);
  const crossfadeActiveRef = useRef(false);
  const crossfadeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const crossfadeHandoffTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const crossfadePendingRef = useRef(false);
  const idlePrefetchRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [engineReady, setEngineReady] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(1);
  const volumeRef = useRef(1);
  const [muted, setMuted] = useState(false);
  const [artHovered, setArtHovered] = useState(false);
  const [crossfadeEnabled, setCrossfadeEnabled] = useState(false);
  const [crossfadeDuration, setCrossfadeDuration] = useState(6);

  const setVideoRef = useCallback((node: HTMLVideoElement | null) => {
    mediaRef.current = node;
    setMediaElement(node);
    if (node) {
      const ok = crossfadeEngine.init(node);
      setEngineReady(ok);
      if (ok) {
        crossfadeEngine.setVolume(volumeRef.current);
      }
    }
  }, [setMediaElement]);

  const isPlayingRef = useRef(isPlaying);
  isPlayingRef.current = isPlaying;

  useEffect(() => {
    window.electron?.settings.get().then((data) => {
      if (data) {
        setCrossfadeEnabled(data.crossfade_enabled ?? false);
        setCrossfadeDuration(data.crossfade_duration ?? 6);
      }
    }).catch(() => {});
  }, []);

  const prevQueueKeyRef = useRef('');
  useEffect(() => {
    if (queueIndex < 0 || !queue[queueIndex]) return;
    const track = queue[queueIndex];
    const key = `${queueIndex}:${track.url}`;
    if (key === prevQueueKeyRef.current) return;
    prevQueueKeyRef.current = key;

    const cached = usePlayerStore.getState().getCachedStream(track.url);
    if (cached) {
      usePlayerStore.setState({
        streamUrl: cached.streamUrl,
        isPlaying: true,
        ...(cached.mediaType ? { mediaType: cached.mediaType as 'audio' | 'video' } : {}),
        isLive: cached.isLive ?? false,
      });
      logInfo('playback', 'Stream from cache', `Playing cached stream for "${track.title}"`);
      return;
    }

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
      usePlayerStore.setState({
        streamUrl: data.streamUrl,
        ...(data.mediaType ? { mediaType: data.mediaType } : {}),
        isLive: data.isLive ?? false,
      });
      if (data.durationSec) setDuration(data.durationSec);
      logInfo('playback', 'Stream ready', `Now playing stream from ${data.platform || 'unknown'}`);
    });
    return () => cleanup?.();
  }, []);

  useEffect(() => {
    const el = mediaRef.current;
    if (!el || !streamUrl) return;
    if (crossfadeActiveRef.current) return;

    crossfadeEngine.ensureResumed();
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
  }, [streamUrl, setPlaying]);

  useEffect(() => {
    if (crossfadeActiveRef.current) return;
    setCurrentTime(0);
    setDuration(0);
  }, [streamUrl]);

  useEffect(() => {
    const el = mediaRef.current;
    if (!el || !el.src) return;
    if (isPlaying) {
      crossfadeEngine.ensureResumed();
      el.play().catch(() => {
        logWarning('playback', 'Play failed', 'Could not resume playback');
      });
    } else {
      crossfadeEngine.suspend();
      el.pause();
    }
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
  }, [isPlaying, canNext, canPrev, playNext, playPrev, setPlaying]);

  const resetIdlePrefetch = useCallback(() => {
    if (idlePrefetchRef.current) {
      clearTimeout(idlePrefetchRef.current);
      idlePrefetchRef.current = null;
    }
  }, []);

  const scheduleIdlePrefetch = useCallback(() => {
    resetIdlePrefetch();
    idlePrefetchRef.current = setTimeout(() => {
      const state = usePlayerStore.getState();
      const nextIdx = state.queueIndex + 1;
      if (nextIdx >= state.queue.length) return;
      const nextTrack = state.queue[nextIdx];
      if (!nextTrack?.url) return;
      if (state.getCachedStream(nextTrack.url)) return;

      window.electron?.player.prefetchMedia({
        url: nextTrack.url,
        platform: nextTrack.platform === 'youtubemusic' ? 'youtubeMusic' : (nextTrack.platform ?? ''),
      }).then((data) => {
        if (data?.streamUrl) {
          const current = usePlayerStore.getState();
          if (current.queue[current.queueIndex + 1]?.url === nextTrack.url) {
            current.cacheStream(nextTrack.url, data.streamUrl, data.mediaType, data.isLive);
            logInfo('playback', 'Idle prefetch complete', `Cached stream for "${nextTrack.title}"`);
          }
        }
      }).catch(() => {});
    }, 15000);
  }, [resetIdlePrefetch]);

  useEffect(() => {
    if (queueIndex >= 0 && isPlaying) {
      scheduleIdlePrefetch();
    }
    return resetIdlePrefetch;
  }, [queueIndex, isPlaying, scheduleIdlePrefetch, resetIdlePrefetch]);

  const handleTimeUpdate = () => {
    const el = mediaRef.current;
    if (!el) return;
    setCurrentTime(el.currentTime);
    if (el.duration && isFinite(el.duration)) setDuration(el.duration);

    if (
      crossfadeEnabled &&
      !crossfadeActiveRef.current &&
      repeat !== 'one' &&
      el.duration &&
      isFinite(el.duration) &&
      el.duration > crossfadeDuration + 2 &&
      el.currentTime >= el.duration - crossfadeDuration
    ) {
      startCrossfade();
    }
  };

  const disposeBridge = useCallback(() => {
    if (crossfadeTimerRef.current) {
      clearTimeout(crossfadeTimerRef.current);
      crossfadeTimerRef.current = null;
    }
    if (crossfadeHandoffTimerRef.current) {
      clearTimeout(crossfadeHandoffTimerRef.current);
      crossfadeHandoffTimerRef.current = null;
    }
    if (engineReady) {
      crossfadeEngine.completeCrossfade();
    }
    const fadeAudio = crossfadeAudioRef.current;
    if (fadeAudio) {
      fadeAudio.pause();
      fadeAudio.removeAttribute('src');
      fadeAudio.load();
      crossfadeAudioRef.current = null;
    }
    crossfadeActiveRef.current = false;
    crossfadePendingRef.current = false;
  }, [engineReady]);

  const finishCrossfade = useCallback(() => {
    if (crossfadeTimerRef.current) {
      clearTimeout(crossfadeTimerRef.current);
      crossfadeTimerRef.current = null;
    }

    const fadeAudio = crossfadeAudioRef.current;
    const mainEl = mediaRef.current;
    if (mainEl) mainEl.pause();

    const state = usePlayerStore.getState();
    const nextIdx = state.queueIndex + 1;
    const nextTrack = state.queue[nextIdx];

    if (!nextTrack) {
      disposeBridge();
      usePlayerStore.setState({ isPlaying: false });
      return;
    }

    const cached = state.getCachedStream(nextTrack.url);
    prevQueueKeyRef.current = `${nextIdx}:${nextTrack.url}`;

    const { [nextTrack.url]: _, ...restCache } = state.streamCache;
    usePlayerStore.setState({
      queueIndex: nextIdx,
      streamCache: restCache,
      streamUrl: cached ? cached.streamUrl : null,
      title: nextTrack.title,
      artist: nextTrack.artist,
      thumbnail: nextTrack.thumbnail ?? null,
      mediaType: (nextTrack.mediaType ?? 'audio') as 'audio' | 'video',
      isPlaying: true,
      platform: nextTrack.platform ?? null,
      syncedLyrics: null,
      plainLyrics: null,
      wordSyncedLyrics: null,
      lyricsLoading: false,
    });

    if (!fadeAudio) {
      crossfadeActiveRef.current = false;
      return;
    }

    crossfadeHandoffTimerRef.current = setTimeout(() => {
      disposeBridge();
    }, 8000);

    const doHandoff = (url: string) => {
      const el = mediaRef.current;
      if (!el) { disposeBridge(); return; }

      crossfadeEngine.ensureResumed();
      const resolvedUrl = toAudioUrl(url);
      let done = false;
      const onReady = () => {
        if (done) return;
        done = true;
        el.removeEventListener('canplay', onReady);
        el.removeEventListener('loadeddata', onReady);
        const pos = crossfadeAudioRef.current?.currentTime ?? 0;
        if (pos > 0.5) el.currentTime = pos;
        el.play().then(() => {
          requestAnimationFrame(() => disposeBridge());
        }).catch(() => disposeBridge());
      };
      el.addEventListener('canplay', onReady);
      el.addEventListener('loadeddata', onReady);
      el.src = resolvedUrl;
      el.load();
    };

    if (cached) {
      doHandoff(cached.streamUrl);
    } else {
      window.electron?.player.playMedia({
        url: nextTrack.url,
        platform: nextTrack.platform === 'youtubemusic' ? 'youtubeMusic' : (nextTrack.platform ?? ''),
      }).catch(() => {});

      const unsub = usePlayerStore.subscribe((curr, prev) => {
        if (curr.streamUrl && curr.streamUrl !== prev.streamUrl) {
          unsub();
          doHandoff(curr.streamUrl);
        }
      });
    }
  }, [disposeBridge]);

  const startCrossfade = useCallback(() => {
    if (crossfadeActiveRef.current) return;

    const state = usePlayerStore.getState();
    const nextIdx = state.queueIndex + 1;
    const nextTrack = state.queue[nextIdx];
    if (!nextTrack) return;

    const cached = state.getCachedStream(nextTrack.url);
    if (!cached) {
      if (!crossfadePendingRef.current) {
        crossfadePendingRef.current = true;
        window.electron?.player.prefetchMedia({
          url: nextTrack.url,
          platform: nextTrack.platform === 'youtubemusic' ? 'youtubeMusic' : (nextTrack.platform ?? ''),
        }).then((data) => {
          if (data?.streamUrl) {
            usePlayerStore.getState().cacheStream(nextTrack.url, data.streamUrl, data.mediaType, data.isLive);
          }
        }).catch(() => {
          crossfadePendingRef.current = false;
        });
      }
      return;
    }

    crossfadePendingRef.current = false;
    crossfadeActiveRef.current = true;
    const fadeAudio = new Audio();
    fadeAudio.crossOrigin = 'anonymous';
    fadeAudio.src = toAudioUrl(cached.streamUrl);
    crossfadeAudioRef.current = fadeAudio;

    window.electron?.player.prefetchMedia({
      url: nextTrack.url,
      platform: nextTrack.platform === 'youtubemusic' ? 'youtubeMusic' : (nextTrack.platform ?? ''),
    }).then((data) => {
      if (data?.streamUrl) {
        usePlayerStore.getState().cacheStream(nextTrack.url, data.streamUrl, data.mediaType, data.isLive);
      }
    }).catch(() => {});

    if (engineReady) {
      fadeAudio.volume = 1;
      crossfadeEngine.prepareFade(fadeAudio);
    } else {
      fadeAudio.volume = 0;
    }

    fadeAudio.addEventListener('canplay', () => {
      fadeAudio.play().catch(() => {});
      if (engineReady) {
        crossfadeEngine.startCrossfade(crossfadeDuration);
        crossfadeTimerRef.current = setTimeout(() => {
          finishCrossfade();
        }, crossfadeDuration * 1000);
      }
    }, { once: true });

    fadeAudio.load();

    if (!engineReady) {
      const mainEl = mediaRef.current;
      if (!mainEl) return;
      const fadeDur = crossfadeDuration * 1000;
      const startTs = performance.now();
      const fadeStartVol = volumeRef.current;

      const tick = () => {
        const elapsed = performance.now() - startTs;
        const t = Math.min(1, elapsed / fadeDur);
        const eqOut = Math.cos(t * Math.PI * 0.5);
        const eqIn = Math.sin(t * Math.PI * 0.5);

        if (mainEl && !mainEl.ended) {
          mainEl.volume = Math.max(0, fadeStartVol * eqOut);
        }
        fadeAudio.volume = Math.min(fadeStartVol, fadeStartVol * eqIn);

        if (t < 1 && !mainEl.ended) {
          requestAnimationFrame(tick);
        } else {
          if (mainEl) mainEl.volume = volumeRef.current;
          finishCrossfade();
        }
      };
      requestAnimationFrame(tick);
    }
  }, [crossfadeDuration, engineReady, finishCrossfade]);

  useEffect(() => {
    return () => {
      if (!crossfadeActiveRef.current) {
        if (crossfadeTimerRef.current) {
          clearTimeout(crossfadeTimerRef.current);
          crossfadeTimerRef.current = null;
        }
        if (crossfadeAudioRef.current) {
          crossfadeAudioRef.current.pause();
          crossfadeAudioRef.current.removeAttribute('src');
          crossfadeAudioRef.current.load();
          crossfadeAudioRef.current = null;
        }
      }
    };
  }, [streamUrl]);

  const handleSeek = (val: number[]) => {
    const el = mediaRef.current;
    if (!el) return;
    el.currentTime = val[0];
    setCurrentTime(val[0]);
    scheduleIdlePrefetch();
  };

  const handleVolume = (val: number[]) => {
    const v = val[0];
    setVolume(v);
    volumeRef.current = v;
    if (engineReady) {
      crossfadeEngine.setVolume(v);
    } else {
      const el = mediaRef.current;
      if (el) el.volume = v;
    }
    if (v > 0 && muted) {
      setMuted(false);
      if (engineReady) {
        crossfadeEngine.setMuted(false);
      } else {
        const el = mediaRef.current;
        if (el) el.muted = false;
      }
    }
  };

  const toggleMute = () => {
    const next = !muted;
    setMuted(next);
    if (engineReady) {
      crossfadeEngine.setMuted(next);
    } else {
      const el = mediaRef.current;
      if (el) el.muted = next;
    }
  };

  const handleEnded = () => {
    if (crossfadeActiveRef.current) return;
    if (repeat === 'one') {
      const el = mediaRef.current;
      if (el) {
        el.currentTime = 0;
        el.play().catch(() => {});
      }
      return;
    }
    playNext();
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
  const RepeatIcon = repeat === 'one' ? Repeat1 : Repeat;
  const activeColor = platformColor || 'hsl(var(--primary))';

  if (!streamUrl && !title) return null;

  return (
    <div
      className="h-[88px] border-t border-border bg-card shrink-0 flex items-center px-4 gap-4 relative"
      style={platformColor ? { borderTopColor: platformColor } : undefined}
    >

      <div className="flex items-center gap-3 w-[28%] min-w-0">
        <div
          className="relative shrink-0 rounded-md overflow-hidden bg-muted cursor-pointer"
          style={{ width: 56, height: 56 }}
          onMouseEnter={() => setArtHovered(true)}
          onMouseLeave={() => setArtHovered(false)}
          onClick={mediaType === 'video' && artHovered ? handleFullscreen : toggleNowPlaying}
        >
          <video
            ref={setVideoRef}
            crossOrigin="anonymous"
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
          <p className="text-sm font-semibold truncate leading-snug">{title || '\u2014'}</p>
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
            className="h-8 w-8"
            onClick={toggleShuffle}
            title="Shuffle"
            style={shuffle ? { color: activeColor, backgroundColor: activeColor + '20' } : undefined}
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
            className="h-8 w-8"
            onClick={cycleRepeat}
            title={repeat === 'off' ? 'Repeat' : repeat === 'all' ? 'Repeat All' : 'Repeat One'}
            style={repeat !== 'off' ? { color: activeColor, backgroundColor: activeColor + '20' } : undefined}
          >
            <RepeatIcon className="h-3.5 w-3.5" />
          </Button>
        </div>

        {isLive ? (
          <div className="flex items-center gap-2 w-full max-w-[480px]">
            <span className="text-[10px] font-bold tracking-widest text-red-500 uppercase px-1.5 py-0.5 border border-red-500 rounded select-none">
              LIVE
            </span>
            <div className="flex-1 h-1 bg-muted-foreground/20 rounded-full" />
          </div>
        ) : (
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
        )}
      </div>

      <div className="flex items-center justify-end gap-2 w-[28%]">
        <Button
          variant="ghost" size="icon"
          className="h-8 w-8 shrink-0 text-muted-foreground hover:text-foreground"
          onClick={toggleNowPlaying}
          title="Now Playing"
          style={nowPlayingOpen ? { color: activeColor } : undefined}
        >
          <ListMusic className="h-4 w-4" />
        </Button>
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

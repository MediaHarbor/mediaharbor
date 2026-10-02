import { useState, useRef, useEffect, useCallback, useMemo } from 'react';
import {
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Volume2,
  Volume1,
  VolumeX,
  Music2,
  Maximize2,
  Shuffle,
  Repeat,
  Repeat1,
  ListMusic,
  Loader2,
} from 'lucide-react';
import { Slider } from '@/components/ui/slider';
import { Button } from '@/components/ui/button';
import { usePlayerStore, type PlayableTrack } from '@/stores/usePlayerStore';
import { resolveCoverUrl, trackIdFromUrl } from '@/features/library/api';
import {
  PLATFORM_COLORS,
  SERVICES,
  serviceIdFromUrl,
  toClientPlatform,
  toIpcPlatform,
} from '@/utils/platform-data';
import { logError, logWarning, logInfo } from '@/utils/logger';
import { useNotificationStore } from '@/stores/useNotificationStore';
import { openExternalUrl } from '@/features/library/actions/shared';
import { usePrefetchStream } from '@/features/player/hooks/usePrefetchStream';
import { PlaybackSeekBar } from '@/features/player/components/PlaybackSeekBar';
import { useAppSettings } from '@/hooks/useAppSettings';
import { toAudioUrl } from '@/utils/mediaUrl';
import { TrackContextMenu } from '@/features/library/actions/RowContextMenu';
import { normalizePlatform } from '@/utils/platform-data';
import type { ReactNode } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { tauriAPI } from '@/tauri-bridge';
import { errorDetail, errorMessage } from '@/utils/errors';
import { copyText } from '@/utils/clipboard';
import { buildUndecodableReport } from '@/features/player/undecodableReport';

const PREFETCH_LEAD_SECS = 30;

function NowPlayingBarMenu({ children }: { children: ReactNode }) {
  const queue = usePlayerStore((s) => s.queue);
  const queueIndex = usePlayerStore((s) => s.queueIndex);
  const playFromQueue = usePlayerStore((s) => s.playFromQueue);
  const track = queueIndex >= 0 ? queue[queueIndex] : undefined;
  if (!track) return <>{children}</>;
  return (
    <TrackContextMenu
      platform={normalizePlatform(track.platform ?? '')}
      context="now-playing"
      queueIndex={queueIndex}
      onPlay={() => playFromQueue(queueIndex)}
      track={{
        id: trackIdFromUrl(track.url),
        title: track.title ?? null,
        artist: track.artist ?? null,
        album: track.album ?? null,
        url: track.url,
        thumbnail: track.thumbnail ?? null,
        albumId: track.albumId ?? null,
        artistId: track.artistId ?? null,
      }}
    >
      {children}
    </TrackContextMenu>
  );
}

export function Player() {
  const {
    streamUrl,
    audioStreamUrl,
    title,
    artist,
    album,
    thumbnail,
    mediaType,
    isLive,
    liveTitle,
    isPlaying,
    isBuffering,
    setPlaying,
    platform,
    queue,
    queueIndex,
    queueContextUri,
    playNext,
    peekNext,
    playPrev,
    shuffle,
    toggleShuffle,
    shuffleHistory,
    repeat,
    cycleRepeat,
    nowPlayingOpen,
    toggleNowPlaying,
    setMediaElement,
  } = usePlayerStore(
    useShallow((s) => ({
      streamUrl: s.streamUrl,
      audioStreamUrl: s.audioStreamUrl,
      title: s.title,
      artist: s.artist,
      album: s.album,
      thumbnail: s.thumbnail,
      mediaType: s.mediaType,
      isLive: s.isLive,
      liveTitle: s.liveTitle,
      isPlaying: s.isPlaying,
      isBuffering: s.isBuffering,
      setPlaying: s.setPlaying,
      platform: s.platform,
      queue: s.queue,
      queueIndex: s.queueIndex,
      queueContextUri: s.queueContextUri,
      playNext: s.playNext,
      peekNext: s.peekNext,
      playPrev: s.playPrev,
      shuffle: s.shuffle,
      toggleShuffle: s.toggleShuffle,
      shuffleHistory: s.shuffleHistory,
      repeat: s.repeat,
      cycleRepeat: s.cycleRepeat,
      nowPlayingOpen: s.nowPlayingOpen,
      toggleNowPlaying: s.toggleNowPlaying,
      setMediaElement: s.setMediaElement,
    }))
  );

  const platformColor = platform ? PLATFORM_COLORS[platform] : undefined;
  const canPrev = shuffle ? shuffleHistory.length > 1 : queueIndex > 0;
  const canNext = queue.length > 1;

  const mediaRef = useRef<HTMLVideoElement | null>(null);
  const prefetchStream = usePrefetchStream();
  const idlePrefetchRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState(1);
  const volumeRef = useRef(1);
  const [muted, setMuted] = useState(false);
  const [artHovered, setArtHovered] = useState(false);
  const { data: appSettings } = useAppSettings();
  const crossfadeEnabled = appSettings?.crossfade_enabled ?? false;
  const crossfadeDuration = appSettings?.crossfade_duration ?? 6;

  // The element draws frames and nothing else — it is muted and the native
  // player owns the audio, so wiring it into Web Audio would open a second
  // output stream for a graph that never carries a sample.
  const setVideoRef = useCallback(
    (node: HTMLVideoElement | null) => {
      mediaRef.current = node;
      setMediaElement(node);
    },
    [setMediaElement]
  );

  const isPlayingRef = useRef(isPlaying);
  useEffect(() => {
    isPlayingRef.current = isPlaying;
  }, [isPlaying]);

  const mediaTypeRef = useRef(mediaType);
  useEffect(() => {
    mediaTypeRef.current = mediaType;
  }, [mediaType]);

  const handleEndedRef = useRef<() => void>(() => {});
  const maybeStartNativeCrossfadeRef = useRef<(pos: number, dur?: number) => void>(() => {});
  /** True from a track ending until the next one is loaded, so late state
   *  events from the outgoing track cannot pause the incoming one. */
  const advancingRef = useRef(false);
  /** Track URL a fade has already been started for, so it fires once. */
  const fadedFromRef = useRef<string | null>(null);
  /** Queue move handed to the decode thread but not yet performed by it. */
  const pendingSwapRef = useRef<{ index: number; url: string } | null>(null);
  /** Which queued track each URL handed to the native player belongs to, so a
   *  decode failure on the incoming side of a crossfade names that track rather
   *  than the one still playing. Only the last few are kept. */
  const tracksByAudioUrlRef = useRef(new Map<string, PlayableTrack>());
  const rememberTrack = useCallback((audioUrl: string, track: PlayableTrack | undefined) => {
    if (!track) return;
    const known = tracksByAudioUrlRef.current;
    known.delete(audioUrl);
    known.set(audioUrl, track);
    for (const stale of [...known.keys()].slice(0, -4)) known.delete(stale);
  }, []);

  const prevQueueKeyRef = useRef('');
  const loadTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const loadFailuresRef = useRef(0);
  useEffect(() => {
    if (queueIndex < 0 || !queue[queueIndex]) return;
    const track = queue[queueIndex];
    const key = `${queueIndex}:${track.url}`;
    if (key === prevQueueKeyRef.current) return;
    prevQueueKeyRef.current = key;

    if (loadTimerRef.current) {
      clearTimeout(loadTimerRef.current);
      loadTimerRef.current = null;
    }

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

    loadTimerRef.current = setTimeout(() => {
      loadTimerRef.current = null;
      const st = usePlayerStore.getState();
      if (st.queueIndex !== queueIndex || st.queue[st.queueIndex]?.url !== track.url) return;
      tauriAPI.player
        .playMedia({
          url: track.url,
          platform: toIpcPlatform(track.platform ?? ''),
        })
        .catch((err: unknown) => {
          const msg = errorMessage(err);
          logError('playback', `Failed to play "${track.title}"`, msg);
          const st = usePlayerStore.getState();
          if (st.queueIndex !== queueIndex || st.queue[st.queueIndex]?.url !== track.url) return;
          loadFailuresRef.current += 1;
          if (loadFailuresRef.current < st.queue.length && st.queue.length > 1) {
            playNext();
          } else {
            setPlaying(false);
          }
        });
    }, 500);
  }, [queueIndex, queue, playNext, setPlaying]);

  const currentCoverId = queue[queueIndex]?.coverId ?? null;
  useEffect(() => {
    if (thumbnail || !currentCoverId) return;
    let cancelled = false;
    void resolveCoverUrl(currentCoverId).then((url) => {
      if (cancelled || !url) return;
      usePlayerStore.setState((s) =>
        s.queue[s.queueIndex]?.coverId === currentCoverId ? { thumbnail: url } : {}
      );
    });
    return () => {
      cancelled = true;
    };
  }, [thumbnail, currentCoverId]);

  useEffect(() => {
    const cleanup = tauriAPI.radio?.onMetadata?.((data) => {
      usePlayerStore.setState({ liveTitle: data.title });
    });
    return () => cleanup?.();
  }, []);

  useEffect(() => {
    const cleanup = tauriAPI.player?.onStreamReady?.((data) => {
      if (!data?.streamUrl) return;
      loadFailuresRef.current = 0;
      usePlayerStore.setState({
        streamUrl: data.streamUrl,
        audioStreamUrl: data.audioStreamUrl ?? null,
        ...(data.mediaType ? { mediaType: data.mediaType } : {}),
        isLive: data.isLive ?? false,
        liveTitle: null,
      });
      if (data.durationSec) setDuration(data.durationSec);
      logInfo('playback', 'Stream ready', `Now playing stream from ${data.platform || 'unknown'}`);
    });
    return () => cleanup?.();
  }, []);

  // Audio goes through the native player for *every* media type. For video the
  // element is muted and only draws frames, so its audio arrives here as a
  // separate stream — a progressive stream is consumed once, which is why the
  // backend serves picture and sound as two URLs rather than muxing them.
  useEffect(() => {
    const audioUrl = mediaType === 'video' ? audioStreamUrl : streamUrl;
    if (!audioUrl) return;
    // read at load time, not at render time: the queue index and the stream URL land
    // in separate renders, so the closure's copy can be a track behind
    const currentTrackForGain = () => {
      const s = usePlayerStore.getState();
      return s.queueIndex >= 0 ? s.queue[s.queueIndex] : undefined;
    };

    const url = toAudioUrl(audioUrl);
    const track = currentTrackForGain();
    rememberTrack(url, track);
    void tauriAPI.player.native
      .load(url, null, track?.url ?? null)
      .then(() => {
        advancingRef.current = false;
        if (!isPlayingRef.current) return tauriAPI.player.native.pause();
      })
      .catch((e: unknown) => {
        advancingRef.current = false;
        setPlaying(false);
        logError('playback', 'Playback failed', errorDetail(e));
      });

    return () => {
      void tauriAPI.player.native.stop().catch(() => {});
    };
  }, [streamUrl, audioStreamUrl, mediaType, setPlaying, rememberTrack]);

  useEffect(() => {
    const el = mediaRef.current;
    if (!el || !streamUrl || mediaType !== 'video') return;

    advancingRef.current = false;
    const resolvedUrl = toAudioUrl(streamUrl);

    // The element is muted: it draws frames and nothing else, so a blocked
    // autoplay costs a still picture, never silence.
    const onReady = () => {
      if (isPlayingRef.current) el.play().catch(() => {});
    };

    el.addEventListener('canplay', onReady, { once: true });
    el.addEventListener('loadeddata', onReady, { once: true });
    el.src = resolvedUrl;
    el.load();
    if (isPlayingRef.current) el.play().catch(() => {});

    return () => {
      el.removeEventListener('canplay', onReady);
      el.removeEventListener('loadeddata', onReady);
    };
  }, [streamUrl, mediaType, setPlaying]);

  useEffect(() => {
    setDuration(0);
  }, [streamUrl]);

  useEffect(() => {
    const native = tauriAPI.player.native;
    if (!native) return;

    const offPosition = native.onPosition(({ positionSecs, durationSecs }) => {
      // The picture follows the audio clock, never the other way round. The
      // decoder counts frames actually written to the device, so it is the only
      // honest position; the element is nudged rather than seeked, because a
      // `currentTime` write shows as a visible hitch while a small rate change
      // does not.
      if (mediaTypeRef.current === 'video') {
        const el = mediaRef.current;
        if (el && !el.paused && el.readyState >= 2) {
          const drift = el.currentTime - positionSecs;
          if (Math.abs(drift) > 1) {
            el.currentTime = positionSecs;
            el.playbackRate = 1;
          } else if (Math.abs(drift) > 0.04) {
            el.playbackRate = Math.min(1.02, Math.max(0.98, 1 - drift * 0.05));
          } else if (el.playbackRate !== 1) {
            el.playbackRate = 1;
          }
        }
      }
      if (durationSecs) setDuration(durationSecs);
      usePlayerStore.getState().setPosition(positionSecs, durationSecs);
      maybeScrobbleRef.current(positionSecs);
      maybeStartNativeCrossfadeRef.current(positionSecs, durationSecs);
    });

    const offState = native.onState(({ playing, ended, buffering }) => {
      if (ended) {
        usePlayerStore.getState().setBuffering(false);
        handleEndedRef.current();
        return;
      }
      usePlayerStore.getState().setBuffering(playing && buffering);
      if (!playing && isPlayingRef.current && !advancingRef.current) {
        setPlaying(false);
      }
    });

    const offError = native.onError(({ message, undecodable }) => {
      setPlaying(false);
      usePlayerStore.getState().setBuffering(false);
      logError('playback', 'Playback error', message);

      if (!undecodable) return;
      // The library deliberately keeps indexing formats with no decoder, and a
      // service stream the player rejects is a bug on our side — either way this
      // report is the only signal, so it stays up until dismissed. The toast
      // carries just the cause; the rest goes to the issue and the clipboard.
      const st = usePlayerStore.getState();
      const track =
        tracksByAudioUrlRef.current.get(undecodable.url) ??
        (st.queueIndex >= 0 ? st.queue[st.queueIndex] : undefined);
      const { text, issueUrl } = buildUndecodableReport(undecodable, track);
      useNotificationStore.getState().addNotification({
        type: 'error',
        title: track?.title ? `Can't play "${track.title}"` : "Can't play this track",
        message: undecodable.detail,
        dedupeKey: 'undecodable-track',
        duration: 0,
        cta: {
          label: 'Report',
          onClick: () => openExternalUrl(issueUrl),
        },
        secondaryCta: {
          label: 'Copy details',
          onClick: () => void copyText(text),
        },
      });
    });

    return () => {
      offPosition();
      offState();
      offError();
    };
  }, [setPlaying]);

  const currentTrackUrl = queue[queueIndex]?.url ?? null;
  const scrobbledRef = useRef(false);
  useEffect(() => {
    scrobbledRef.current = false;
  }, [currentTrackUrl]);

  // Audio is the native player's job for every media type; the element is only
  // kept in step so the picture starts and stops with the sound.
  useEffect(() => {
    const native = tauriAPI.player.native;
    void (isPlaying ? native?.play() : native?.pause())?.catch(() => {
      logWarning('playback', 'Play failed', 'Could not change playback state');
    });

    if (mediaType !== 'video') return;
    const el = mediaRef.current;
    if (!el || !el.src) return;
    if (isPlaying) {
      el.play().catch(() => {});
    } else {
      el.pause();
    }
  }, [isPlaying, mediaType]);

  const noMediaSession = useMemo(
    () => '__TAURI_INTERNALS__' in window || !('mediaSession' in navigator),
    []
  );

  useEffect(() => {
    if (noMediaSession) return;
    navigator.mediaSession.metadata = new MediaMetadata({
      title: title || 'Unknown',
      artist: artist || '',
      artwork: thumbnail ? [{ src: thumbnail, sizes: '512x512', type: 'image/jpeg' }] : [],
    });
  }, [noMediaSession, title, artist, thumbnail]);

  useEffect(() => {
    if (noMediaSession) return;
    navigator.mediaSession.playbackState = isPlaying ? 'playing' : 'paused';
  }, [noMediaSession, isPlaying]);

  useEffect(() => {
    if (noMediaSession) return;
    navigator.mediaSession.setActionHandler('play', () => setPlaying(true));
    navigator.mediaSession.setActionHandler('pause', () => setPlaying(false));
    navigator.mediaSession.setActionHandler('nexttrack', canNext ? playNext : null);
    navigator.mediaSession.setActionHandler('previoustrack', canPrev ? playPrev : null);
  }, [noMediaSession, canNext, canPrev, playNext, playPrev, setPlaying]);

  // A track change lands these fields over several renders — the store sets
  // title/artist first while `duration` is still the previous track's, the
  // stream-url effect then zeroes it, and `onStreamReady` and `onPosition`
  // each set it again. Pushing every render advertised three or four states
  // per track, some of them mismatched. Coalescing to the end of the burst
  // publishes only the settled one.
  useEffect(() => {
    const id = setTimeout(() => {
      tauriAPI.player.setMediaMetadata?.({
        title: title || 'Unknown',
        artist: artist || null,
        album: album || null,
        coverUrl: thumbnail || null,
        durationSecs: duration || null,
      });
    }, 150);
    return () => clearTimeout(id);
  }, [title, artist, album, thumbnail, duration]);

  // Depending on `currentTime` here meant one IPC call and one D-Bus
  // PropertiesChanged per position tick — ten a second, indefinitely — to
  // publish a value no MPRIS consumer samples anywhere near that fast. Pushed
  // on state change plus a 1 Hz heartbeat instead, reading the position at
  // send time so nothing is stale.
  useEffect(() => {
    const push = () =>
      tauriAPI.player.setMediaPlayback?.({
        playing: isPlaying,
        positionSecs: usePlayerStore.getState().position || 0,
      });
    push();
    if (!isPlaying) return;
    const id = setInterval(push, 1000);
    return () => clearInterval(id);
  }, [isPlaying]);

  useEffect(() => {
    const off = tauriAPI.player.onMediaControl?.((payload) => {
      switch (payload.action) {
        case 'play':
          setPlaying(true);
          break;
        case 'pause':
          setPlaying(false);
          break;
        case 'toggle':
          setPlaying(!usePlayerStore.getState().isPlaying);
          break;
        case 'next':
          playNext();
          break;
        case 'previous':
          playPrev();
          break;
        case 'stop':
          setPlaying(false);
          break;
        case 'set_position': {
          const el = mediaRef.current;
          if (el && typeof payload.seconds === 'number') el.currentTime = payload.seconds;
          break;
        }
        case 'seek_by':
        case 'seek_forward':
        case 'seek_backward': {
          const el = mediaRef.current;
          if (el) {
            const delta = payload.action === 'seek_backward' ? -10 : (payload.seconds ?? 10);
            el.currentTime = Math.max(0, el.currentTime + delta);
          }
          break;
        }
      }
    });
    return off;
  }, [playNext, playPrev, setPlaying]);

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
      if (state.shuffle) return;
      const nextIdx = state.queueIndex + 1;
      if (nextIdx >= state.queue.length) return;
      const nextTrack = state.queue[nextIdx];
      if (!nextTrack?.url) return;

      prefetchStream(nextTrack, {
        stillWanted: () => {
          const current = usePlayerStore.getState();
          return current.queue[current.queueIndex + 1]?.url === nextTrack.url;
        },
        onCached: () =>
          logInfo('playback', 'Idle prefetch complete', `Cached stream for "${nextTrack.title}"`),
      });
    }, 15000);
  }, [resetIdlePrefetch, prefetchStream]);

  useEffect(() => {
    if (queueIndex >= 0 && isPlaying) {
      scheduleIdlePrefetch();
    }
    return resetIdlePrefetch;
  }, [queueIndex, isPlaying, scheduleIdlePrefetch, resetIdlePrefetch]);

  /// Report a play once the listener is 30s in. Driven by the native player
  /// for audio and by the media element for video, so it fires on both paths.
  const maybeScrobble = useCallback(
    (position: number) => {
      if (scrobbledRef.current || position < 30 || !currentTrackUrl) return;

      const trackId = serviceIdFromUrl(currentTrackUrl)?.id;
      const client = platform ? toClientPlatform(platform) : null;

      if (platform === 'local' || (!platform && currentTrackUrl.startsWith('/'))) {
        scrobbledRef.current = true;
        void tauriAPI.library.recordPlay?.(currentTrackUrl).catch(() => {});
        return;
      }

      if (!trackId || !client || !SERVICES[client]?.reportsPlayback) {
        return;
      }

      scrobbledRef.current = true;
      const current = queueIndex >= 0 ? queue[queueIndex] : undefined;
      let context: { contextUri: string; trackIndex: number } | undefined;
      if (client === 'spotify') {
        if (current?.contextUri) {
          context = { contextUri: current.contextUri, trackIndex: current.trackIndex ?? 0 };
        } else if (queueContextUri) {
          context = { contextUri: queueContextUri, trackIndex: queueIndex >= 0 ? queueIndex : 0 };
        }
      }
      tauriAPI.serviceLibrary
        ?.reportPlayback?.(client, trackId, position, context)
        ?.catch(() => {});
    },
    [currentTrackUrl, platform, queue, queueContextUri, queueIndex]
  );

  const maybeScrobbleRef = useRef(maybeScrobble);
  useEffect(() => {
    maybeScrobbleRef.current = maybeScrobble;
  }, [maybeScrobble]);

  const handleTimeUpdate = () => {
    const el = mediaRef.current;
    if (!el) return;
    // Position comes from the native clock now, for video as well as audio.
    // Letting the element report it too would have the two fight each other.
    if (mediaTypeRef.current === 'video') return;
    const elDuration = el.duration && isFinite(el.duration) ? el.duration : 0;
    if (elDuration) {
      setDuration(Math.max(elDuration, el.currentTime));
    }
    usePlayerStore
      .getState()
      .setPosition(el.currentTime, elDuration ? Math.max(elDuration, el.currentTime) : undefined);

    maybeScrobble(el.currentTime);
  };

  const handleSeek = (val: number[]) => {
    // Optimistic: the store is what the seek bar reads, so move it before the
    // native player reports back.
    usePlayerStore.getState().setPosition(val[0]);
    if (mediaType === 'audio') {
      void tauriAPI.player.native.seek(val[0]).catch(() => {});
    } else {
      const el = mediaRef.current;
      if (el) el.currentTime = val[0];
    }
    scheduleIdlePrefetch();
  };

  // Volume and mute belong to the native player alone. The element stays muted
  // whatever happens here — it carries no audio.
  const handleVolume = (val: number[]) => {
    const v = val[0];
    setVolume(v);
    volumeRef.current = v;
    void tauriAPI.player.native.setVolume(v).catch(() => {});
    if (v > 0 && muted) {
      setMuted(false);
      void tauriAPI.player.native.setMuted(false).catch(() => {});
    }
  };

  const toggleMute = () => {
    const next = !muted;
    setMuted(next);
    void tauriAPI.player.native.setMuted(next).catch(() => {});
  };

  const handleEnded = () => {
    advancingRef.current = true;
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

  useEffect(() => {
    handleEndedRef.current = handleEnded;
  });

  useEffect(() => {
    maybeStartNativeCrossfadeRef.current = (position, durationSecs) => {
      if (!crossfadeEnabled || repeat === 'one') return;
      if (!durationSecs || !isFinite(durationSecs)) return;
      if (durationSecs <= crossfadeDuration + 2) return;

      const remaining = durationSecs - position;
      if (remaining > Math.max(crossfadeDuration, PREFETCH_LEAD_SECS)) return;

      const state = usePlayerStore.getState();
      const currentUrl = state.queue[state.queueIndex]?.url ?? null;
      if (!currentUrl || fadedFromRef.current === currentUrl) return;

      const nextIndex = peekNext().index;
      const nextTrack = nextIndex != null ? state.queue[nextIndex] : undefined;
      if (nextTrack == null || nextIndex == null) return;

      const cached = state.getCachedStream(nextTrack.url);
      if (!cached) {
        prefetchStream(nextTrack, {
          stillWanted: () => peekNext().index === nextIndex,
        });
        return;
      }
      // Video is deliberately left out: the stream cache holds the picture URL,
      // not the separate audio one a native crossfade would need, and the
      // picture would hard-cut mid-fade regardless. Spotify and Apple Music
      // also crossfade audio only.
      if ((cached.mediaType ?? 'audio') !== 'audio') return;
      if (remaining > crossfadeDuration) return;

      fadedFromRef.current = currentUrl;
      const fadeUrl = toAudioUrl(cached.streamUrl);
      rememberTrack(fadeUrl, nextTrack);
      void tauriAPI.player.native
        .crossfadeTo(fadeUrl, crossfadeDuration)
        // `crossfadeTo` only queues the command — the decode thread mixes for a
        // further `crossfadeDuration` before the handover happens. Advancing the
        // queue here made the UI and MPRIS advertise the next track against the
        // outgoing track's clock for that whole window. The move is recorded and
        // applied when Rust reports the swap.
        .then(() => {
          pendingSwapRef.current = { index: nextIndex, url: nextTrack.url };
        })
        .catch((e: unknown) => {
          fadedFromRef.current = null;
          logWarning('playback', 'Crossfade failed', errorDetail(e));
        });
    };
  }, [crossfadeEnabled, crossfadeDuration, repeat, peekNext, prefetchStream, rememberTrack]);

  useEffect(() => {
    fadedFromRef.current = null;
    // A fresh load supersedes any fade still in flight.
    pendingSwapRef.current = null;
  }, [streamUrl]);

  // The decode thread owns the moment a crossfade completes; this is where the
  // queue and the now-playing metadata catch up with it. Applied as one update
  // so the track change reaches MPRIS as a single consistent state.
  useEffect(() => {
    const off = tauriAPI.player.native?.onTrackChanged?.(() => {
      const swap = pendingSwapRef.current;
      if (!swap) return;
      pendingSwapRef.current = null;

      const state = usePlayerStore.getState();
      const t = state.queue[swap.index];
      if (!t || t.url !== swap.url) return;

      // Matches the key the load effect compares against, so the incoming track
      // is not re-fetched on top of the audio already playing.
      prevQueueKeyRef.current = `${swap.index}:${t.url}`;
      usePlayerStore.setState({
        queueIndex: swap.index,
        title: t.title,
        artist: t.artist,
        album: t.album ?? null,
        thumbnail: t.thumbnail ?? null,
        platform: t.platform ?? null,
      });
    });
    return () => off?.();
  }, []);

  const handleFullscreen = () => {
    mediaRef.current?.requestFullscreen?.();
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
      <NowPlayingBarMenu>
        <div className="flex items-center gap-3 w-[28%] min-w-0">
          <div
            className="relative shrink-0 rounded-md overflow-hidden bg-muted cursor-pointer"
            style={{ width: 56, height: 56 }}
            onMouseEnter={() => setArtHovered(true)}
            onMouseLeave={() => setArtHovered(false)}
            onClick={mediaType === 'video' && artHovered ? handleFullscreen : toggleNowPlaying}
          >
            {/* Mounted only for video. Keeping it mounted and merely hidden left
                its AudioContext — created in `setVideoRef` — open for the whole
                session, which PulseAudio lists as a second "mediaharbor" output
                stream even though no samples ever flow through it. Unmounting
                also gives each video session a fresh element, which matters:
                `createMediaElementSource` may be called only once per element
                for its lifetime, and an element already routed into Web Audio
                never returns to the default output. */}
            {mediaType === 'video' && (
              <video
                ref={setVideoRef}
                muted
                crossOrigin="anonymous"
                className="w-full h-full object-cover"
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
                    logError(
                      'playback',
                      netDetail ?? base,
                      netDetail ? `${base}\n${netDetail}` : base
                    );
                  };

                  if (srcUrl !== '(none)') {
                    fetch(srcUrl)
                      .then((res) =>
                        res
                          .text()
                          .then((body) =>
                            emit(res.ok ? undefined : `HTTP ${res.status}: ${body.trim()}`)
                          )
                      )
                      .catch((err) => emit(`Net error: ${err.message}`));
                  } else {
                    emit();
                  }
                }}
              />
            )}

            {mediaType === 'audio' &&
              (thumbnail ? (
                <img
                  src={thumbnail}
                  alt={title}
                  className="absolute inset-0 w-full h-full object-cover"
                />
              ) : (
                <div className="absolute inset-0 flex items-center justify-center">
                  <Music2 className="h-6 w-6 text-muted-foreground/30" />
                </div>
              ))}

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
            {artist && <p className="text-xs text-muted-foreground truncate mt-0.5">{artist}</p>}
            {queue.length > 1 && queueIndex >= 0 && (
              <p className="text-[10px] text-muted-foreground/50 mt-0.5 select-none">
                {queueIndex + 1} / {queue.length}
              </p>
            )}
          </div>
        </div>
      </NowPlayingBarMenu>

      <div className="flex flex-col items-center justify-center gap-1.5 flex-1 min-w-0">
        <div className="flex items-center gap-0.5">
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={toggleShuffle}
            title="Shuffle"
            style={
              shuffle ? { color: activeColor, backgroundColor: activeColor + '20' } : undefined
            }
          >
            <Shuffle className="h-3.5 w-3.5" />
          </Button>

          <Button
            variant="ghost"
            size="icon"
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
            {isBuffering ? (
              <Loader2 className="h-[18px] w-[18px] animate-spin" />
            ) : isPlaying ? (
              <Pause className="h-[18px] w-[18px] fill-current" />
            ) : (
              <Play className="h-[18px] w-[18px] fill-current translate-x-px" />
            )}
          </button>

          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={playNext}
            disabled={!canNext}
            title="Next"
          >
            <SkipForward className="h-4 w-4" />
          </Button>

          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={cycleRepeat}
            title={repeat === 'off' ? 'Repeat' : repeat === 'all' ? 'Repeat All' : 'Repeat One'}
            style={
              repeat !== 'off'
                ? { color: activeColor, backgroundColor: activeColor + '20' }
                : undefined
            }
          >
            <RepeatIcon className="h-3.5 w-3.5" />
          </Button>
        </div>

        {isLive ? (
          <div className="flex items-center gap-2 w-full max-w-[480px]">
            <span className="text-[10px] font-bold tracking-widest text-red-500 uppercase px-1.5 py-0.5 border border-red-500 rounded select-none">
              LIVE
            </span>
            {liveTitle ? (
              <span className="flex-1 truncate text-[11px] text-muted-foreground">{liveTitle}</span>
            ) : (
              <div className="flex-1 h-1 bg-muted-foreground/20 rounded-full" />
            )}
          </div>
        ) : (
          <PlaybackSeekBar duration={duration} onSeek={handleSeek} trackColor={platformColor} />
        )}
      </div>

      <div className="flex items-center justify-end gap-2 w-[28%]">
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8 shrink-0 text-muted-foreground hover:text-foreground"
          onClick={toggleNowPlaying}
          title="Now Playing"
          style={nowPlayingOpen ? { color: activeColor } : undefined}
        >
          <ListMusic className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
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

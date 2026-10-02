import { useRef, useEffect, useState, useCallback, memo, type ReactNode } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { motion, AnimatePresence } from 'framer-motion';
import {
  ChevronDown,
  Music2,
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Shuffle,
  Repeat,
  Repeat1,
  SlidersHorizontal,
  Disc,
  Circle,
  Image,
  Sparkles,
  Moon,
  Layers,
  EyeOff,
  Activity,
  BarChart2,
  AlignJustify,
  PanelRight,
  X,
  Film,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { TrackContextMenu, TrackKebab } from '@/features/library/actions/RowContextMenu';
import { trackIdFromUrl } from '@/features/library/api';
import { usePlayerStore, type NowPlayingView, type PlayableTrack } from '@/stores/usePlayerStore';
import { useVisibleCoverUrls } from '@/features/library/useCoverUrls';
import {
  PLATFORM_COLORS,
  PLATFORM_LABELS,
  serviceSourceOf,
  normalizePlatform,
  serviceIdFromUrl,
} from '@/utils/platform-data';
import { PlatformIcon } from '@/utils/platforms';
import { toAudioUrl } from '@/utils/mediaUrl';
import { formatDuration } from '@/utils/formatters';
import { LyricsView } from './LyricsView';
import { RadialVisualizer } from './RadialVisualizer';
import { BarVisualizer } from './BarVisualizer';
import { useShallow } from 'zustand/react/shallow';
import { tauriAPI } from '@/tauri-bridge';

function queueTrackRef(track: PlayableTrack) {
  return {
    id: trackIdFromUrl(track.url),
    title: track.title ?? null,
    artist: track.artist ?? null,
    album: track.album ?? null,
    url: track.url,
    thumbnail: track.thumbnail ?? null,
    albumId: track.albumId ?? null,
    artistId: track.artistId ?? null,
  };
}

function QueueItemMenu({
  index,
  track,
  children,
}: {
  index: number;
  track: PlayableTrack;
  children: ReactNode;
}) {
  const playFromQueue = usePlayerStore((s) => s.playFromQueue);
  return (
    <TrackContextMenu
      platform={normalizePlatform(track.platform ?? '')}
      context="queue"
      queueIndex={index}
      onPlay={() => playFromQueue(index)}
      track={queueTrackRef(track)}
    >
      {children}
    </TrackContextMenu>
  );
}

function CurrentTrackMenu({
  track,
  queueIndex,
  onPlay,
  children,
}: {
  track: PlayableTrack | undefined;
  queueIndex: number;
  onPlay: () => void;
  children: ReactNode;
}) {
  if (!track) return <>{children}</>;
  return (
    <TrackContextMenu
      platform={normalizePlatform(track.platform ?? '')}
      context="now-playing"
      queueIndex={queueIndex}
      onPlay={onPlay}
      track={queueTrackRef(track)}
    >
      {children}
    </TrackContextMenu>
  );
}

interface QueuePanelProps {
  queue: PlayableTrack[];
  queueIndex: number;
  accentColor: string;
  playFromQueue: (i: number) => void;
}

const QueueSidePanel = memo(function QueueSidePanel({
  queue,
  queueIndex,
  accentColor,
  playFromQueue,
}: QueuePanelProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: queue.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 68,
    overscan: 8,
  });
  const virtualItems = virtualizer.getVirtualItems();
  const covers = useVisibleCoverUrls(virtualItems.map((v) => queue[v.index]?.coverId));

  return (
    <div ref={parentRef} className="flex-1 overflow-y-auto scrollbar-none">
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
        {virtualItems.map((vi) => {
          const track = queue[vi.index];
          if (!track) return null;
          const i = vi.index;
          const isCurrent = i === queueIndex;
          const thumb = track.thumbnail ?? (track.coverId ? (covers[track.coverId] ?? null) : null);
          return (
            <div
              key={`${i}-${track.url}`}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                right: 0,
                height: vi.size,
                transform: `translateY(${vi.start}px)`,
              }}
            >
              <QueueItemMenu index={i} track={track}>
                <div className="group relative">
                  <button
                    onClick={() => playFromQueue(i)}
                    className={`w-full flex items-center gap-3 px-5 py-3 pr-12 text-left transition-colors ${
                      isCurrent ? 'bg-accent/50' : 'hover:bg-accent/30'
                    }`}
                    style={
                      isCurrent
                        ? { borderLeft: `3px solid ${accentColor}` }
                        : { borderLeft: '3px solid transparent' }
                    }
                  >
                    <div className="h-11 w-11 rounded-lg overflow-hidden bg-muted shrink-0">
                      {thumb ? (
                        <img
                          src={thumb}
                          alt=""
                          loading="lazy"
                          decoding="async"
                          className="h-full w-full object-cover"
                        />
                      ) : (
                        <div className="h-full w-full flex items-center justify-center">
                          <Music2 className="h-4 w-4 text-muted-foreground/30" />
                        </div>
                      )}
                    </div>
                    <div className="flex-1 min-w-0">
                      <p
                        className={`text-sm font-medium truncate ${isCurrent ? 'text-foreground' : 'text-foreground/80'}`}
                        style={isCurrent ? { color: accentColor } : undefined}
                      >
                        {track.title}
                      </p>
                      {track.artist && (
                        <p className="text-xs text-muted-foreground truncate mt-0.5">
                          {track.artist}
                        </p>
                      )}
                    </div>
                  </button>
                  <div
                    className="absolute right-3 top-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100 transition-opacity"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <TrackKebab
                      size="sm"
                      platform={normalizePlatform(track.platform ?? '')}
                      context="queue"
                      queueIndex={i}
                      onPlay={() => playFromQueue(i)}
                      track={queueTrackRef(track)}
                    />
                  </div>
                </div>
              </QueueItemMenu>
            </div>
          );
        })}
      </div>
    </div>
  );
});

const QueueStrip = memo(function QueueStrip({
  queue,
  queueIndex,
  accentColor,
  playFromQueue,
}: QueuePanelProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    horizontal: true,
    count: queue.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 60,
    overscan: 8,
  });
  const virtualItems = virtualizer.getVirtualItems();
  const covers = useVisibleCoverUrls(virtualItems.map((v) => queue[v.index]?.coverId));

  useEffect(() => {
    if (queueIndex >= 0) virtualizer.scrollToIndex(queueIndex, { align: 'center' });
  }, [queueIndex, virtualizer]);

  return (
    <div ref={parentRef} className="overflow-x-auto scrollbar-none pb-0.5 h-[72px]">
      <div style={{ width: virtualizer.getTotalSize(), height: '100%', position: 'relative' }}>
        {virtualItems.map((vi) => {
          const track = queue[vi.index];
          if (!track) return null;
          const i = vi.index;
          const isCurrent = i === queueIndex;
          const thumb = track.thumbnail ?? (track.coverId ? (covers[track.coverId] ?? null) : null);
          return (
            <div
              key={`${i}-${track.url}`}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                height: '100%',
                width: vi.size,
                transform: `translateX(${vi.start}px)`,
              }}
            >
              <QueueItemMenu index={i} track={track}>
                <button
                  onClick={() => playFromQueue(i)}
                  className={`shrink-0 flex flex-col items-center gap-1 w-[52px] transition-opacity duration-200 ${
                    isCurrent ? 'opacity-100' : 'opacity-35 hover:opacity-60'
                  }`}
                >
                  <div
                    className="w-[52px] h-[52px] rounded-xl overflow-hidden bg-muted relative"
                    style={
                      isCurrent
                        ? { outline: `2px solid ${accentColor}`, outlineOffset: '2px' }
                        : undefined
                    }
                  >
                    {thumb ? (
                      <img
                        src={thumb}
                        alt=""
                        loading="lazy"
                        decoding="async"
                        className="w-full h-full object-cover"
                      />
                    ) : (
                      <div className="w-full h-full flex items-center justify-center">
                        <Music2 className="h-3.5 w-3.5 text-muted-foreground/30" />
                      </div>
                    )}
                    {isCurrent && (
                      <div className="absolute inset-0 flex items-center justify-center bg-black/35">
                        <div className="flex items-end gap-[2px]">
                          <span className="block w-[2.5px] h-2.5 rounded-full animate-bounce [animation-delay:0ms] bg-white" />
                          <span className="block w-[2.5px] h-3.5 rounded-full animate-bounce [animation-delay:150ms] bg-white" />
                          <span className="block w-[2.5px] h-2 rounded-full animate-bounce [animation-delay:300ms] bg-white" />
                        </div>
                      </div>
                    )}
                  </div>
                  <p className="text-[9px] font-medium text-muted-foreground truncate w-full text-center leading-tight">
                    {track.title}
                  </p>
                </button>
              </QueueItemMenu>
            </div>
          );
        })}
      </div>
    </div>
  );
});

const PREFS_KEY = 'np-prefs-v1';

interface NowPlayingPrefs {
  discStyle: 'vinyl' | 'circle' | 'framed';
  background: 'bloom' | 'dark' | 'gradient';
  visualizer: 'off' | 'radial' | 'bars';
  queue: 'strip' | 'side' | 'hidden';
}

const DEFAULT_PREFS: NowPlayingPrefs = {
  discStyle: 'circle',
  background: 'dark',
  visualizer: 'radial',
  queue: 'side',
};

function useNowPlayingPrefs() {
  const [prefs, setPrefsState] = useState<NowPlayingPrefs>(() => {
    try {
      const raw = localStorage.getItem(PREFS_KEY);
      return raw
        ? { ...DEFAULT_PREFS, ...(JSON.parse(raw) as Partial<NowPlayingPrefs>) }
        : DEFAULT_PREFS;
    } catch {
      return DEFAULT_PREFS;
    }
  });

  const setPrefs = useCallback((updates: Partial<NowPlayingPrefs>) => {
    setPrefsState((p) => {
      const next = { ...p, ...updates };
      try {
        localStorage.setItem(PREFS_KEY, JSON.stringify(next));
      } catch {
        /* no-op */
      }
      return next;
    });
  }, []);

  return [prefs, setPrefs] as const;
}

interface IconOption<T extends string> {
  value: T;
  icon: LucideIcon;
  label: string;
}

function IconRow<T extends string>({
  label,
  options,
  value,
  onChange,
  accentColor,
}: {
  label: string;
  options: IconOption<T>[];
  value: T;
  onChange: (v: T) => void;
  accentColor: string;
}) {
  return (
    <div className="space-y-1.5">
      <p className="text-[9px] font-bold uppercase tracking-widest text-muted-foreground">
        {label}
      </p>
      <div className="flex gap-1">
        {options.map((opt) => {
          const active = value === opt.value;
          const Icon = opt.icon;
          return (
            <button
              key={opt.value}
              onClick={() => onChange(opt.value)}
              title={opt.label}
              className={`flex-1 flex flex-col items-center gap-1 py-2 rounded-lg transition-all ${
                active
                  ? ''
                  : 'text-muted-foreground hover:text-foreground bg-muted/50 hover:bg-muted'
              }`}
              style={
                active ? { backgroundColor: accentColor + '22', color: accentColor } : undefined
              }
            >
              <Icon className="h-3.5 w-3.5" />
              <span className="text-[9px] font-semibold leading-none">{opt.label}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function PersonalisePopover({
  prefs,
  setPrefs,
  accentColor,
  onClose,
}: {
  prefs: NowPlayingPrefs;
  setPrefs: (p: Partial<NowPlayingPrefs>) => void;
  accentColor: string;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [onClose]);

  return (
    <motion.div
      ref={ref}
      initial={{ opacity: 0, scale: 0.92, y: -6 }}
      animate={{ opacity: 1, scale: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.92, y: -6 }}
      transition={{ type: 'spring', damping: 28, stiffness: 400 }}
      className="absolute right-0 top-full mt-2 z-30 bg-popover border border-border shadow-2xl rounded-xl p-3 w-[240px]"
      style={{ transformOrigin: 'top right' }}
    >
      <div className="grid grid-cols-2 gap-3">
        <IconRow
          label="Disc"
          options={
            [
              { value: 'vinyl', icon: Disc, label: 'Vinyl' },
              { value: 'circle', icon: Circle, label: 'Circle' },
              { value: 'framed', icon: Image, label: 'Framed' },
            ] as IconOption<NowPlayingPrefs['discStyle']>[]
          }
          value={prefs.discStyle}
          onChange={(v) => setPrefs({ discStyle: v })}
          accentColor={accentColor}
        />
        <IconRow
          label="Background"
          options={
            [
              { value: 'bloom', icon: Sparkles, label: 'Bloom' },
              { value: 'dark', icon: Moon, label: 'Dark' },
              { value: 'gradient', icon: Layers, label: 'Grad' },
            ] as IconOption<NowPlayingPrefs['background']>[]
          }
          value={prefs.background}
          onChange={(v) => setPrefs({ background: v })}
          accentColor={accentColor}
        />
        <IconRow
          label="Visualizer"
          options={
            [
              { value: 'off', icon: EyeOff, label: 'Off' },
              { value: 'radial', icon: Activity, label: 'Radial' },
              { value: 'bars', icon: BarChart2, label: 'Bars' },
            ] as IconOption<NowPlayingPrefs['visualizer']>[]
          }
          value={prefs.visualizer}
          onChange={(v) => setPrefs({ visualizer: v })}
          accentColor={accentColor}
        />
        <IconRow
          label="Queue"
          options={
            [
              { value: 'strip', icon: AlignJustify, label: 'Strip' },
              { value: 'side', icon: PanelRight, label: 'Side' },
              { value: 'hidden', icon: X, label: 'Off' },
            ] as IconOption<NowPlayingPrefs['queue']>[]
          }
          value={prefs.queue}
          onChange={(v) => setPrefs({ queue: v })}
          accentColor={accentColor}
        />
      </div>
    </motion.div>
  );
}

const VIEW_TABS: { id: NowPlayingView; label: string }[] = [
  { id: 'artwork', label: 'Art' },
  { id: 'lyrics', label: 'Lyrics' },
];

export function NowPlaying() {
  const {
    title,
    artist,
    thumbnail,
    platform,
    queue,
    queueIndex,
    isPlaying,
    setPlaying,
    playNext,
    playPrev,
    shuffle,
    toggleShuffle,
    repeat,
    cycleRepeat,
    activeNowPlayingView,
    lyricsEnabled,
    toggleLyricsEnabled,
    toggleNowPlaying,
    playFromQueue,
    setActiveNowPlayingView,
  } = usePlayerStore(
    useShallow((s) => ({
      title: s.title,
      artist: s.artist,
      thumbnail: s.thumbnail,
      platform: s.platform,
      queue: s.queue,
      queueIndex: s.queueIndex,
      isPlaying: s.isPlaying,
      setPlaying: s.setPlaying,
      playNext: s.playNext,
      playPrev: s.playPrev,
      shuffle: s.shuffle,
      toggleShuffle: s.toggleShuffle,
      repeat: s.repeat,
      cycleRepeat: s.cycleRepeat,
      activeNowPlayingView: s.activeNowPlayingView,
      lyricsEnabled: s.lyricsEnabled,
      toggleLyricsEnabled: s.toggleLyricsEnabled,
      toggleNowPlaying: s.toggleNowPlaying,
      playFromQueue: s.playFromQueue,
      setActiveNowPlayingView: s.setActiveNowPlayingView,
    }))
  );

  const [prefs, setPrefs] = useNowPlayingPrefs();
  const [personaliseOpen, setPersonaliseOpen] = useState(false);

  const canPrev = shuffle ? usePlayerStore.getState().shuffleHistory.length > 1 : queueIndex > 0;
  const canNext = queue.length > 1;
  const RepeatIcon = repeat === 'one' ? Repeat1 : Repeat;

  const platformColor = platform ? PLATFORM_COLORS[platform] : undefined;
  const platformLabel = platform ? PLATFORM_LABELS[platform] : undefined;
  const accentColor = platformColor || 'hsl(var(--primary))';

  const [duration, setDuration] = useState(0);

  const currentTime = usePlayerStore((s) => s.position);
  const storeDuration = usePlayerStore((s) => s.duration);
  useEffect(() => {
    if (storeDuration) setDuration(storeDuration);
  }, [storeDuration]);

  const handleSeek = (e: React.MouseEvent<HTMLDivElement>) => {
    if (isFinite(duration) && duration > 0) {
      const rect = e.currentTarget.getBoundingClientRect();

      usePlayerStore.getState().seekTo(((e.clientX - rect.left) / rect.width) * duration);
    }
  };

  const progress = duration > 0 ? (currentTime / duration) * 100 : 0;
  const currentTrack = queue[queueIndex];
  const isYtMusic = serviceSourceOf(platform ?? currentTrack?.platform) === 'ytmusic';
  const currentVideoUrl =
    currentTrack?.videoUrl ?? (isYtMusic ? (currentTrack?.url ?? null) : null);
  const episodeId =
    serviceSourceOf(platform ?? currentTrack?.platform) === 'spotify'
      ? (serviceIdFromUrl(currentTrack?.url ?? '', 'episode')?.id ?? null)
      : null;
  const [transcript, setTranscript] = useState<{
    lines: { startMs: number; speaker?: string | null; text: string }[];
  } | null>(null);
  useEffect(() => {
    setTranscript(null);
    if (!episodeId) return;
    let cancelled = false;
    void tauriAPI.serviceLibrary
      ?.transcript?.('spotify', episodeId)
      .then((t) => {
        if (!cancelled && t && t.lines?.length) setTranscript(t);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [episodeId]);

  const spotifyTrackId =
    serviceSourceOf(platform ?? currentTrack?.platform) === 'spotify'
      ? (serviceIdFromUrl(currentTrack?.url ?? '', 'track')?.id ?? null)
      : null;
  const [canvasUrl, setCanvasUrl] = useState<string | null>(null);
  useEffect(() => {
    setCanvasUrl(null);
    if (!spotifyTrackId) return;
    let cancelled = false;
    void tauriAPI.serviceLibrary
      ?.canvas?.('spotify', spotifyTrackId)
      .then((url) => {
        if (!cancelled && url) setCanvasUrl(toAudioUrl(url));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [spotifyTrackId]);

  const isSpotify = serviceSourceOf(platform ?? currentTrack?.platform) === 'spotify';
  const hasVideo = (isYtMusic && !!currentVideoUrl) || isSpotify;
  const hasCanvas = !!canvasUrl;
  const hasDescription = !!transcript;
  const tabs = [
    ...VIEW_TABS,
    ...(hasVideo ? [{ id: 'video' as NowPlayingView, label: 'Video' }] : []),
    ...(hasCanvas ? [{ id: 'canvas' as NowPlayingView, label: 'Canvas' }] : []),
    ...(hasDescription ? [{ id: 'description' as NowPlayingView, label: 'Description' }] : []),
  ];

  useEffect(() => {
    if (!hasVideo && activeNowPlayingView === 'video') setActiveNowPlayingView('artwork');
    if (!hasCanvas && activeNowPlayingView === 'canvas') setActiveNowPlayingView('artwork');
    if (!hasDescription && activeNowPlayingView === 'description')
      setActiveNowPlayingView('artwork');
  }, [hasVideo, hasCanvas, hasDescription, activeNowPlayingView, setActiveNowPlayingView]);

  const showLyrics = activeNowPlayingView === 'lyrics';
  const showVideo = activeNowPlayingView === 'video';
  const showCanvas = activeNowPlayingView === 'canvas';
  const showDescription = activeNowPlayingView === 'description';

  const [videoStreamUrl, setVideoStreamUrl] = useState<string | null>(null);
  useEffect(() => {
    setVideoStreamUrl(null);
    if (!currentVideoUrl) return;
    let cancelled = false;
    void tauriAPI.player
      ?.prefetchMedia?.({ url: currentVideoUrl, platform: 'ytmusic' })
      .then((data) => {
        if (!cancelled && data?.streamUrl) setVideoStreamUrl(toAudioUrl(data.streamUrl));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [currentVideoUrl]);

  const discSize = 'min(40vh, 300px)';

  return (
    <motion.div
      initial={{ y: '100%' }}
      animate={{ y: 0 }}
      exit={{ y: '100%' }}
      transition={{ type: 'spring', damping: 30, stiffness: 300 }}
      className="fixed inset-0 z-50 flex flex-col overflow-hidden"
    >
      <div className="absolute inset-0 bg-background/90 dark:bg-background/95 backdrop-blur-3xl" />

      {showCanvas && canvasUrl && (
        <>
          <video
            key={`bg-${canvasUrl}`}
            src={canvasUrl}
            className="absolute inset-0 h-full w-full object-cover pointer-events-none"
            autoPlay
            muted
            loop
            playsInline
          />
          <div className="absolute inset-x-0 top-0 h-40 pointer-events-none bg-gradient-to-b from-black/70 to-transparent" />
          <div className="absolute inset-x-0 bottom-0 h-1/2 pointer-events-none bg-gradient-to-t from-black/90 via-black/60 to-transparent" />
        </>
      )}

      {prefs.background !== 'dark' && thumbnail && (
        <div
          className="absolute inset-0 opacity-[0.18] dark:opacity-[0.12] blur-[100px] scale-[1.8] pointer-events-none"
          style={{
            backgroundImage: `url(${thumbnail})`,
            backgroundSize: 'cover',
            backgroundPosition: 'center',
          }}
        />
      )}
      {prefs.background === 'gradient' && platformColor && (
        <div
          className="absolute inset-0 pointer-events-none"
          style={{
            background: `radial-gradient(ellipse 80% 60% at 50% -10%, ${platformColor}40, transparent 70%)`,
          }}
        />
      )}
      <div className="absolute inset-0 bg-gradient-to-t from-background/60 via-transparent to-transparent pointer-events-none" />

      {/* Header — z-20 so its stacking context sits above the body (z-10), keeping the popover clickable */}
      <div className="relative z-20 flex items-center justify-between px-5 pt-4 pb-1 shrink-0">
        <button
          onClick={toggleNowPlaying}
          className="p-2 -ml-2 rounded-xl text-muted-foreground hover:text-foreground transition-colors"
        >
          <ChevronDown className="h-5 w-5" />
        </button>

        <div className="flex items-center gap-2">
          <div className="relative">
            <button
              onClick={() => setPersonaliseOpen((v) => !v)}
              className="p-1.5 rounded-lg transition-colors text-muted-foreground hover:text-foreground"
              style={personaliseOpen ? { color: accentColor } : undefined}
            >
              <SlidersHorizontal className="h-4 w-4" />
            </button>
            <AnimatePresence>
              {personaliseOpen && (
                <PersonalisePopover
                  prefs={prefs}
                  setPrefs={setPrefs}
                  accentColor={accentColor}
                  onClose={() => setPersonaliseOpen(false)}
                />
              )}
            </AnimatePresence>
          </div>

          {platform && platformColor && (
            <div
              className="flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-semibold"
              style={{ backgroundColor: platformColor + '20', color: platformColor }}
            >
              <PlatformIcon platform={platform} size={13} />
              {platformLabel}
            </div>
          )}
        </div>
      </div>

      {/* Body */}
      <div className="relative z-10 flex flex-1 min-h-0 overflow-hidden">
        {/* Main column */}
        <div className="flex-1 flex flex-col items-center min-h-0 px-5 pb-4 gap-4 overflow-hidden">
          {/* Disc / Lyrics */}
          <div className="flex-1 flex items-center justify-center min-h-0 w-full">
            {showDescription && transcript ? (
              <div className="w-full h-full max-w-[640px] overflow-y-auto px-2 py-4">
                {transcript.lines.map((line, i) => {
                  const active =
                    currentTime * 1000 >= line.startMs &&
                    (i + 1 >= transcript.lines.length ||
                      currentTime * 1000 < transcript.lines[i + 1].startMs);
                  const prevSpeaker = i > 0 ? transcript.lines[i - 1].speaker : undefined;
                  return (
                    <div key={i}>
                      {line.speaker && line.speaker !== prevSpeaker && (
                        <p className="mt-3 text-xs font-semibold uppercase tracking-wide text-muted-foreground/50">
                          {line.speaker}
                        </p>
                      )}
                      <p
                        className={
                          active
                            ? 'text-base leading-relaxed text-foreground'
                            : 'text-base leading-relaxed text-muted-foreground/45'
                        }
                      >
                        {line.text}
                      </p>
                    </div>
                  );
                })}
              </div>
            ) : showVideo ? (
              <div className="w-full h-full flex items-center justify-center max-w-[720px]">
                {videoStreamUrl ? (
                  <video
                    key={videoStreamUrl}
                    src={videoStreamUrl}
                    className="w-full h-full object-contain rounded-xl"
                    autoPlay
                    muted
                    loop
                    playsInline
                  />
                ) : isSpotify ? (
                  <div className="flex flex-col items-center gap-2 text-center px-6">
                    <Film className="h-10 w-10 text-muted-foreground/30" />
                    <p className="text-sm text-muted-foreground/60">Music videos coming soon</p>
                  </div>
                ) : (
                  <div className="text-sm text-muted-foreground/50">Loading video…</div>
                )}
              </div>
            ) : !showLyrics ? (
              <div className="flex flex-col items-center gap-3">
                {/* ─ Disc container ─
                    The canvas (RadialVisualizer) fills the full container.
                    All disc elements are inset ~10% so the outer ring of the
                    canvas (where the frequency bars live) stays visible. */}
                <div
                  className="relative flex items-center justify-center"
                  style={{ width: discSize, height: discSize }}
                >
                  {/* Ambient glow behind disc */}
                  {thumbnail && (
                    <div
                      className="absolute -inset-6 blur-[44px] opacity-40 animate-ambient-pulse pointer-events-none"
                      style={{
                        backgroundImage: `url(${thumbnail})`,
                        backgroundSize: 'cover',
                        backgroundPosition: 'center',
                        borderRadius: prefs.discStyle === 'framed' ? '20px' : '9999px',
                      }}
                    />
                  )}

                  {/* Radial visualizer — canvas fills container, bars radiate from 50%→90% radius */}
                  {prefs.visualizer === 'radial' && (
                    <RadialVisualizer
                      platformColor={platformColor || '#888888'}
                      isPlaying={isPlaying}
                    />
                  )}

                  {/* Vinyl — outer dark ring at inset-[16%], artwork at inset-[26%]
                      Disc edge at ~68% halfWidth; visualizer bars start at 72%. */}
                  {prefs.discStyle === 'vinyl' && (
                    <>
                      <div className="absolute inset-[16%] rounded-full bg-zinc-900 dark:bg-zinc-950 shadow-2xl" />
                      <div className="absolute inset-[21%] rounded-full ring-1 ring-white/[0.04]" />
                      <div className="absolute inset-[26%] rounded-full ring-1 ring-white/[0.03]" />
                      <div className="absolute inset-[31%] rounded-full ring-1 ring-white/[0.03]" />
                      <div
                        className="absolute inset-[26%] rounded-full overflow-hidden animate-vinyl-spin"
                        style={{
                          animationPlayState: isPlaying ? 'running' : 'paused',
                          boxShadow: '0 4px 20px rgba(0,0,0,0.6)',
                        }}
                      >
                        {thumbnail ? (
                          <img src={thumbnail} alt={title} className="w-full h-full object-cover" />
                        ) : (
                          <div className="w-full h-full flex items-center justify-center bg-zinc-800">
                            <Music2 className="h-6 w-6 text-white/10" />
                          </div>
                        )}
                      </div>
                      <div className="absolute w-3 h-3 rounded-full bg-zinc-900 dark:bg-zinc-950 ring-1 ring-white/[0.08] z-10" />
                    </>
                  )}

                  {/* Circle — artwork at inset-[16%] */}
                  {prefs.discStyle === 'circle' && (
                    <div
                      className="absolute inset-[16%] rounded-full overflow-hidden animate-vinyl-spin"
                      style={{
                        animationPlayState: isPlaying ? 'running' : 'paused',
                        boxShadow: '0 0 0 1px rgba(255,255,255,0.08), 0 20px 60px rgba(0,0,0,0.5)',
                      }}
                    >
                      {thumbnail ? (
                        <img src={thumbnail} alt={title} className="w-full h-full object-cover" />
                      ) : (
                        <div className="w-full h-full flex items-center justify-center bg-muted">
                          <Music2 className="h-10 w-10 text-muted-foreground/20" />
                        </div>
                      )}
                    </div>
                  )}

                  {/* Framed — artwork at inset-[16%] */}
                  {prefs.discStyle === 'framed' && (
                    <>
                      {thumbnail && (
                        <div
                          className="absolute inset-[8%] rounded-2xl blur-2xl opacity-35 pointer-events-none"
                          style={{
                            backgroundImage: `url(${thumbnail})`,
                            backgroundSize: 'cover',
                            backgroundPosition: 'center',
                          }}
                        />
                      )}
                      <div
                        className="absolute inset-[16%] rounded-xl overflow-hidden"
                        style={{
                          boxShadow:
                            '0 0 0 1px rgba(255,255,255,0.08), 0 24px 80px rgba(0,0,0,0.6)',
                        }}
                      >
                        {thumbnail ? (
                          <img src={thumbnail} alt={title} className="w-full h-full object-cover" />
                        ) : (
                          <div className="w-full h-full flex items-center justify-center bg-muted">
                            <Music2 className="h-10 w-10 text-muted-foreground/20" />
                          </div>
                        )}
                      </div>
                    </>
                  )}
                </div>

                {/* Bar visualizer */}
                {prefs.visualizer === 'bars' && (
                  <div className="h-10 w-full" style={{ maxWidth: discSize }}>
                    <BarVisualizer
                      platformColor={platformColor || '#888888'}
                      isPlaying={isPlaying}
                    />
                  </div>
                )}
              </div>
            ) : (
              <div className="w-full h-full flex flex-col max-w-[520px]">
                <LyricsView platformColor={accentColor} syncedMode={lyricsEnabled} />
              </div>
            )}
          </div>

          {/* Track info */}
          <CurrentTrackMenu
            track={currentTrack}
            queueIndex={queueIndex}
            onPlay={() => playFromQueue(queueIndex)}
          >
            <div className="w-full max-w-[400px] shrink-0">
              <AnimatePresence mode="wait">
                <motion.div
                  key={title}
                  initial={{ opacity: 0, y: 5 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -5 }}
                  transition={{ duration: 0.2 }}
                  className="text-center w-full"
                >
                  <h1 className="text-[26px] font-black tracking-tight truncate leading-none text-foreground">
                    {title || '—'}
                  </h1>
                  {artist && (
                    <p className="text-sm font-medium text-muted-foreground truncate mt-1.5">
                      {artist}
                    </p>
                  )}
                </motion.div>
              </AnimatePresence>
            </div>
          </CurrentTrackMenu>

          {/* Seek bar */}
          <div className="w-full max-w-[400px] shrink-0">
            <div
              className="relative w-full h-[3px] rounded-full cursor-pointer bg-foreground/10 group/seek"
              onClick={handleSeek}
            >
              <div
                className="absolute inset-y-0 left-0 rounded-full"
                style={{ width: `${progress}%`, backgroundColor: accentColor }}
              />
              <div
                className="absolute top-1/2 h-4 w-4 rounded-full opacity-0 group-hover/seek:opacity-100 transition-opacity"
                style={{
                  left: `${progress}%`,
                  backgroundColor: accentColor,
                  transform: 'translate(-50%,-50%)',
                }}
              />
            </div>
            <div className="flex justify-between mt-1.5 text-[11px] font-medium tabular-nums text-muted-foreground/60">
              <span>{formatDuration(currentTime)}</span>
              <span>-{formatDuration(Math.max(0, duration - currentTime))}</span>
            </div>
          </div>

          {/* Controls */}
          <div className="flex items-center justify-center w-full max-w-[400px] shrink-0">
            <button
              onClick={toggleShuffle}
              className={`h-9 w-9 rounded-xl flex items-center justify-center transition-colors ${
                shuffle ? 'text-foreground' : 'text-muted-foreground/40 hover:text-muted-foreground'
              }`}
              style={shuffle ? { color: accentColor } : undefined}
            >
              <Shuffle className="h-4 w-4" />
            </button>

            <div className="flex items-center gap-2 mx-auto">
              <button
                onClick={playPrev}
                disabled={!canPrev}
                className="h-11 w-11 flex items-center justify-center text-foreground/70 hover:text-foreground transition-colors disabled:opacity-20"
              >
                <SkipBack className="h-6 w-6 fill-current" />
              </button>

              <button
                onClick={() => setPlaying(!isPlaying)}
                className="h-[60px] w-[60px] rounded-full flex items-center justify-center transition-transform hover:scale-105 active:scale-95 text-white"
                style={{ backgroundColor: accentColor, boxShadow: `0 8px 24px ${accentColor}50` }}
              >
                {isPlaying ? (
                  <Pause className="h-7 w-7 fill-current" />
                ) : (
                  <Play className="h-7 w-7 fill-current translate-x-[2px]" />
                )}
              </button>

              <button
                onClick={playNext}
                disabled={!canNext}
                className="h-11 w-11 flex items-center justify-center text-foreground/70 hover:text-foreground transition-colors disabled:opacity-20"
              >
                <SkipForward className="h-6 w-6 fill-current" />
              </button>
            </div>

            <button
              onClick={cycleRepeat}
              className={`h-9 w-9 rounded-xl flex items-center justify-center transition-colors ${
                repeat !== 'off'
                  ? 'text-foreground'
                  : 'text-muted-foreground/40 hover:text-muted-foreground'
              }`}
              style={repeat !== 'off' ? { color: accentColor } : undefined}
            >
              <RepeatIcon className="h-4 w-4" />
            </button>
          </div>

          {/* View tabs */}
          <div className="flex items-center gap-2 shrink-0">
            <div className="flex items-center bg-muted rounded-full p-[3px]">
              {tabs.map((tab) => {
                const isActive = activeNowPlayingView === tab.id;
                return (
                  <button
                    key={tab.id}
                    onClick={() => setActiveNowPlayingView(tab.id)}
                    className={`px-4 py-1.5 rounded-full text-xs font-semibold transition-all duration-200 ${
                      isActive
                        ? 'text-white shadow-sm'
                        : 'text-muted-foreground hover:text-foreground'
                    }`}
                    style={isActive ? { backgroundColor: accentColor } : undefined}
                  >
                    {tab.label}
                  </button>
                );
              })}
            </div>

            {activeNowPlayingView === 'lyrics' && (
              <button
                onClick={toggleLyricsEnabled}
                className={`px-3 py-1.5 rounded-full text-xs font-semibold transition-all duration-200 ${
                  lyricsEnabled ? 'text-white' : 'text-muted-foreground hover:text-foreground'
                }`}
                style={lyricsEnabled ? { backgroundColor: accentColor } : undefined}
              >
                {lyricsEnabled ? 'Synced' : 'Plain'}
              </button>
            )}
          </div>

          {/* Queue strip */}
          {prefs.queue === 'strip' && queue.length > 1 && (
            <div className="w-full shrink-0">
              <div className="flex items-center gap-2 mb-2">
                <span className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground">
                  Up Next
                </span>
                <div className="flex-1 h-px bg-border" />
                <span className="text-[10px] text-muted-foreground/50 tabular-nums">
                  {queue.length}
                </span>
              </div>
              <QueueStrip
                queue={queue}
                queueIndex={queueIndex}
                accentColor={accentColor}
                playFromQueue={playFromQueue}
              />
            </div>
          )}
        </div>

        {/* Side queue panel */}
        <AnimatePresence>
          {prefs.queue === 'side' && queue.length > 0 && (
            <motion.div
              initial={{ x: '100%' }}
              animate={{ x: 0 }}
              exit={{ x: '100%' }}
              transition={{ type: 'spring', damping: 28, stiffness: 280 }}
              className="w-[300px] shrink-0 flex flex-col border-l border-border overflow-hidden bg-background/95"
            >
              <div className="px-5 pt-5 pb-3 shrink-0 border-b border-border">
                <div className="flex items-center justify-between">
                  <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">
                    Up Next
                  </p>
                  <span className="text-xs text-muted-foreground/50 tabular-nums">
                    {queue.length} tracks
                  </span>
                </div>
              </div>
              <QueueSidePanel
                queue={queue}
                queueIndex={queueIndex}
                accentColor={accentColor}
                playFromQueue={playFromQueue}
              />
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </motion.div>
  );
}

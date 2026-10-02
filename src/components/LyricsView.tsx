import { useRef, useCallback, useState, useEffect } from 'react';
import { usePlayerStore, type SyncedLine, type WordSyncedLine } from '@/stores/usePlayerStore';
import { Loader2 } from 'lucide-react';
import { useMediaSyncLoop } from '@/hooks/useMediaSyncLoop';
import { useShallow } from 'zustand/react/shallow';

const seekTo = (time: number) => usePlayerStore.getState().seekTo(time);

/// The last line that has already started at `time`, or -1 before the first one.
function activeIndexAt<T>(lines: T[], time: number, startOf: (line: T) => number): number {
  for (let i = lines.length - 1; i >= 0; i--) {
    if (startOf(lines[i]) <= time) return i;
  }
  return -1;
}

/// Scrolls the line at `idx` to the middle of its container.
function scrollLineToCenter(container: HTMLElement, idx: number) {
  const el = container.children[idx] as HTMLElement | undefined;
  if (!el) return;
  const top = el.offsetTop - container.clientHeight / 2 + el.clientHeight / 2;
  container.scrollTo({ top, behavior: 'smooth' });
}

/// Whether the track is far enough from its end to be worth scrolling for.
function farFromEnd(): boolean {
  const { duration, position } = usePlayerStore.getState();
  return (duration || 0) - position > 1.5;
}

function NoLyrics() {
  return (
    <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground/40">
      <p className="text-sm">No lyrics available</p>
    </div>
  );
}

function WordSyncedDisplay({
  lines,
  platformColor,
}: {
  lines: WordSyncedLine[];
  platformColor: string;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const activeLineRef = useRef(-1);
  const litRef = useRef(-1);
  const paintedRef = useRef<{ idx: number; color: string; lines: WordSyncedLine[] } | null>(null);

  const tick = useCallback(
    (time: number) => {
      if (!containerRef.current) return;
      const container = containerRef.current;

      let currentLineIdx = activeIndexAt(lines, time, (l) => l.startTime);
      if (currentLineIdx >= 0) {
        const cur = lines[currentLineIdx];
        const next = lines[currentLineIdx + 1];
        const isGapMarker = cur.words.length === 0;
        const inGap =
          !isGapMarker && cur.endTime > 0 && time > cur.endTime && (!next || time < next.startTime);
        if (inGap) currentLineIdx = -1;
      }

      const lineEls = container.children;
      const painted = paintedRef.current;
      const repaintAll =
        painted === null ||
        painted.idx !== currentLineIdx ||
        painted.color !== platformColor ||
        painted.lines !== lines;

      const activeLine = currentLineIdx >= 0 ? lines[currentLineIdx] : undefined;
      const lit = activeLine ? activeLine.words.filter((w) => time >= w.start).length : -1;
      if (!repaintAll && lit === litRef.current) return;
      litRef.current = lit;

      for (let li = 0; li < lineEls.length; li++) {
        const lineEl = lineEls[li] as HTMLElement;
        const line = lines[li];
        const isActiveLine = li === currentLineIdx;
        if (!repaintAll && !isActiveLine) continue;
        if (line.words.length === 0) {
          lineEl.style.opacity = '0.4';
          continue;
        }
        const isPastLine = li < currentLineIdx;
        lineEl.style.opacity = isActiveLine ? '1' : isPastLine ? '0.3' : '0.2';

        const litThrough = isActiveLine ? lit : isPastLine ? line.words.length : 0;
        const wordEls = lineEl.children;
        for (let wi = 0; wi < wordEls.length; wi++) {
          if (!line.words[wi]) continue;
          (wordEls[wi] as HTMLElement).style.color = wi < litThrough ? platformColor : '';
        }
      }
      paintedRef.current = { idx: currentLineIdx, color: platformColor, lines };

      if (currentLineIdx !== activeLineRef.current && currentLineIdx >= 0 && farFromEnd()) {
        activeLineRef.current = currentLineIdx;
        scrollLineToCenter(container, currentLineIdx);
      }
    },
    [lines, platformColor]
  );

  useMediaSyncLoop(tick);

  return (
    <div ref={containerRef} className="flex-1 overflow-y-auto scrollbar-none px-6 py-12 space-y-3">
      {lines.map((line, i) => {
        const isGap = line.words.length === 0;
        return (
          <p
            key={i}
            onClick={() => seekTo(line.startTime)}
            className={
              isGap
                ? 'text-base leading-none cursor-pointer text-muted-foreground'
                : 'text-[1.7rem] font-bold leading-snug cursor-pointer'
            }
            style={{ opacity: isGap ? 0.4 : 0.2 }}
          >
            {line.words.length > 0 ? (
              line.words.map((word, wi) => (
                <span key={wi}>
                  {word.text}
                  {wi < line.words.length - 1 ? ' ' : ''}
                </span>
              ))
            ) : (
              <span>♪</span>
            )}
          </p>
        );
      })}
    </div>
  );
}

function SyncedLyricsDisplay({
  lines,
  platformColor,
}: {
  lines: SyncedLine[];
  platformColor: string;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [activeIdx, setActiveIdx] = useState(-1);
  const lastScrolledRef = useRef(-1);

  const tick = useCallback(
    (time: number) => setActiveIdx(activeIndexAt(lines, time, (l) => l.time)),
    [lines]
  );

  useMediaSyncLoop(tick);

  useEffect(() => {
    if (activeIdx < 0 || !containerRef.current) return;
    if (lastScrolledRef.current === activeIdx) return;
    if (!farFromEnd()) return;
    scrollLineToCenter(containerRef.current, activeIdx);
    lastScrolledRef.current = activeIdx;
  }, [activeIdx]);

  useEffect(() => {
    lastScrolledRef.current = -1;
  }, [lines]);

  return (
    <div ref={containerRef} className="flex-1 overflow-y-auto scrollbar-none px-6 py-12 space-y-3">
      {lines.map((line, i) => {
        const isActive = i === activeIdx;
        const isPast = i < activeIdx;
        const isGap = line.text === '\u266A';
        return (
          <p
            key={i}
            onClick={() => seekTo(line.time)}
            className={
              isGap
                ? 'text-base leading-none cursor-pointer transition-opacity text-muted-foreground'
                : 'text-[1.7rem] font-bold leading-snug cursor-pointer transition-opacity'
            }
            style={{
              opacity: isGap ? 0.4 : isActive ? 1 : isPast ? 0.35 : 0.2,
              color: !isGap && isActive ? platformColor : undefined,
            }}
          >
            {line.text || '\u00A0'}
          </p>
        );
      })}
    </div>
  );
}

function PlainLyricsDisplay({ text }: { text: string }) {
  return (
    <div className="flex-1 overflow-y-auto scrollbar-none px-6 py-12">
      {text.split('\n').map((line, i) => (
        <p key={i} className="text-lg font-medium leading-loose text-foreground/90">
          {line || '\u00A0'}
        </p>
      ))}
    </div>
  );
}

export function LyricsView({
  platformColor,
  syncedMode = true,
}: {
  platformColor: string;
  syncedMode?: boolean;
}) {
  const { syncedLyrics, plainLyrics, wordSyncedLyrics, lyricsLoading } = usePlayerStore(
    useShallow((s) => ({
      syncedLyrics: s.syncedLyrics,
      plainLyrics: s.plainLyrics,
      wordSyncedLyrics: s.wordSyncedLyrics,
      lyricsLoading: s.lyricsLoading,
    }))
  );

  if (lyricsLoading) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground/40" />
      </div>
    );
  }

  const allPlainText =
    plainLyrics ||
    (syncedLyrics ? syncedLyrics.map((l) => l.text).join('\n') : null) ||
    (wordSyncedLyrics
      ? wordSyncedLyrics.map((l) => l.words.map((w) => w.text).join(' ')).join('\n')
      : null);

  if (!syncedMode) {
    if (allPlainText) {
      return <PlainLyricsDisplay text={allPlainText} />;
    }
    return <NoLyrics />;
  }

  if (wordSyncedLyrics && wordSyncedLyrics.length > 0) {
    return <WordSyncedDisplay lines={wordSyncedLyrics} platformColor={platformColor} />;
  }

  if (syncedLyrics && syncedLyrics.length > 0) {
    return <SyncedLyricsDisplay lines={syncedLyrics} platformColor={platformColor} />;
  }

  if (plainLyrics) {
    return <PlainLyricsDisplay text={plainLyrics} />;
  }

  return <NoLyrics />;
}

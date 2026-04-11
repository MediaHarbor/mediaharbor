import { useRef, useCallback } from 'react';
import { usePlayerStore, type SyncedLine, type WordSyncedLine } from '@/stores/usePlayerStore';
import { Loader2 } from 'lucide-react';
import { useMediaSyncLoop } from '@/hooks/useMediaSyncLoop';

function WordSyncedDisplay({
  lines,
  platformColor,
  mediaElement,
}: {
  lines: WordSyncedLine[];
  platformColor: string;
  mediaElement: HTMLMediaElement | null;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const activeLineRef = useRef(-1);

  const tick = useCallback((time: number) => {
    if (!containerRef.current) return;
    const remaining = mediaElement ? (mediaElement.duration || 0) - time : 0;
    const container = containerRef.current;

    let currentLineIdx = -1;
    for (let i = lines.length - 1; i >= 0; i--) {
      if (lines[i].startTime <= time) {
        currentLineIdx = i;
        break;
      }
    }

    const lineEls = container.children;
    for (let li = 0; li < lineEls.length; li++) {
      const lineEl = lineEls[li] as HTMLElement;
      const line = lines[li];
      const isActiveLine = li === currentLineIdx;
      const isPastLine = li < currentLineIdx;

      if (isActiveLine) {
        lineEl.style.opacity = '1';
        const wordEls = lineEl.children;
        for (let wi = 0; wi < wordEls.length; wi++) {
          const wordEl = wordEls[wi] as HTMLElement;
          const word = line.words[wi];
          if (!word) continue;
          if (time >= word.end) {
            wordEl.style.backgroundImage = '';
            wordEl.style.webkitBackgroundClip = '';
            wordEl.style.webkitTextFillColor = '';
            wordEl.style.color = platformColor;
          } else if (time >= word.start) {
            const progress = (time - word.start) / (word.end - word.start);
            const pct = Math.min(100, Math.max(0, progress * 100));
            wordEl.style.backgroundImage = `linear-gradient(90deg, ${platformColor} ${pct}%, currentColor ${pct}%)`;
            wordEl.style.webkitBackgroundClip = 'text';
            wordEl.style.webkitTextFillColor = 'transparent';
            wordEl.style.color = '';
          } else {
            wordEl.style.backgroundImage = '';
            wordEl.style.webkitBackgroundClip = '';
            wordEl.style.webkitTextFillColor = '';
            wordEl.style.color = '';
          }
        }
      } else if (isPastLine) {
        lineEl.style.opacity = '0.3';
        const wordEls = lineEl.children;
        for (let wi = 0; wi < wordEls.length; wi++) {
          const wordEl = wordEls[wi] as HTMLElement;
          wordEl.style.backgroundImage = '';
          wordEl.style.webkitBackgroundClip = '';
          wordEl.style.webkitTextFillColor = '';
          wordEl.style.color = platformColor;
        }
      } else {
        lineEl.style.opacity = '0.2';
        const wordEls = lineEl.children;
        for (let wi = 0; wi < wordEls.length; wi++) {
          const wordEl = wordEls[wi] as HTMLElement;
          wordEl.style.backgroundImage = '';
          wordEl.style.webkitBackgroundClip = '';
          wordEl.style.webkitTextFillColor = '';
          wordEl.style.color = '';
        }
      }
    }

    if (currentLineIdx !== activeLineRef.current && currentLineIdx >= 0 && remaining > 1.5) {
      activeLineRef.current = currentLineIdx;
      if (currentLineIdx < lineEls.length) {
        const el = lineEls[currentLineIdx] as HTMLElement;
        const top = el.offsetTop - container.clientHeight / 2 + el.clientHeight / 2;
        container.scrollTo({ top, behavior: 'smooth' });
      }
    }
  }, [lines, platformColor, mediaElement]);

  useMediaSyncLoop(tick, mediaElement);

  const handleLineClick = (time: number) => {
    if (mediaElement) {
      mediaElement.currentTime = time;
    }
  };

  return (
    <div ref={containerRef} className="flex-1 overflow-y-auto scrollbar-none px-6 py-12 space-y-3">
      {lines.map((line, i) => (
        <p
          key={i}
          onClick={() => handleLineClick(line.startTime)}
          className="text-[1.7rem] font-bold leading-snug cursor-pointer"
          style={{ opacity: 0.2 }}
        >
          {line.words.map((word, wi) => (
            <span key={wi}>
              {word.text}{wi < line.words.length - 1 ? ' ' : ''}
            </span>
          ))}
        </p>
      ))}
    </div>
  );
}

function SyncedLyricsDisplay({
  lines,
  platformColor,
  mediaElement,
}: {
  lines: SyncedLine[];
  platformColor: string;
  mediaElement: HTMLMediaElement | null;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const activeIndexRef = useRef(-1);

  const tick = useCallback((time: number) => {
    if (!containerRef.current) return;
    const remaining = mediaElement ? (mediaElement.duration || 0) - time : 0;
    let idx = -1;
    for (let i = lines.length - 1; i >= 0; i--) {
      if (lines[i].time <= time) {
        idx = i;
        break;
      }
    }

    if (idx !== activeIndexRef.current) {
      activeIndexRef.current = idx;
      const container = containerRef.current;
      const children = container.children;
      for (let i = 0; i < children.length; i++) {
        const el = children[i] as HTMLElement;
        if (i === idx) {
          el.style.opacity = '1';
          el.style.color = platformColor;
          if (remaining > 1.5) {
            const top = el.offsetTop - container.clientHeight / 2 + el.clientHeight / 2;
            container.scrollTo({ top, behavior: 'smooth' });
          }
        } else {
          el.style.opacity = '0.2';
          el.style.color = '';
        }
      }
    }
  }, [lines, platformColor, mediaElement]);

  useMediaSyncLoop(tick, mediaElement);

  const handleLineClick = (time: number) => {
    if (mediaElement) {
      mediaElement.currentTime = time;
    }
  };

  return (
    <div ref={containerRef} className="flex-1 overflow-y-auto scrollbar-none px-6 py-12 space-y-3">
      {lines.map((line, i) => (
        <p
          key={i}
          onClick={() => handleLineClick(line.time)}
          className="text-[1.7rem] font-bold leading-snug cursor-pointer"
          style={{ opacity: 0.2 }}
        >
          {line.text || '\u00A0'}
        </p>
      ))}
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

export function LyricsView({ platformColor, syncedMode = true }: { platformColor: string; syncedMode?: boolean }) {
  const { syncedLyrics, plainLyrics, wordSyncedLyrics, lyricsLoading, mediaElement } = usePlayerStore();

  if (lyricsLoading) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground/40" />
      </div>
    );
  }

  const allPlainText = plainLyrics
    || (syncedLyrics ? syncedLyrics.map(l => l.text).join('\n') : null)
    || (wordSyncedLyrics ? wordSyncedLyrics.map(l => l.words.map(w => w.text).join(' ')).join('\n') : null);

  if (!syncedMode) {
    if (allPlainText) {
      return <PlainLyricsDisplay text={allPlainText} />;
    }
    return (
      <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground/40">
        <p className="text-sm">No lyrics available</p>
      </div>
    );
  }

  if (wordSyncedLyrics && wordSyncedLyrics.length > 0) {
    return (
      <WordSyncedDisplay
        lines={wordSyncedLyrics}
        platformColor={platformColor}
        mediaElement={mediaElement}
      />
    );
  }

  if (syncedLyrics && syncedLyrics.length > 0) {
    return (
      <SyncedLyricsDisplay
        lines={syncedLyrics}
        platformColor={platformColor}
        mediaElement={mediaElement}
      />
    );
  }

  if (plainLyrics) {
    return <PlainLyricsDisplay text={plainLyrics} />;
  }

  return (
    <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground/40">
      <p className="text-sm">No lyrics available</p>
    </div>
  );
}

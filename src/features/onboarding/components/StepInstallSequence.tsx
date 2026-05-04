import { useState, useEffect, useRef, useCallback } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { CircleCheckBig, CircleX, Loader2, ExternalLink, ArrowRight, SkipForward } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/utils/cn';
import { useInstallProgress } from '../hooks/useInstallProgress';

interface DepDef {
  id: string;
  label: string;
  why: string;
  manualUrl?: string;
}

const REQUIRED_DEPS: DepDef[] = [
  {
    id: 'python',
    label: 'Python',
    why: 'Powers yt-dlp and the Spotify / Apple Music download tools. Without it, most services won’t work.',
  },
  {
    id: 'ffmpeg',
    label: 'FFmpeg',
    why: 'Converts and remuxes audio after download — required for high-quality output.',
    manualUrl: 'https://ffmpeg.org/download.html',
  },
  {
    id: 'git',
    label: 'Git',
    why: 'Used to fetch and update OrpheusDL modules (SoundCloud, Beatport, etc).',
    manualUrl: 'https://git-scm.com/downloads',
  },
  {
    id: 'ytdlp',
    label: 'yt-dlp',
    why: 'Handles all YouTube and YouTube Music downloads.',
  },
];

const OPTIONAL_DEPS: DepDef[] = [
  {
    id: 'spotify',
    label: 'Spotify tools (votify)',
    why: 'Lossless Spotify downloads. Skip if you don’t plan to use Spotify.',
  },
  {
    id: 'apple',
    label: 'Apple Music tools (gamdl)',
    why: 'Lossless Apple Music downloads. Skip if you don’t plan to use Apple Music.',
  },
];

type CardStatus = 'idle' | 'installing' | 'done' | 'error' | 'skipped';

interface StepInstallSequenceProps {
  onInstallingChange?: (installing: boolean) => void;
  onAllDone?: () => void;
}

export function StepInstallSequence({ onInstallingChange, onAllDone }: StepInstallSequenceProps) {
  const [installed, setInstalled] = useState<Record<string, boolean>>({});
  const [statusMap, setStatusMap] = useState<Record<string, CardStatus>>({});
  const [activeIndex, setActiveIndex] = useState(0);
  const [phase, setPhase] = useState<'required' | 'optional' | 'done'>('required');
  const [installing, setInstalling] = useState<string | null>(null);
  const [checkedDeps, setCheckedDeps] = useState(false);
  const progressMap = useInstallProgress();
  const advanceTimer = useRef<number | null>(null);

  const queue = phase === 'required' ? REQUIRED_DEPS : OPTIONAL_DEPS;
  const current = queue[activeIndex];

  useEffect(() => {
    window.electron?.updates.checkDeps().then((r) => {
      setInstalled({
        python: r.python ?? false,
        ffmpeg: r.ffmpeg ?? false,
        git: r.git ?? false,
        ytdlp: r.ytdlp ?? false,
        spotify: r.spotify ?? false,
        apple: r.apple ?? false,
      });
      setCheckedDeps(true);
    });
  }, []);

  useEffect(() => {
    onInstallingChange?.(installing !== null);
  }, [installing, onInstallingChange]);

  const advance = useCallback(() => {
    if (activeIndex < queue.length - 1) {
      setActiveIndex((i) => i + 1);
    } else if (phase === 'required') {
      setPhase('optional');
      setActiveIndex(0);
    } else {
      setPhase('done');
      onAllDone?.();
    }
  }, [activeIndex, queue.length, phase, onAllDone]);

  useEffect(() => {
    if (!checkedDeps || installing || !current) return;
    if (installed[current.id] && statusMap[current.id] !== 'done') {
      const t = window.setTimeout(() => {
        setStatusMap((s) => ({ ...s, [current.id]: 'done' }));
        advance();
      }, 400);
      advanceTimer.current = t;
      return () => window.clearTimeout(t);
    }
  }, [checkedDeps, installed, current, installing, statusMap, advance]);

  const verifyInstalled = async (id: string): Promise<boolean> => {
    const r = await window.electron?.updates.checkDeps().catch(() => null);
    if (!r) return false;
    const map: Record<string, boolean> = {
      python: r.python ?? false,
      ffmpeg: r.ffmpeg ?? false,
      git: r.git ?? false,
      ytdlp: r.ytdlp ?? false,
      spotify: r.spotify ?? false,
      apple: r.apple ?? false,
    };
    setInstalled((s) => ({ ...s, ...map }));
    return map[id] ?? false;
  };

  const handleInstall = async (id: string) => {
    setInstalling(id);
    setStatusMap((s) => ({ ...s, [id]: 'installing' }));
    try {
      const result = await window.electron?.updates.installDep(id);
      const present = await verifyInstalled(id);
      if (result?.success || present) {
        setStatusMap((s) => ({ ...s, [id]: 'done' }));
        setInstalling(null);
        advanceTimer.current = window.setTimeout(advance, 600);
      } else {
        setStatusMap((s) => ({ ...s, [id]: 'error' }));
        setInstalling(null);
      }
    } catch {
      const present = await verifyInstalled(id);
      if (present) {
        setStatusMap((s) => ({ ...s, [id]: 'done' }));
        setInstalling(null);
        advanceTimer.current = window.setTimeout(advance, 600);
      } else {
        setStatusMap((s) => ({ ...s, [id]: 'error' }));
        setInstalling(null);
      }
    }
  };

  const handleSkip = () => {
    if (current) setStatusMap((s) => ({ ...s, [current.id]: 'skipped' }));
    advance();
  };

  if (phase === 'done' || !current) {
    return (
      <motion.div
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        className="flex flex-col items-center text-center gap-4 py-8"
      >
        <div className="rounded-full bg-green-500/10 p-3">
          <CircleCheckBig className="w-8 h-8 text-green-500" />
        </div>
        <div className="space-y-1">
          <h2 className="text-lg font-semibold">All set</h2>
          <p className="text-sm text-muted-foreground max-w-xs">
            Your tools are ready. We can move on to a couple of basic preferences.
          </p>
        </div>
        <Summary statusMap={statusMap} installed={installed} />
      </motion.div>
    );
  }

  const status = statusMap[current.id] ?? 'idle';
  const isInstalled = installed[current.id] ?? false;
  const progress = progressMap.get(current.id);
  const isOptional = phase === 'optional';
  const totalInPhase = queue.length;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <p className="text-[10px] uppercase tracking-wider font-medium text-muted-foreground">
            {isOptional ? 'Optional tools' : 'Required tools'} · {activeIndex + 1} of {totalInPhase}
          </p>
          <h2 className="text-base font-semibold">{current.label}</h2>
        </div>
        <PhaseProgress current={activeIndex} total={totalInPhase} />
      </div>

      <AnimatePresence mode="wait">
        <motion.div
          key={current.id}
          initial={{ opacity: 0, x: 20 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: -20 }}
          transition={{ duration: 0.2 }}
          className="rounded-xl border border-border bg-card/60 p-5 space-y-4 min-h-[180px]"
        >
          <p className="text-sm text-muted-foreground leading-relaxed">{current.why}</p>

          <div className="flex items-center gap-3">
            <StatusIcon status={status} isInstalled={isInstalled} />
            <p className="text-xs text-muted-foreground flex-1">
              {status === 'installing' && (progress?.status ?? 'Starting…')}
              {status === 'done' && (isInstalled ? 'Installed.' : 'Done.')}
              {status === 'error' && 'Install failed. You can retry or skip and install it later.'}
              {status === 'skipped' && 'Skipped.'}
              {status === 'idle' && (isInstalled ? 'Already installed.' : 'Not installed yet.')}
            </p>
          </div>

          <AnimatePresence>
            {status === 'installing' && (
              <motion.div
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                exit={{ opacity: 0, height: 0 }}
                className="overflow-hidden"
              >
                <div className="h-1.5 w-full rounded-full bg-muted overflow-hidden">
                  <motion.div
                    className="h-full rounded-full bg-primary"
                    initial={{ width: '0%' }}
                    animate={{ width: `${progress?.percent ?? 0}%` }}
                    transition={{ ease: 'easeOut', duration: 0.3 }}
                  />
                </div>
              </motion.div>
            )}
          </AnimatePresence>

          <div className="flex items-center justify-between gap-2 pt-1">
            <div className="flex items-center gap-2">
              {current.manualUrl && (
                <button
                  onClick={(e) => {
                    e.preventDefault();
                    window.electron?.updates.openRelease(current.manualUrl!);
                  }}
                  className="text-[11px] text-muted-foreground hover:text-foreground transition-colors inline-flex items-center gap-1"
                >
                  <ExternalLink className="w-3 h-3" />
                  Manual install
                </button>
              )}
            </div>
            <div className="flex items-center gap-2">
              {(status === 'idle' || status === 'error') && !isInstalled && (
                <Button variant="ghost" size="sm" className="h-7 px-2 gap-1" onClick={handleSkip}>
                  <SkipForward className="w-3.5 h-3.5" />
                  {isOptional ? 'No thanks' : 'Skip'}
                </Button>
              )}
              {!isInstalled && status !== 'done' && status !== 'skipped' && (
                <Button
                  size="sm"
                  className="h-7 gap-1"
                  onClick={() => handleInstall(current.id)}
                  disabled={installing !== null}
                >
                  {status === 'installing' ? (
                    <>
                      <Loader2 className="w-3.5 h-3.5 animate-spin" />
                      Installing…
                    </>
                  ) : status === 'error' ? (
                    'Retry'
                  ) : (
                    <>
                      Install {current.label}
                      <ArrowRight className="w-3.5 h-3.5" />
                    </>
                  )}
                </Button>
              )}
              {(status === 'done' || status === 'skipped') && (
                <Button size="sm" className="h-7 gap-1" onClick={advance}>
                  Continue
                  <ArrowRight className="w-3.5 h-3.5" />
                </Button>
              )}
            </div>
          </div>
        </motion.div>
      </AnimatePresence>
    </div>
  );
}

function StatusIcon({ status, isInstalled }: { status: CardStatus; isInstalled: boolean }) {
  if (status === 'installing') return <Loader2 className="w-4 h-4 animate-spin text-primary shrink-0" />;
  if (status === 'done' || isInstalled) return <CircleCheckBig className="w-4 h-4 text-green-500 shrink-0" />;
  if (status === 'error') return <CircleX className="w-4 h-4 text-destructive shrink-0" />;
  if (status === 'skipped') return <SkipForward className="w-4 h-4 text-muted-foreground shrink-0" />;
  return <CircleX className="w-4 h-4 text-muted-foreground/60 shrink-0" />;
}

function PhaseProgress({ current, total }: { current: number; total: number }) {
  return (
    <div className="flex items-center gap-1">
      {Array.from({ length: total }).map((_, i) => (
        <div
          key={i}
          className={cn(
            'h-1 w-4 rounded-full transition-colors',
            i < current ? 'bg-primary' : i === current ? 'bg-primary/60' : 'bg-muted'
          )}
        />
      ))}
    </div>
  );
}

function Summary({
  statusMap,
  installed,
}: {
  statusMap: Record<string, CardStatus>;
  installed: Record<string, boolean>;
}) {
  const all = [...REQUIRED_DEPS, ...OPTIONAL_DEPS];
  return (
    <div className="w-full grid grid-cols-2 gap-1.5 pt-2">
      {all.map((d) => {
        const ok = installed[d.id] || statusMap[d.id] === 'done';
        const skipped = statusMap[d.id] === 'skipped';
        return (
          <div
            key={d.id}
            className="flex items-center gap-1.5 rounded-md border border-border/60 bg-card/40 px-2 py-1"
          >
            {ok ? (
              <CircleCheckBig className="w-3 h-3 text-green-500 shrink-0" />
            ) : skipped ? (
              <SkipForward className="w-3 h-3 text-muted-foreground shrink-0" />
            ) : (
              <CircleX className="w-3 h-3 text-muted-foreground/60 shrink-0" />
            )}
            <span className="text-[11px] text-muted-foreground truncate">{d.label}</span>
          </div>
        );
      })}
    </div>
  );
}

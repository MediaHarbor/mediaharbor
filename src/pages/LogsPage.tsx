import { useState, useEffect, useRef } from 'react';
import { useLogStore, type LogEntry, type LogSource } from '@/stores/useLogStore';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Trash2, AlertTriangle, XCircle, Info, ChevronDown, ChevronRight } from 'lucide-react';
import { cn } from '@/utils/cn';
import { motion, AnimatePresence } from 'framer-motion';

const LEVEL_CONFIG = {
  info:    { icon: Info,           color: 'text-blue-500',   bg: 'bg-blue-500/10',  border: 'border-blue-500/30' },
  warning: { icon: AlertTriangle,  color: 'text-yellow-500', bg: 'bg-yellow-500/10', border: 'border-yellow-500/30' },
  error:   { icon: XCircle,        color: 'text-destructive',bg: 'bg-destructive/10',border: 'border-destructive/30' },
};

const SOURCE_CONFIG: Record<LogSource, { label: string; color: string }> = {
  download: { label: 'Download', color: 'text-emerald-500' },
  playback: { label: 'Playback', color: 'text-purple-500' },
  search:   { label: 'Search',   color: 'text-sky-500' },
  settings: { label: 'Settings', color: 'text-orange-500' },
  system:   { label: 'System',   color: 'text-slate-500' },
  install:  { label: 'Install',  color: 'text-amber-500' },
  app:          { label: 'App',          color: 'text-rose-500'   },
  mediascanner: { label: 'Media Scanner', color: 'text-violet-500' },
  filewatcher:  { label: 'File Watcher',  color: 'text-cyan-500'   },
  qobuz:        { label: 'Qobuz',         color: 'text-indigo-500' },
  deezer:       { label: 'Deezer',        color: 'text-pink-500'   },
  tidal:        { label: 'Tidal',         color: 'text-sky-400'    },
  gam:          { label: 'Streaming',     color: 'text-green-400'  },
};

const SOURCE_FILTERS: ('all' | LogSource)[] = [
  'all', 'download', 'playback', 'search', 'install', 'settings', 'system', 'app',
  'mediascanner', 'filewatcher', 'qobuz', 'deezer', 'tidal', 'gam',
];

function LogEntryCard({ entry, isHighlighted }: { entry: LogEntry; isHighlighted: boolean }) {
  const [expanded, setExpanded] = useState(isHighlighted);
  const ref = useRef<HTMLDivElement>(null);
  const cfg = LEVEL_CONFIG[entry.level];
  const srcCfg = SOURCE_CONFIG[entry.source] || SOURCE_CONFIG.app;
  const Icon = cfg.icon;

  useEffect(() => {
    if (isHighlighted) {
      setExpanded(true);
      ref.current?.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  }, [isHighlighted]);

  return (
    <div
      ref={ref}
      className={cn(
        'rounded-lg border p-3 transition-all duration-500',
        cfg.border,
        isHighlighted ? `${cfg.bg} ring-2 ring-primary` : 'bg-card'
      )}
    >
      <button
        className="flex items-center gap-3 w-full text-left"
        onClick={() => setExpanded(!expanded)}
      >
        <Icon className={cn('h-4 w-4 shrink-0', cfg.color)} />
        <div className="flex-1 min-w-0">
          <span className="font-medium text-sm">{entry.title}</span>
          <span className="text-xs text-muted-foreground ml-2">
            {entry.order !== undefined ? `#${entry.order} · ` : ''}{new Date(entry.timestamp).toLocaleTimeString()}
          </span>
        </div>
        <span className={cn(
          'text-[10px] font-medium px-1.5 py-0.5 rounded-full',
          'bg-muted', srcCfg.color
        )}>
          {srcCfg.label}
        </span>
        <span className={cn(
          'text-xs font-medium uppercase px-2 py-0.5 rounded-full',
          cfg.bg, cfg.color
        )}>
          {entry.level}
        </span>
        {expanded ? <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" /> : <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />}
      </button>

      <AnimatePresence>
        {expanded && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="overflow-hidden"
          >
            <pre className="mt-3 p-3 rounded-md bg-muted/50 text-xs font-mono whitespace-pre-wrap break-words max-h-96 overflow-y-auto border border-border">
              {entry.fullLog || 'No log output captured.'}
            </pre>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

export default function LogsPage() {
  const { entries, highlightId, setHighlight, clearLogs } = useLogStore();
  const [filter, setFilter] = useState('');
  const [levelFilter, setLevelFilter] = useState<'all' | 'info' | 'warning' | 'error'>('all');
  const [sourceFilter, setSourceFilter] = useState<'all' | LogSource>('all');

  useEffect(() => {
    if (highlightId) {
      const timer = setTimeout(() => setHighlight(null), 3000);
      return () => clearTimeout(timer);
    }
  }, [highlightId, setHighlight]);

  const filtered = entries.filter((e) => {
    if (levelFilter !== 'all' && e.level !== levelFilter) return false;
    if (sourceFilter !== 'all' && e.source !== sourceFilter) return false;
    if (filter && !e.title.toLowerCase().includes(filter.toLowerCase()) && !e.fullLog.toLowerCase().includes(filter.toLowerCase())) return false;
    return true;
  });

  return (
    <div className="p-6 space-y-4 h-full flex flex-col">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold">Logs</h1>
        <Button variant="ghost" size="sm" onClick={clearLogs} disabled={entries.length === 0}>
          <Trash2 className="h-4 w-4 mr-1" /> Clear
        </Button>
      </div>

      <div className="flex gap-2">
        <Input
          placeholder="Search logs..."
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          className="flex-1 h-9"
        />
        {(['all', 'error', 'warning', 'info'] as const).map((lvl) => (
          <Button
            key={lvl}
            variant={levelFilter === lvl ? 'default' : 'outline'}
            size="sm"
            className="h-9 capitalize"
            onClick={() => setLevelFilter(lvl)}
          >
            {lvl}
          </Button>
        ))}
      </div>

      <div className="flex gap-1.5 flex-wrap">
        {SOURCE_FILTERS.map((src) => (
          <Button
            key={src}
            variant={sourceFilter === src ? 'default' : 'outline'}
            size="sm"
            className="h-7 text-xs capitalize"
            onClick={() => setSourceFilter(src)}
          >
            {src === 'all' ? 'All Sources' : SOURCE_CONFIG[src].label}
          </Button>
        ))}
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto space-y-2">
        {filtered.length === 0 ? (
          <div className="flex items-center justify-center h-32 text-muted-foreground text-sm">
            {entries.length === 0 ? 'No logs yet. Logs appear as you use the app.' : 'No logs match the filter.'}
          </div>
        ) : (
          filtered.map((entry) => (
            <LogEntryCard
              key={entry.id}
              entry={entry}
              isHighlighted={entry.id === highlightId}
            />
          ))
        )}
      </div>
    </div>
  );
}

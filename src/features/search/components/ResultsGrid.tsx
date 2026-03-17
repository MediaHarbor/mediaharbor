import { motion, AnimatePresence } from 'framer-motion';
import { ResultCard } from './ResultCard';
import { Loader2, SearchX, AlertCircle } from 'lucide-react';
import type { SearchResult } from '@/types';

interface ResultsGridProps {
  results: SearchResult[];
  isLoading?: boolean;
  error?: Error | null;
  onPlayTrack?: (result: SearchResult) => void;
  onPlayNext?: (result: SearchResult) => void;
  onDownload?: (result: SearchResult) => void;
  onResultClick?: (result: SearchResult) => void;
}

const listVariants = {
  hidden: {},
  show: { transition: { staggerChildren: 0.03 } },
};

const itemVariants = {
  hidden: { opacity: 0, y: 8 },
  show:   { opacity: 1, y: 0, transition: { duration: 0.18, ease: 'easeOut' } },
};

export function ResultsGrid({
  results,
  isLoading,
  error,
  onPlayTrack,
  onPlayNext,
  onDownload,
  onResultClick,
}: ResultsGridProps) {
  const safeResults = Array.isArray(results) ? results : [];

  if (isLoading) {
    return (
      <div className="flex h-48 items-center justify-center">
        <div className="flex flex-col items-center gap-3 text-muted-foreground">
          <Loader2 className="h-6 w-6 animate-spin" />
          <p className="text-xs">Searching...</p>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex h-48 items-center justify-center">
        <div className="flex flex-col items-center gap-2 text-center">
          <AlertCircle className="h-6 w-6 text-destructive/70" />
          <p className="text-sm font-medium text-destructive">Search failed</p>
          <p className="text-xs text-muted-foreground">{error.message}</p>
        </div>
      </div>
    );
  }

  if (safeResults.length === 0) {
    return (
      <div className="flex h-48 items-center justify-center">
        <div className="flex flex-col items-center gap-2 text-muted-foreground">
          <SearchX className="h-6 w-6 opacity-40" />
          <p className="text-sm">No results — try a different query</p>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-0.5">
      <div className="flex items-center gap-3 px-3 pb-2 text-[11px] font-medium text-muted-foreground uppercase tracking-wide">
        <div className="w-10 shrink-0" />
        <div className="flex-1">Title</div>
        <div className="w-10 text-right">Time</div>
        <div className="w-[4.5rem]" />
      </div>

      <motion.div
        className="space-y-0.5"
        variants={listVariants}
        initial="hidden"
        animate="show"
      >
        <AnimatePresence>
          {safeResults.map((result, index) => (
            <motion.div key={`${result.id}-${index}`} variants={itemVariants}>
              <ResultCard
                result={result}
                onPlay={onPlayTrack ? () => onPlayTrack(result) : undefined}
                onPlayNext={onPlayNext ? () => onPlayNext(result) : undefined}
                onDownload={onDownload ? () => onDownload(result) : undefined}
                onClick={onResultClick ? () => onResultClick(result) : undefined}
              />
            </motion.div>
          ))}
        </AnimatePresence>
      </motion.div>
    </div>
  );
}

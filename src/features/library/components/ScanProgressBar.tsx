import { ScanLine } from 'lucide-react';
import { motion } from 'framer-motion';

interface ScanProgressBarProps {
  progress: number;
  currentFile: string;
}

export function ScanProgressBar({ progress, currentFile }: ScanProgressBarProps) {
  // Zero only happens while the walker is still counting files, which on a large tree
  // takes long enough that a bar pinned at 0% reads as a hang.
  const indexing = progress <= 0;

  return (
    <motion.div
      initial={{ opacity: 0, y: -8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -8 }}
      className="rounded-xl border border-primary/20 bg-primary/5 p-4 space-y-2.5"
    >
      <div className="flex items-center justify-between text-sm">
        <span className="font-medium flex items-center gap-2 text-primary">
          <ScanLine className="h-4 w-4 animate-pulse" />
          {indexing ? 'Indexing files…' : 'Scanning library…'}
        </span>
        {!indexing && (
          <span className="text-muted-foreground font-mono text-xs">{Math.round(progress)}%</span>
        )}
      </div>
      <div className="h-1.5 w-full rounded-full bg-primary/10 overflow-hidden">
        {indexing ? (
          <motion.div
            className="h-full w-1/3 rounded-full bg-primary"
            animate={{ x: ['-100%', '300%'] }}
            transition={{ duration: 1.2, ease: 'easeInOut', repeat: Infinity }}
          />
        ) : (
          <motion.div
            className="h-full rounded-full bg-primary"
            initial={{ width: 0 }}
            animate={{ width: `${progress}%` }}
            transition={{ duration: 0.3, ease: 'easeOut' }}
          />
        )}
      </div>
      {currentFile && (
        <p className="text-[11px] text-muted-foreground/60 truncate font-mono">{currentFile}</p>
      )}
    </motion.div>
  );
}

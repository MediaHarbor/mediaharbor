import { ScanLine } from 'lucide-react';
import { motion } from 'framer-motion';

interface ScanProgressBarProps {
  progress: number;
  currentFile: string;
}

export function ScanProgressBar({ progress, currentFile }: ScanProgressBarProps) {
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
          Scanning library…
        </span>
        <span className="text-muted-foreground font-mono text-xs">
          {Math.round(progress)}%
        </span>
      </div>
      <div className="h-1.5 w-full rounded-full bg-primary/10 overflow-hidden">
        <motion.div
          className="h-full rounded-full bg-primary"
          initial={{ width: 0 }}
          animate={{ width: `${progress}%` }}
          transition={{ duration: 0.3, ease: 'easeOut' }}
        />
      </div>
      {currentFile && (
        <p className="text-[11px] text-muted-foreground/60 truncate font-mono">
          {currentFile}
        </p>
      )}
    </motion.div>
  );
}

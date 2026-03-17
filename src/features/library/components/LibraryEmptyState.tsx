import { Music2, Search, FolderOpen } from 'lucide-react';
import { motion } from 'framer-motion';

interface LibraryEmptyStateProps {
  variant: 'no-folder' | 'empty' | 'no-results';
}

export function LibraryEmptyState({ variant }: LibraryEmptyStateProps) {
  const config = {
    'no-folder': {
      icon: FolderOpen,
      title: 'No download folder set',
      description: 'Go to Settings → General and set your download location to get started.',
      color: 'text-amber-500/40',
      bg: 'from-amber-500/5 to-transparent',
    },
    empty: {
      icon: Music2,
      title: 'Your library is empty',
      description: 'Download some music or videos first — they\'ll appear here automatically.',
      color: 'text-primary/30',
      bg: 'from-primary/5 to-transparent',
    },
    'no-results': {
      icon: Search,
      title: 'No results found',
      description: 'Try a different search term or adjust your filters.',
      color: 'text-muted-foreground/30',
      bg: 'from-muted/30 to-transparent',
    },
  };

  const { icon: Icon, title, description, color, bg } = config[variant];

  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className={`rounded-2xl border border-border/40 bg-gradient-to-b ${bg} p-16 flex flex-col items-center gap-4 text-center`}
    >
      <div className={`${color}`}>
        <Icon className="h-16 w-16" strokeWidth={1.2} />
      </div>
      <div className="space-y-1.5">
        <p className="font-semibold text-lg">{title}</p>
        <p className="text-sm text-muted-foreground/70 max-w-sm">{description}</p>
      </div>
    </motion.div>
  );
}

import { motion } from 'framer-motion';
import { Music2, Library, Settings } from 'lucide-react';
import logo from '@/assets/MediaHarbor_Logo.svg';

const HIGHLIGHTS = [
  {
    icon: Music2,
    title: 'Download music',
    description: 'From Spotify, Tidal, Deezer, Qobuz, Apple Music and YouTube.',
  },
  {
    icon: Library,
    title: 'Build your library',
    description: 'Scan, browse, and play your downloaded tracks offline.',
  },
  {
    icon: Settings,
    title: 'Configure services',
    description: 'Set up platform credentials, quality, and file paths.',
  },
];

export function StepWelcome() {
  return (
    <motion.div
      initial={{ opacity: 0, y: 16 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.1 }}
      className="flex flex-col items-center text-center gap-6 py-4"
    >
      <img src={logo} alt="MediaHarbor" className="w-16 h-16" />
      <div className="space-y-2">
        <h1 className="text-2xl font-bold tracking-tight">Welcome to MediaHarbor</h1>
        <p className="text-sm text-muted-foreground max-w-xs">
          Your all-in-one desktop music downloader. Let&apos;s get you set up in a few quick steps.
        </p>
      </div>
      <div className="w-full space-y-2 text-left">
        {HIGHLIGHTS.map(({ icon: Icon, title, description }) => (
          <div
            key={title}
            className="flex items-start gap-3 rounded-xl border border-border bg-card/60 px-4 py-3"
          >
            <div className="rounded-lg bg-muted/50 p-2 shrink-0">
              <Icon className="w-4 h-4 text-foreground" />
            </div>
            <div>
              <p className="text-sm font-medium">{title}</p>
              <p className="text-xs text-muted-foreground">{description}</p>
            </div>
          </div>
        ))}
      </div>
    </motion.div>
  );
}

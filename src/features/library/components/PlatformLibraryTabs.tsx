import { Library, Construction } from 'lucide-react';
import { motion } from 'framer-motion';
import { Button } from '@/components/ui/button';
import { PlatformIcon } from '@/utils/platforms';

const PLATFORMS = [
  { id: 'local', label: 'Local Library', wip: false },
  { id: 'spotify', label: 'Spotify', wip: true },
  { id: 'deezer', label: 'Deezer', wip: true },
  { id: 'tidal', label: 'Tidal', wip: true },
];

interface PlatformTabsProps {
  activeTab: string;
  onTabChange: (tab: string) => void;
}

export function PlatformTabs({ activeTab, onTabChange }: PlatformTabsProps) {
  return (
    <div className="flex items-center gap-1 bg-muted/30 rounded-lg p-0.5 w-fit">
      {PLATFORMS.map((p) => (
        <button
          key={p.id}
          onClick={() => onTabChange(p.id)}
          className={`inline-flex items-center gap-2 px-3 py-1.5 rounded-md text-xs font-medium transition-all duration-150 ${
            activeTab === p.id
              ? 'bg-background text-foreground shadow-sm'
              : 'text-muted-foreground hover:text-foreground'
          }`}
        >
          {p.id === 'local' ? (
            <Library className="h-3.5 w-3.5" />
          ) : (
            <PlatformIcon platform={p.id} size={14} />
          )}
          {p.label}
          {p.wip && (
            <span className="text-[8px] font-bold uppercase tracking-wider bg-amber-500/10 text-amber-500/70 px-1 py-0.5 rounded leading-none">
              WIP
            </span>
          )}
        </button>
      ))}
    </div>
  );
}

export function PlatformPlaceholder({ platformId }: { platformId: string }) {
  const platform = PLATFORMS.find((p) => p.id === platformId);
  if (!platform) return null;

  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
      className="space-y-6"
    >
      <div className="relative rounded-2xl border border-border/30 bg-gradient-to-br from-card to-muted/30 p-8 text-center overflow-hidden">
        <div className="absolute top-3 right-3">
          <span className="inline-flex items-center gap-1 text-[10px] font-bold uppercase tracking-wider bg-amber-500/15 text-amber-600 dark:text-amber-400 px-2.5 py-1 rounded-full">
            <Construction className="h-3 w-3" />
            Work in Progress
          </span>
        </div>

        <div className="flex flex-col items-center gap-4 py-4">
          <div className="h-16 w-16 rounded-2xl flex items-center justify-center shadow-lg bg-muted/40">
            <PlatformIcon platform={platform.id} size={36} />
          </div>

          <div className="space-y-2 max-w-md">
            <h3 className="text-xl font-bold">{platform.label} Library</h3>
            <p className="text-sm text-muted-foreground/70">
              Connect your {platform.label} account to browse and manage your saved
              albums, playlists, and tracks — all from within MediaHarbor.
            </p>
          </div>

          <Button
            variant="outline"
            size="sm"
            disabled
            className="rounded-full gap-2 mt-2 opacity-60"
          >
            Connect {platform.label} Account
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4 opacity-30">
        {Array.from({ length: 12 }).map((_, i) => (
          <div key={i} className="rounded-2xl border border-border/30 bg-card/50 overflow-hidden">
            <div className="aspect-square bg-muted animate-pulse" />
            <div className="p-3 space-y-2">
              <div className="h-3 bg-muted rounded-full w-3/4 animate-pulse" />
              <div className="h-2.5 bg-muted rounded-full w-1/2 animate-pulse" />
            </div>
          </div>
        ))}
      </div>
    </motion.div>
  );
}

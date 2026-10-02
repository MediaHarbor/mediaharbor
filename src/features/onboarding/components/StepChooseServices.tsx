import { motion } from 'framer-motion';
import { Download, Check } from 'lucide-react';
import { cn } from '@/utils/cn';
import { PlatformIcon } from '@/utils/platforms';
import { useOnboardingStore } from '../stores/useOnboardingStore';
import { GATED_SERVICES } from '@/utils/platform-data';

/// What each service actually gives you, for the onboarding card. Only the copy lives
/// here — the roster and the labels come from the descriptor table, so a new gated
/// service cannot be added without showing up on this step.
const DESCRIPTIONS: Record<string, string> = {
  spotify: 'Up to 256 kbps AAC · full catalog',
  tidal: 'Lossless FLAC · Hi-Res up to 24-bit/192 kHz',
  qobuz: 'Lossless FLAC · Hi-Res up to 24-bit/192 kHz',
  deezer: 'Up to lossless FLAC (HiFi)',
  applemusic: 'Up to 256 kbps AAC · full catalog',
};

const SERVICES = GATED_SERVICES.map((s) => ({
  ...s,
  description: DESCRIPTIONS[s.platform] ?? '',
}));

export function StepChooseServices() {
  const { enabledServices, toggleService } = useOnboardingStore();

  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.05 }}
      className="space-y-6"
    >
      <div className="space-y-1">
        <h2 className="text-lg font-semibold">How will you use MediaHarbor?</h2>
        <p className="text-sm text-muted-foreground">
          You can start with no setup at all — just paste a URL and download. Add streaming services
          only if you want to browse and download from your accounts.
        </p>
      </div>

      <div className="rounded-xl border border-primary/40 bg-primary/5 p-4">
        <div className="flex items-start gap-3">
          <div className="rounded-lg bg-primary/10 p-2 text-primary">
            <Download className="w-5 h-5" />
          </div>
          <div className="space-y-0.5">
            <p className="text-sm font-semibold">Just paste URLs</p>
            <p className="text-xs text-muted-foreground">
              Works out of the box with yt-dlp — YouTube, SoundCloud and hundreds of sites. No
              accounts needed. This is always available.
            </p>
          </div>
        </div>
      </div>

      <div className="space-y-2">
        <p className="text-sm font-medium">
          Add streaming services{' '}
          <span className="text-muted-foreground font-normal">(optional)</span>
        </p>
        <div className="grid grid-cols-2 gap-2">
          {SERVICES.map(({ platform, label, description }) => {
            const selected = enabledServices.includes(platform);
            return (
              <button
                key={platform}
                onClick={() => toggleService(platform)}
                className={cn(
                  'flex items-center gap-2.5 rounded-xl border p-3 transition-colors text-sm text-left',
                  selected
                    ? 'border-primary bg-primary/10 text-foreground'
                    : 'border-border bg-card/60 text-muted-foreground hover:bg-muted/40'
                )}
              >
                <PlatformIcon platform={platform} size={18} />
                <span className="flex-1 min-w-0">
                  <span className="block font-medium">{label}</span>
                  <span className="block text-[11px] text-muted-foreground/70 truncate">
                    {description}
                  </span>
                </span>
                {selected && <Check className="w-4 h-4 text-primary shrink-0" />}
              </button>
            );
          })}
        </div>
        <p className="text-xs text-muted-foreground">
          You can add or remove services any time in Settings.
        </p>
      </div>
    </motion.div>
  );
}

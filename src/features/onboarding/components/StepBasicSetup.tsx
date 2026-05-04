import { useEffect } from 'react';
import { motion } from 'framer-motion';
import { FolderOpen, Moon, Sun, Monitor } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/utils/cn';
import { useOnboardingStore } from '../stores/useOnboardingStore';
import { useThemeStore } from '@/stores/useThemeStore';

const THEME_OPTIONS = [
  { value: 'dark' as const, label: 'Dark', icon: Moon },
  { value: 'light' as const, label: 'Light', icon: Sun },
  { value: 'auto' as const, label: 'System', icon: Monitor },
];

export function StepBasicSetup() {
  const { downloadLocation, theme, setDownloadLocation, setTheme } = useOnboardingStore();

  useEffect(() => {
    window.electron?.settings.get().then((data) => {
      if (data?.downloadLocation) {
        setDownloadLocation(data.downloadLocation);
      }
    });
  }, [setDownloadLocation]);

  const handleBrowse = async () => {
    const path = await window.electron?.settings.openFolder();
    if (path) setDownloadLocation(path);
  };

  const handleTheme = (t: 'auto' | 'dark' | 'light') => {
    setTheme(t);
    const resolved =
      t === 'auto'
        ? window.matchMedia('(prefers-color-scheme: dark)').matches
          ? 'dark'
          : 'light'
        : t;
    useThemeStore.getState().setTheme(resolved);
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.05 }}
      className="space-y-6"
    >
      <div className="space-y-1">
        <h2 className="text-lg font-semibold">Basic Setup</h2>
        <p className="text-sm text-muted-foreground">
          Choose where to save your music and how the app looks.
        </p>
      </div>
      <div className="space-y-2">
        <p className="text-sm font-medium">Download location</p>
        <div className="flex items-center gap-2">
          <div className="flex-1 rounded-lg border border-border bg-muted/30 px-3 py-2 text-sm text-muted-foreground truncate min-w-0">
            {downloadLocation || 'Not set'}
          </div>
          <Button variant="outline" size="sm" className="shrink-0 gap-1.5" onClick={handleBrowse}>
            <FolderOpen className="w-3.5 h-3.5" />
            Browse
          </Button>
        </div>
      </div>
      <div className="space-y-2">
        <p className="text-sm font-medium">Appearance</p>
        <div className="grid grid-cols-3 gap-2">
          {THEME_OPTIONS.map(({ value, label, icon: Icon }) => (
            <button
              key={value}
              onClick={() => handleTheme(value)}
              className={cn(
                'flex flex-col items-center gap-2 rounded-xl border p-3 transition-colors text-sm',
                theme === value
                  ? 'border-primary bg-primary/10 text-foreground ring-2 ring-primary ring-offset-1 ring-offset-background'
                  : 'border-border bg-card/60 text-muted-foreground hover:bg-muted/40'
              )}
            >
              <Icon className="w-5 h-5" />
              <span className="text-xs font-medium">{label}</span>
            </button>
          ))}
        </div>
      </div>
    </motion.div>
  );
}

import { HashRouter as Router, useLocation, useNavigate } from 'react-router-dom';
import { useEffect, useState, useRef, lazy, Suspense } from 'react';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { MainLayout } from '@/components/layout/MainLayout';
import { Toaster } from '@/components/ui/toaster';
import {
  resolveThemePreference,
  useThemeStore,
  watchSystemTheme,
  type ThemePreference,
} from '@/stores/useThemeStore';
import { useDownloadEvents } from '@/hooks/useDownloadEvents';
import { useKeyboardShortcuts } from '@/hooks/useKeyboardShortcuts';
import { useSavedStateSyncBootstrap } from '@/features/library/hooks/useSavedStateSyncBootstrap';
import { useAutoplayContinuation } from '@/features/player/hooks/useAutoplayContinuation';
import { useCredentialHealthBootstrap } from '@/features/credentials/useCredentialHealthBootstrap';
import { useAutoUpdateCheck } from '@/hooks/useAutoUpdateCheck';
import { useAppSettings } from '@/hooks/useAppSettings';
import { useNotificationStore } from '@/stores/useNotificationStore';
import { usePlayerStore } from '@/stores/usePlayerStore';
import { useLogStore, type LogSource } from '@/stores/useLogStore';
import { logError } from '@/utils/logger';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { GlobalDownloadDialog } from '@/components/GlobalDownloadDialog';
import { OnboardingWizard, SpotlightTour } from '@/features/onboarding';
import { useOnboardingStore } from '@/features/onboarding/stores/useOnboardingStore';

import SearchPage from '@/pages/SearchPage';
import { tauriAPI } from '@/tauri-bridge';

const DownloadPage = lazy(() => import('@/pages/DownloadPage'));
const LibraryPage = lazy(() => import('@/pages/LibraryPage'));
const RadioPage = lazy(() => import('@/pages/RadioPage'));
const SettingsPage = lazy(() => import('@/pages/SettingsPage'));
const HelpPage = lazy(() => import('@/pages/HelpPage'));
const UpdatesPage = lazy(() => import('@/pages/UpdatesPage'));
const LogsPage = lazy(() => import('@/pages/LogsPage'));

const PAGES = [
  { path: '/search', Component: SearchPage },
  { path: '/downloads', Component: DownloadPage },
  { path: '/library', Component: LibraryPage },
  { path: '/radio', Component: RadioPage },
  { path: '/settings', Component: SettingsPage },
  { path: '/updates', Component: UpdatesPage },
  { path: '/logs', Component: LogsPage },
  { path: '/help', Component: HelpPage },
] as const;

function PersistentRoutes() {
  const location = useLocation();
  const currentPath = ['/', ''].includes(location.pathname) ? '/search' : location.pathname;

  const [everMounted, setEverMounted] = useState<ReadonlySet<string>>(() => new Set([currentPath]));

  if (!everMounted.has(currentPath)) {
    setEverMounted((prev) => {
      if (prev.has(currentPath)) return prev;
      const next = new Set(prev);
      next.add(currentPath);
      return next;
    });
  }

  return (
    <div className="flex-1 min-h-0 overflow-hidden relative">
      <Suspense fallback={null}>
        {PAGES.map(({ path, Component }) => {
          const isActive = currentPath === path;
          return (
            <div key={path} className={isActive ? 'h-full overflow-y-auto' : 'hidden'}>
              {everMounted.has(path) && <Component />}
            </div>
          );
        })}
      </Suspense>
    </div>
  );
}

function AppInner() {
  const theme = useThemeStore((state) => state.theme);
  const setTheme = useThemeStore((state) => state.setTheme);
  const addNotification = useNotificationStore((s) => s.addNotification);
  const setPlaying = usePlayerStore((s) => s.setPlaying);
  const addLog = useLogStore((s) => s.addLog);
  const navigate = useNavigate();
  const { data: appSettings } = useAppSettings();
  useDownloadEvents();
  useKeyboardShortcuts();
  useSavedStateSyncBootstrap();
  useAutoplayContinuation();
  useCredentialHealthBootstrap();
  useAutoUpdateCheck({
    enabled: appSettings?.autoUpdate,
    autoDownload: appSettings?.autoDownloadUpdates,
  });

  const [stdinPrompt, setStdinPrompt] = useState<{
    downloadId: number;
    promptLines: string[];
  } | null>(null);
  const [stdinInput, setStdinInput] = useState('');
  const stdinInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    return tauriAPI.app.onStdinPrompt((data) => {
      setStdinInput('');
      setStdinPrompt(data);
    });
  }, []);

  useEffect(() => {
    if (stdinPrompt) requestAnimationFrame(() => stdinInputRef.current?.focus());
  }, [stdinPrompt]);

  const handleStdinSubmit = async () => {
    if (!stdinPrompt) return;
    const prompt = stdinPrompt;
    setStdinPrompt(null);
    await tauriAPI.app.sendProcessStdin(prompt.downloadId, stdinInput);
  };

  const onboardingSeeded = useRef(false);
  useEffect(() => {
    if (!appSettings?.theme) return;

    if (!onboardingSeeded.current && !appSettings.onboarding_completed) {
      onboardingSeeded.current = true;
      const store = useOnboardingStore.getState();
      store.setDownloadLocation(appSettings.downloadLocation ?? '');
      store.setTheme((appSettings.theme as ThemePreference) ?? 'auto');
      store.open();
    }

    const pref = appSettings.theme as ThemePreference;
    setTheme(resolveThemePreference(pref));
    if (pref === 'auto') return watchSystemTheme();
  }, [appSettings, setTheme]);

  useEffect(() => {
    const cleanup = tauriAPI.app?.onError?.((data) => {
      const msg = data.message ?? 'An unexpected error occurred.';
      const isNotInstalled =
        msg.includes('not installed') || msg.includes('not in PATH') || msg.includes('ENOENT');
      if (data.context === 'playback') setPlaying(false);

      const detail = isNotInstalled
        ? `${msg}\n\nFix: pip install yt-dlp  or  winget install yt-dlp.yt-dlp`
        : msg;

      const source = data.context === 'playback' ? ('playback' as const) : ('app' as const);

      const needsAuth = data.needs_auth;
      if (needsAuth) {
        navigate(`/settings?tab=${needsAuth}`);
        logError(
          source,
          'Login required',
          `You need to log in to ${needsAuth.charAt(0).toUpperCase() + needsAuth.slice(1)} first. Opening settings…`,
          { duration: 6000 }
        );
        return;
      }

      logError(source, isNotInstalled ? 'Dependency not found' : 'Error', detail, {
        duration: isNotInstalled ? 10000 : 6000,
      });
    });
    return () => cleanup?.();
  }, [addNotification, setPlaying, navigate]);

  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove('light', 'dark');
    root.classList.add(theme);
  }, [theme]);

  useEffect(() => {
    const cleanup = tauriAPI.app?.onBackendLog?.((data) => {
      const levelMap: Record<string, 'info' | 'warning' | 'error'> = {
        info: 'info',
        warn: 'warning',
        error: 'error',
      };
      addLog({
        source: (data.source as LogSource) || 'system',
        title: data.title || 'System',
        fullLog: data.message || '',
        level: levelMap[data.level] || 'info',
      });
    });
    return () => cleanup?.();
  }, [addLog]);

  return (
    <MainLayout>
      <PersistentRoutes />
      <Dialog
        open={!!stdinPrompt}
        onOpenChange={(open) => {
          if (!open) setStdinPrompt(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Input Required</DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            {stdinPrompt?.promptLines.map((line, i) => (
              <p key={i} className="text-sm font-mono text-muted-foreground">
                {line}
              </p>
            ))}
            <Input
              ref={stdinInputRef}
              value={stdinInput}
              onChange={(e) => setStdinInput(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleStdinSubmit()}
              placeholder="Type your response..."
            />
          </div>
          <DialogFooter>
            <Button onClick={handleStdinSubmit}>Send</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <GlobalDownloadDialog />
      <OnboardingWizard />
      <SpotlightTour />
    </MainLayout>
  );
}

function App() {
  return (
    <Router>
      <ErrorBoundary>
        <AppInner />
      </ErrorBoundary>
      <Toaster />
    </Router>
  );
}

export default App;

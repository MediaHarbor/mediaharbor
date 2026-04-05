import { useState, useEffect } from 'react';
import {
  RefreshCw, ExternalLink, CheckCircle2, AlertCircle,
  PackageCheck, Loader2, ArrowUpCircle, XCircle,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Checkbox } from '@/components/ui/checkbox';
import { logError, logWarning, logInfo } from '@/utils/logger';

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h3 className="text-xs font-semibold text-muted-foreground uppercase tracking-widest border-b border-border pb-1">
      {children}
    </h3>
  );
}

type AppStatus = 'idle' | 'checking' | 'up-to-date' | 'available' | 'error';

interface UpdateInfo {
  hasUpdate: boolean; currentVersion: string; latestVersion: string;
  releaseNotes: string; releaseUrl: string; publishedAt: string;
}

function AppUpdateSection() {
  const [status, setStatus]           = useState<AppStatus>('idle');
  const [info, setInfo]               = useState<UpdateInfo | null>(null);
  const [currentVersion, setVersion]  = useState('');
  const [errorMsg, setErrorMsg]       = useState('');

  useEffect(() => {
    window.electron?.updates.getVersion().then(setVersion).catch((err: unknown) => {
      logWarning('system', 'Failed to fetch app version', err instanceof Error ? (err.stack || err.message) : String(err));
    });
  }, []);

  const handleCheck = async () => {
    if (!window.electron) return;
    setStatus('checking'); setErrorMsg(''); setInfo(null);
    try {
      const result = await window.electron.updates.check();
      setInfo(result); setVersion(result.currentVersion);
      setStatus(result.hasUpdate ? 'available' : 'up-to-date');
    } catch (err: unknown) {
      setErrorMsg(err instanceof Error ? err.message : 'Could not reach update server.');
      setStatus('error');
    }
  };

  const publishedDate = info?.publishedAt
    ? new Date(info.publishedAt).toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' })
    : '';

  return (
    <div className="space-y-3">
      <SectionTitle>MediaHarbor</SectionTitle>

      <div className="rounded-xl border border-border bg-card p-4 flex items-center gap-4">
        <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-muted shrink-0">
          <PackageCheck className="h-4 w-4 text-muted-foreground" />
        </div>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium">MediaHarbor</p>
          <p className="text-xs text-muted-foreground">Version: <span className="font-mono">{currentVersion || '…'}</span></p>
        </div>
        <Button size="sm" variant="outline" onClick={handleCheck} disabled={status === 'checking'} className="gap-1.5 shrink-0">
          {status === 'checking' ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <RefreshCw className="h-3.5 w-3.5" />}
          {status === 'checking' ? 'Checking…' : 'Check for updates'}
        </Button>
      </div>

      {status === 'up-to-date' && (
        <div className="flex items-center gap-3 rounded-xl border border-border bg-card px-4 py-3">
          <CheckCircle2 className="h-4 w-4 text-emerald-500 shrink-0" />
          <div>
            <p className="text-sm font-medium">You&apos;re up to date</p>
            <p className="text-xs text-muted-foreground">{info?.latestVersion} is the latest release.</p>
          </div>
        </div>
      )}

      {status === 'error' && (
        <div className="flex items-start gap-3 rounded-xl border border-destructive/30 bg-destructive/5 px-4 py-3">
          <AlertCircle className="h-4 w-4 text-destructive shrink-0 mt-0.5" />
          <p className="text-sm text-destructive">{errorMsg}</p>
        </div>
      )}

      {status === 'available' && info && (
        <div className="rounded-xl border border-border bg-card overflow-hidden">
          <div className="px-4 py-3 border-b border-border flex items-center justify-between gap-4">
            <div className="flex items-center gap-3 min-w-0">
              <ArrowUpCircle className="h-4 w-4 text-primary shrink-0" />
              <div>
                <p className="text-sm font-semibold">Update available — <span className="text-primary">{info.latestVersion}</span></p>
                {publishedDate && <p className="text-xs text-muted-foreground">Released {publishedDate}</p>}
              </div>
            </div>
            <Button size="sm" onClick={() => window.electron?.updates.openRelease(info.releaseUrl)} className="gap-1.5 shrink-0">
              <ExternalLink className="h-3.5 w-3.5" /> Download
            </Button>
          </div>
          {info.releaseNotes && (
            <div className="px-4 py-4">
              <p className="text-[11px] font-medium text-muted-foreground uppercase tracking-wide mb-2">Release Notes</p>
              <pre className="text-sm text-foreground/80 whitespace-pre-wrap leading-relaxed font-sans">{info.releaseNotes}</pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

const REQUIRED_DEPS = [
  { id: 'python', label: 'Python',  desc: 'Required — 3.10+',                          pipName: null },
  { id: 'git',    label: 'Git',     desc: 'Required — for git+ package installs',      pipName: null },
  { id: 'ffmpeg', label: 'FFmpeg',  desc: 'Required — audio/video processing',         pipName: null },
  { id: 'ytdlp',  label: 'yt-dlp', desc: 'Required — YouTube & audio downloader',     pipName: 'yt-dlp' },
];

const OPTIONAL_DEPS = [
  { id: 'apple',   label: 'Apple Music', desc: 'gamdl + Bento4 — Apple Music downloader', pipName: 'gamdl',  parent: null },
  { id: 'spotify', label: 'Spotify',     desc: 'votify — Spotify downloader',             pipName: 'votify', parent: null },
];

const ALL_DEPS = [...REQUIRED_DEPS, ...OPTIONAL_DEPS];

type InstallStatus = 'idle' | 'installing' | 'done' | 'error';

function SystemDepsSection() {
  const [checking, setChecking]                   = useState(true);
  const [depStatus, setDepStatus]                 = useState<Record<string, boolean>>({});
  const [versions, setVersions]                   = useState<Record<string, string>>({});
  const [installStatus, setInstallStatus]         = useState<Record<string, InstallStatus>>({});
  const [installProgress, setInstallProgress]     = useState<Record<string, number>>({});
  const [installStatusText, setInstallStatusText] = useState<Record<string, string>>({});
  const [installing, setInstalling]               = useState<string | null>(null);

  const fetchStatus = async () => {
    if (!window.electron) return;
    setChecking(true);
    try {
      const [deps, vers, binVers] = await Promise.all([
        window.electron.updates.checkDeps(),
        window.electron.updates.getDependencyVersions(
          ALL_DEPS.filter((d) => d.pipName).map((d) => d.pipName!)
        ),
        window.electron.updates.getBinaryVersions(),
      ]);
      setDepStatus(deps);
      const mapped: Record<string, string> = {};
      if (binVers.python) mapped['python'] = binVers.python;
      if (binVers.git)    mapped['git']    = binVers.git;
      if (binVers.ffmpeg) mapped['ffmpeg'] = binVers.ffmpeg;
      for (const dep of ALL_DEPS) {
        if (!dep.pipName) continue;
        const ver = vers[dep.pipName.toLowerCase()] ?? '';
        if (ver) mapped[dep.id] = ver;
      }
      setVersions(mapped);
    } catch (err: unknown) {
      logWarning('system', 'Failed to check dependencies', err instanceof Error ? (err.stack || err.message) : String(err));
    }
    setChecking(false);
  };

  useEffect(() => { fetchStatus(); }, []);

  const handleInstall = async (depId: string) => {
    if (!window.electron) return;
    const depLabel = ALL_DEPS.find((d) => d.id === depId)?.label ?? depId;
    setInstalling(depId);
    setInstallStatus((p) => ({ ...p, [depId]: 'installing' }));
    setInstallProgress((p) => ({ ...p, [depId]: 0 }));
    setInstallStatusText((p) => ({ ...p, [depId]: 'Starting…' }));
    logInfo('install', `Installing ${depLabel}`, `Starting installation of ${depLabel}...`);

    const cleanup = window.electron.updates.onInstallProgress((data) => {
      if (data.dependency !== depId) return;
      setInstallProgress((p) => ({ ...p, [depId]: data.percent }));
      setInstallStatusText((p) => ({ ...p, [depId]: data.status }));
    });

    try {
      await window.electron.updates.installDep(depId);
      setInstallStatus((p) => ({ ...p, [depId]: 'done' }));
      setDepStatus((p) => ({ ...p, [depId]: true }));
      logInfo('install', `${depLabel} installed`, `${depLabel} was installed/updated successfully.`, { notify: true });
      const dep = ALL_DEPS.find((d) => d.id === depId);
      if (dep?.pipName) {
        window.electron?.updates.getDependencyVersions([dep.pipName]).then((r) => {
          const ver = r[dep.pipName!.toLowerCase()] ?? '';
          if (ver) setVersions((p) => ({ ...p, [depId]: ver }));
        }).catch((err: unknown) => {
          logWarning('install', `Version check failed for ${depLabel}`, err instanceof Error ? (err.stack || err.message) : String(err));
        });
      } else {
        window.electron?.updates.getBinaryVersions().then((b) => {
          setVersions((p) => ({
            ...p,
            ...(b.python ? { python: b.python } : {}),
            ...(b.git    ? { git:    b.git    } : {}),
            ...(b.ffmpeg ? { ffmpeg: b.ffmpeg } : {}),
          }));
        }).catch((err: unknown) => {
          logWarning('install', `Version check failed for ${depLabel}`, err instanceof Error ? (err.stack || err.message) : String(err));
        });
      }
    } catch (err: unknown) {
      setInstallStatus((p) => ({ ...p, [depId]: 'error' }));
      logError('install', `Failed to install ${depLabel}`, err instanceof Error ? (err.stack || err.message) : String(err));
    } finally {
      cleanup();
      setInstalling(null);
    }
  };

  const renderDep = (dep: typeof ALL_DEPS[number]) => {
    const installed = depStatus[dep.id];
    const inst      = installStatus[dep.id] ?? 'idle';
    const progress  = installProgress[dep.id] ?? 0;
    const statusTxt = installStatusText[dep.id] ?? '';
    const ver       = versions[dep.id];

    return (
      <div key={dep.id} className="flex items-center gap-3 py-3 px-4">
        <div className="shrink-0">
          {inst === 'installing' ? <Loader2      className="h-4 w-4 animate-spin text-muted-foreground" /> :
           inst === 'done'       ? <CheckCircle2 className="h-4 w-4 text-emerald-500" /> :
           inst === 'error'      ? <XCircle      className="h-4 w-4 text-destructive" /> :
           installed             ? <CheckCircle2 className="h-4 w-4 text-emerald-500" /> :
                                   <XCircle      className="h-4 w-4 text-muted-foreground/30" />}
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-baseline gap-2">
            <p className="text-sm font-medium">{dep.label}</p>
            {ver && <span className="text-[11px] font-mono text-muted-foreground">{ver}</span>}
          </div>
          <p className="text-xs text-muted-foreground">{dep.desc}</p>
          {inst === 'installing' && (
            <div className="mt-1.5 space-y-0.5">
              <div className="h-1 rounded-full bg-muted overflow-hidden">
                <div className="h-full rounded-full bg-primary transition-all duration-300" style={{ width: `${progress}%` }} />
              </div>
              <p className="text-[11px] text-muted-foreground">{statusTxt}</p>
            </div>
          )}
        </div>
        <Button
          size="sm"
          variant={installed && inst !== 'error' ? 'outline' : 'default'}
          onClick={() => handleInstall(dep.id)}
          disabled={!!installing || dep.pipName === null}
          className="shrink-0"
        >
          {dep.pipName === null
            ? 'Built-in'
            : installing === dep.id
            ? <><Loader2 className="h-3 w-3 animate-spin mr-1" />Installing…</>
            : installed ? 'Update' : 'Install'}
        </Button>
      </div>
    );
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <SectionTitle>Dependencies</SectionTitle>
        <button
          onClick={fetchStatus}
          disabled={checking || !!installing}
          className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 disabled:opacity-40 transition-colors"
        >
          <RefreshCw className={`h-3 w-3 ${checking ? 'animate-spin' : ''}`} /> Refresh
        </button>
      </div>

      {checking ? (
        <div className="flex items-center gap-2 px-4 py-6 text-sm text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" /> Checking installed dependencies…
        </div>
      ) : (
        <div className="rounded-xl border border-border bg-card overflow-hidden">
          <div className="px-4 py-2 border-b border-border bg-muted/30">
            <p className="text-[11px] font-medium text-muted-foreground uppercase tracking-wide">Required</p>
          </div>
          <div className="divide-y divide-border">
            {REQUIRED_DEPS.map(renderDep)}
          </div>
          <div className="px-4 py-2 border-t border-border border-b border-border bg-muted/30">
            <p className="text-[11px] font-medium text-muted-foreground uppercase tracking-wide">Optional</p>
          </div>
          <div className="divide-y divide-border">
            {OPTIONAL_DEPS.map(renderDep)}
          </div>
        </div>
      )}
    </div>
  );
}

function OrpheusDLSection() {
  const [orpheusInstalled, setOrpheusInstalled] = useState(false);
  const [modules, setModules] = useState<Array<{ id: string; label: string; installed: boolean }>>([]);
  const [enabledModules, setEnabledModules] = useState<Set<string>>(new Set());
  const [installing, setInstalling] = useState<string | null>(null);
  const [installProgress, setInstallProgress] = useState(0);
  const [installStatusText, setInstallStatusText] = useState('');
  const [customUrl, setCustomUrl] = useState('');
  const [customLabel, setCustomLabel] = useState('');

  const fetchStatus = async () => {
    if (!window.electron) return;
    try {
      const [deps, settings] = await Promise.all([
        window.electron.orpheus.checkDeps(),
        window.electron.settings.get(),
      ]);
      setOrpheusInstalled(deps.orpheus_installed);
      setModules(deps.modules);
      const mods = (settings?.orpheus_dl_enabled_modules ?? 'tidal,qobuz,deezer')
        .split(',').map((m: string) => m.trim()).filter(Boolean);
      setEnabledModules(new Set(mods));
    } catch (err) {
      logWarning('system', 'Failed to check OrpheusDL status', err instanceof Error ? (err.stack || err.message) : String(err));
    }
  };

  useEffect(() => { fetchStatus(); }, []);

  const persistEnabledModules = async (next: Set<string>) => {
    if (!window.electron) return;
    try {
      const current = await window.electron.settings.get();
      await window.electron.settings.set({ ...current, orpheus_dl_enabled_modules: [...next].join(',') });
    } catch (err) {
      logWarning('system', 'Failed to save enabled modules', err instanceof Error ? (err.stack || err.message) : String(err));
    }
  };

  const toggleModule = (id: string, checked: boolean) => {
    const next = new Set(enabledModules);
    if (checked) next.add(id); else next.delete(id);
    setEnabledModules(next);
    persistEnabledModules(next);
  };

  const handleInstallCore = async () => {
    if (!window.electron || installing) return;
    setInstalling('core');
    setInstallProgress(0);
    setInstallStatusText('Starting…');
    logInfo('install', 'Installing OrpheusDL', 'Starting OrpheusDL core installation...');

    const cleanup = window.electron.orpheus.onInstallProgress((data) => {
      if (data.dependency !== 'orpheus') return;
      setInstallProgress(data.percent);
      setInstallStatusText(data.status);
    });

    try {
      await window.electron.orpheus.installCore();
      setOrpheusInstalled(true);
      logInfo('install', 'OrpheusDL installed', 'OrpheusDL core installed successfully.', { notify: true });
    } catch (err) {
      logError('install', 'Failed to install OrpheusDL', err instanceof Error ? (err.stack || err.message) : String(err));
    } finally {
      cleanup();
      setInstalling(null);
    }
  };

  const handleInstallModule = async (moduleId: string, customGitUrl?: string, label?: string) => {
    if (!window.electron || installing) return;
    const depKey = customGitUrl ? 'custom' : moduleId;
    setInstalling(depKey);
    setInstallProgress(0);
    setInstallStatusText('Starting…');
    const depTag = `orpheus_module_${moduleId || 'custom'}`;

    const cleanup = window.electron.orpheus.onInstallProgress((data) => {
      if (data.dependency !== depTag) return;
      setInstallProgress(data.percent);
      setInstallStatusText(data.status);
    });

    try {
      await window.electron.orpheus.installModule(moduleId, customGitUrl, label);
      setModules((prev) => {
        const exists = prev.some((m) => m.id === moduleId);
        if (exists) return prev.map((m) => m.id === moduleId ? { ...m, installed: true } : m);
        return [...prev, { id: moduleId, label: label || moduleId, installed: true }];
      });
      logInfo('install', `Module ${moduleId} installed`, `OrpheusDL module ${moduleId} installed.`, { notify: true });
      if (customGitUrl) { setCustomUrl(''); setCustomLabel(''); }
    } catch (err) {
      logError('install', `Failed to install module ${moduleId}`, err instanceof Error ? (err.stack || err.message) : String(err));
    } finally {
      cleanup();
      setInstalling(null);
    }
  };

  return (
    <div className="space-y-3">
      <SectionTitle>OrpheusDL</SectionTitle>

      <div className="rounded-xl border border-border bg-card overflow-hidden">
        <div className="flex items-center gap-3 px-4 py-3">
          <div className="shrink-0">
            {installing === 'core'
              ? <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
              : orpheusInstalled
              ? <CheckCircle2 className="h-4 w-4 text-emerald-500" />
              : <XCircle className="h-4 w-4 text-muted-foreground/30" />}
          </div>
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium">OrpheusDL</p>
            <p className="text-xs text-muted-foreground">Alternative download backend supporting multiple services</p>
            {installing === 'core' && (
              <div className="mt-1.5 space-y-0.5">
                <div className="h-1 rounded-full bg-muted overflow-hidden">
                  <div className="h-full rounded-full bg-primary transition-all duration-300" style={{ width: `${installProgress}%` }} />
                </div>
                <p className="text-[11px] text-muted-foreground">{installStatusText}</p>
              </div>
            )}
          </div>
          <Button
            size="sm"
            variant={orpheusInstalled ? 'outline' : 'default'}
            onClick={handleInstallCore}
            disabled={!!installing}
            className="shrink-0"
          >
            {installing === 'core'
              ? <><Loader2 className="h-3 w-3 animate-spin mr-1" />Installing…</>
              : orpheusInstalled ? 'Reinstall' : 'Install'}
          </Button>
        </div>
      </div>

      <SectionTitle>Modules</SectionTitle>

      <div className="rounded-xl border border-border bg-card overflow-hidden">
        <div className="divide-y divide-border">
          {modules.map((mod) => {
            const isInstalling = installing === mod.id;
            return (
              <div key={mod.id} className="flex items-center gap-3 px-4 py-3">
                <Checkbox
                  checked={enabledModules.has(mod.id)}
                  onCheckedChange={(v) => toggleModule(mod.id, !!v)}
                  disabled={!mod.installed}
                />
                <div className="shrink-0">
                  {isInstalling
                    ? <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                    : mod.installed
                    ? <CheckCircle2 className="h-4 w-4 text-emerald-500" />
                    : <XCircle className="h-4 w-4 text-muted-foreground/30" />}
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium">{mod.label}</p>
                  {isInstalling && (
                    <div className="mt-1 space-y-0.5">
                      <div className="h-1 rounded-full bg-muted overflow-hidden">
                        <div className="h-full rounded-full bg-primary transition-all duration-300" style={{ width: `${installProgress}%` }} />
                      </div>
                      <p className="text-[11px] text-muted-foreground">{installStatusText}</p>
                    </div>
                  )}
                </div>
                <Button
                  size="sm"
                  variant={mod.installed ? 'outline' : 'default'}
                  onClick={() => handleInstallModule(mod.id)}
                  disabled={!!installing || !orpheusInstalled}
                  className="shrink-0"
                >
                  {isInstalling
                    ? <><Loader2 className="h-3 w-3 animate-spin mr-1" />Installing…</>
                    : mod.installed ? 'Update' : 'Install'}
                </Button>
              </div>
            );
          })}
        </div>

        <div className="px-4 py-3 border-t border-border bg-muted/20">
          <p className="text-[11px] font-medium text-muted-foreground uppercase tracking-wide mb-2">Custom Module</p>
          <div className="flex flex-col gap-2">
            <Input
              value={customUrl}
              onChange={(e) => setCustomUrl(e.target.value)}
              placeholder="https://github.com/user/orpheusdl-module"
              className="text-sm"
            />
            <div className="flex items-center gap-2">
              <Input
                value={customLabel}
                onChange={(e) => setCustomLabel(e.target.value)}
                placeholder="Service name (e.g. Napster)"
                className="flex-1 text-sm"
              />
              <Button
                size="sm"
                onClick={() => {
                  const id = customUrl.split('/').pop()?.replace(/^orpheusdl-/, '') ?? 'custom';
                  handleInstallModule(id, customUrl, customLabel.trim() || undefined);
                }}
                disabled={!!installing || !orpheusInstalled || !customUrl.trim()}
                className="shrink-0"
              >
                {installing === 'custom'
                  ? <><Loader2 className="h-3 w-3 animate-spin mr-1" />Installing…</>
                  : 'Install'}
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

export default function UpdatesPage() {
  return (
    <div className="flex flex-col h-full">
      <div className="px-8 pt-8 pb-6 border-b border-border shrink-0">
        <h1 className="text-lg font-semibold">Updates & Dependencies</h1>
        <p className="text-sm text-muted-foreground mt-0.5">Install and update all MediaHarbor components</p>
      </div>

      <div className="flex-1 overflow-y-auto">
        <div className="px-8 py-6 space-y-8">
          <AppUpdateSection />
          <SystemDepsSection />
          <OrpheusDLSection />
        </div>
      </div>
    </div>
  );
}

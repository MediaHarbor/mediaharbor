import { useState, useEffect } from 'react';
import { errorDetail, errorMessage } from '@/utils/errors';
import {
  RefreshCw,
  ExternalLink,
  CircleCheckBig,
  CircleAlert,
  PackageCheck,
  Loader2,
  CircleArrowUp,
  CircleX,
  Download,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Checkbox } from '@/components/ui/checkbox';
import { logError, logWarning, logInfo } from '@/utils/logger';
import { tauriAPI, type UpdateChannel, type UpdateStatus } from '@/tauri-bridge';

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <h3 className="text-xs font-semibold text-muted-foreground uppercase tracking-widest border-b border-border pb-1">
      {children}
    </h3>
  );
}

type AppStatus = 'idle' | 'checking' | 'up-to-date' | 'available' | 'error';
type ApplyStatus = 'idle' | 'downloading' | 'ready' | 'applying';

const CHANNEL_ADVICE: Partial<Record<UpdateChannel, string>> = {
  flatpak: 'Installed as a Flatpak — update with `flatpak update org.mediaharbor.MediaHarbor`.',
  snap: 'Installed as a Snap — snapd updates it for you, or run `sudo snap refresh mediaharbor`.',
  source:
    'This build was not produced by the Tauri bundler (built from source, an AUR package, or the Microsoft Store), so it cannot replace itself. Update it the way you installed it.',
};

function AppUpdateSection() {
  const [status, setStatus] = useState<AppStatus>('idle');
  const [info, setInfo] = useState<UpdateStatus | null>(null);
  const [currentVersion, setVersion] = useState('');
  const [errorMsg, setErrorMsg] = useState('');
  const [apply, setApply] = useState<ApplyStatus>('idle');
  const [percent, setPercent] = useState(0);

  useEffect(() => {
    tauriAPI.updates
      .getVersion()
      .then(setVersion)
      .catch((err: unknown) => {
        logWarning('system', 'Failed to fetch app version', errorDetail(err));
      });
  }, []);

  useEffect(() => tauriAPI.updates.onUpdateProgress((p) => setPercent(p.percent)), []);

  const handleCheck = async () => {
    setStatus('checking');
    setErrorMsg('');
    setInfo(null);
    setApply('idle');
    try {
      const result = await tauriAPI.updates.status();
      setInfo(result);
      setVersion(result.currentVersion);
      setStatus(result.available ? 'available' : 'up-to-date');
      // A background download may already have staged this build.
      setApply(result.staged ? 'ready' : 'idle');
    } catch (err: unknown) {
      setErrorMsg(errorMessage(err));
      setStatus('error');
    }
  };

  const handleDownload = async () => {
    setApply('downloading');
    setPercent(0);
    setErrorMsg('');
    try {
      await tauriAPI.updates.download();
      setApply('ready');
      logInfo('install', 'Update downloaded', 'Restart MediaHarbor to finish updating.');
    } catch (err: unknown) {
      setApply('idle');
      setErrorMsg(errorMessage(err));
      setStatus('error');
      logError('install', 'Failed to download update', errorDetail(err));
    }
  };

  const handleApply = async () => {
    setApply('applying');
    try {
      await tauriAPI.updates.apply();
    } catch (err: unknown) {
      setApply('ready');
      setErrorMsg(errorMessage(err));
      setStatus('error');
      logError('install', 'Failed to install update', errorDetail(err));
    }
  };

  const publishedDate = info?.date
    ? new Date(info.date).toLocaleDateString(undefined, {
        year: 'numeric',
        month: 'long',
        day: 'numeric',
      })
    : '';

  const advice = info ? CHANNEL_ADVICE[info.channel] : undefined;

  return (
    <div className="p-6 space-y-3">
      <SectionTitle>MediaHarbor</SectionTitle>

      <div className="rounded-xl border border-border bg-card p-4 flex items-center gap-4">
        <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-muted shrink-0">
          <PackageCheck className="h-4 w-4 text-muted-foreground" />
        </div>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium">MediaHarbor</p>
          <p className="text-xs text-muted-foreground">
            Version: <span className="font-mono">{currentVersion || '…'}</span>
          </p>
        </div>
        <Button
          size="sm"
          variant="outline"
          onClick={handleCheck}
          disabled={status === 'checking' || apply !== 'idle'}
          className="gap-1.5 shrink-0"
        >
          {status === 'checking' ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" />
          )}
          {status === 'checking' ? 'Checking…' : 'Check for updates'}
        </Button>
      </div>

      {status === 'up-to-date' && (
        <div className="flex items-center gap-3 rounded-xl border border-border bg-card px-4 py-3">
          <CircleCheckBig className="h-4 w-4 text-emerald-500 shrink-0" />
          <div>
            <p className="text-sm font-medium">You&apos;re up to date</p>
            <p className="text-xs text-muted-foreground">{currentVersion} is the latest release.</p>
          </div>
        </div>
      )}

      {status === 'error' && (
        <div className="flex items-start gap-3 rounded-xl border border-destructive/30 bg-destructive/5 px-4 py-3">
          <CircleAlert className="h-4 w-4 text-destructive shrink-0 mt-0.5" />
          <p className="text-sm text-destructive">{errorMsg}</p>
        </div>
      )}

      {status === 'available' && info && (
        <div className="rounded-xl border border-border bg-card overflow-hidden">
          <div className="px-4 py-3 border-b border-border flex items-center justify-between gap-4">
            <div className="flex items-center gap-3 min-w-0">
              <CircleArrowUp className="h-4 w-4 text-primary shrink-0" />
              <div>
                <p className="text-sm font-semibold">
                  Update available — <span className="text-primary">{info.version}</span>
                </p>
                {publishedDate && (
                  <p className="text-xs text-muted-foreground">Released {publishedDate}</p>
                )}
              </div>
            </div>

            {info.supported ? (
              apply === 'ready' ? (
                <Button size="sm" onClick={handleApply} className="gap-1.5 shrink-0">
                  <RefreshCw className="h-3.5 w-3.5" /> Restart now
                </Button>
              ) : (
                <Button
                  size="sm"
                  onClick={handleDownload}
                  disabled={apply !== 'idle'}
                  className="gap-1.5 shrink-0"
                >
                  {apply === 'idle' ? (
                    <Download className="h-3.5 w-3.5" />
                  ) : (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  )}
                  {apply === 'idle' ? 'Download & install' : 'Installing…'}
                </Button>
              )
            ) : (
              <Button
                size="sm"
                onClick={() => info.releaseUrl && tauriAPI.updates.openRelease(info.releaseUrl)}
                disabled={!info.releaseUrl}
                className="gap-1.5 shrink-0"
              >
                <ExternalLink className="h-3.5 w-3.5" /> Download
              </Button>
            )}
          </div>

          {apply === 'downloading' && (
            <div className="px-4 pt-3">
              <InstallProgress percent={percent} text={`Downloading ${info.version}…`} />
            </div>
          )}

          {apply === 'ready' && (
            <div className="px-4 pt-3">
              <p className="text-xs text-muted-foreground">
                Downloaded and verified. MediaHarbor will close while it installs.
              </p>
            </div>
          )}

          {advice && (
            <div className="px-4 pt-3">
              <p className="text-[11px] text-amber-500/80">{advice}</p>
            </div>
          )}

          {info.notes && (
            <div className="px-4 py-4">
              <p className="text-[11px] font-medium text-muted-foreground uppercase tracking-wide mb-2">
                Release Notes
              </p>
              <pre className="text-sm text-foreground/80 whitespace-pre-wrap leading-relaxed font-sans">
                {info.notes}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

const REQUIRED_DEPS = [
  { id: 'python', label: 'Python', desc: 'Required — 3.10+', pipName: null, installable: true },
  {
    id: 'ffmpeg',
    label: 'FFmpeg',
    desc: 'Required — audio/video processing',
    pipName: null,
    installable: true,
  },
  {
    id: 'ytdlp',
    label: 'yt-dlp',
    desc: 'Required — YouTube & audio downloader',
    pipName: 'yt-dlp',
    installable: true,
  },
];

const OPTIONAL_DEPS = [
  {
    id: 'apple',
    label: 'Apple Music',
    desc: 'gamdl — Apple Music downloader',
    pipName: 'gamdl',
    parent: null,
    installable: true,
  },
  {
    id: 'spotify',
    label: 'Spotify',
    desc: 'votify — Spotify downloader',
    pipName: 'votify',
    parent: null,
    installable: true,
  },
  {
    id: 'deno',
    label: 'Deno',
    desc: 'JavaScript runtime — YouTube signature solving',
    pipName: null,
    parent: null,
    installable: true,
  },
  {
    id: 'aria2c',
    label: 'aria2c',
    desc: 'Multi-connection accelerator for yt-dlp',
    pipName: null,
    parent: null,
    installable: true,
  },
  {
    id: 'nm3u8dlre',
    label: 'N_m3u8DL-RE',
    desc: 'Faster HLS downloader for Apple Music (gamdl)',
    pipName: null,
    parent: null,
    installable: true,
    pinned: true,
  },
  {
    id: 'bento4',
    label: 'Bento4',
    desc: 'mp4decrypt — only for the votify backend with remux mode "mp4decrypt"',
    pipName: null,
    parent: null,
    installable: true,
    pinned: true,
  },
];

const ALL_DEPS = [...REQUIRED_DEPS, ...OPTIONAL_DEPS];

type DepState = 'busy' | 'ok' | 'error' | 'missing';

function DepStatusIcon({ state }: { state: DepState }) {
  if (state === 'busy') return <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />;
  if (state === 'ok') return <CircleCheckBig className="h-4 w-4 text-emerald-500" />;
  if (state === 'error') return <CircleX className="h-4 w-4 text-destructive" />;
  return <CircleX className="h-4 w-4 text-muted-foreground/30" />;
}

function InstallProgress({ percent, text }: { percent: number; text: string }) {
  return (
    <div className="mt-1.5 space-y-1">
      <div className="h-1 w-full rounded-full bg-muted overflow-hidden">
        <div
          className="h-full rounded-full bg-primary transition-all duration-300"
          style={{ width: `${Math.min(100, Math.max(0, percent))}%` }}
        />
      </div>
      {text && <p className="text-[11px] text-muted-foreground truncate">{text}</p>}
    </div>
  );
}

type InstallStatus = 'idle' | 'installing' | 'done' | 'error';

function SystemDepsSection() {
  const [checking, setChecking] = useState(true);
  const [depStatus, setDepStatus] = useState<Record<string, boolean>>({});
  const [versions, setVersions] = useState<Record<string, string>>({});
  const [installStatus, setInstallStatus] = useState<Record<string, InstallStatus>>({});
  const [installProgress, setInstallProgress] = useState<Record<string, number>>({});
  const [installStatusText, setInstallStatusText] = useState<Record<string, string>>({});
  const [installing, setInstalling] = useState<string | null>(null);

  const fetchStatus = async () => {
    setChecking(true);
    try {
      const [deps, vers] = await Promise.all([
        tauriAPI.updates.checkDeps(),
        tauriAPI.updates.getDependencyVersions(),
      ]);
      setDepStatus(deps);
      const mapped: Record<string, string> = {};
      for (const dep of ALL_DEPS) {
        const ver = vers[dep.id] ?? '';
        if (ver) mapped[dep.id] = ver;
      }
      setVersions(mapped);
    } catch (err: unknown) {
      logWarning('system', 'Failed to check dependencies', errorDetail(err));
    }
    setChecking(false);
  };

  useEffect(() => {
    fetchStatus();
  }, []);

  const handleInstall = async (depId: string, force = false) => {
    const depLabel = ALL_DEPS.find((d) => d.id === depId)?.label ?? depId;
    const verb = force ? 'Updating' : 'Installing';
    setInstalling(depId);
    setInstallStatus((p) => ({ ...p, [depId]: 'installing' }));
    setInstallProgress((p) => ({ ...p, [depId]: 0 }));
    setInstallStatusText((p) => ({ ...p, [depId]: 'Starting…' }));
    logInfo('install', `${verb} ${depLabel}`, `Starting ${verb.toLowerCase()} of ${depLabel}...`);

    const cleanup = tauriAPI.updates.onInstallProgress((data) => {
      if (data.dependency !== depId) return;
      setInstallProgress((p) => ({ ...p, [depId]: data.percent }));
      setInstallStatusText((p) => ({ ...p, [depId]: data.status }));
    });

    try {
      await tauriAPI.updates.installDep(depId, force);
      setInstallStatus((p) => ({ ...p, [depId]: 'done' }));
      setDepStatus((p) => ({ ...p, [depId]: true }));
      logInfo(
        'install',
        `${depLabel} ${force ? 'updated' : 'installed'}`,
        `${depLabel} was ${force ? 'updated' : 'installed'} successfully.`,
        { notify: true }
      );
      tauriAPI.updates
        .getDependencyVersions()
        .then((r) => {
          const ver = r[depId] ?? '';
          if (ver) setVersions((p) => ({ ...p, [depId]: ver }));
        })
        .catch((err: unknown) => {
          logWarning('install', `Version check failed for ${depLabel}`, errorDetail(err));
        });
    } catch (err: unknown) {
      setInstallStatus((p) => ({ ...p, [depId]: 'error' }));
      logError('install', `Failed to install ${depLabel}`, errorDetail(err));
    } finally {
      cleanup();
      setInstalling(null);
    }
  };

  const renderDep = (dep: (typeof ALL_DEPS)[number]) => {
    const installed = depStatus[dep.id];
    const inst = installStatus[dep.id] ?? 'idle';
    const progress = installProgress[dep.id] ?? 0;
    const statusTxt = installStatusText[dep.id] ?? '';
    const ver = versions[dep.id];

    let btnLabel: React.ReactNode;
    if (!dep.installable) btnLabel = 'Built-in';
    else if (installing === dep.id)
      btnLabel = (
        <>
          <Loader2 className="h-3 w-3 animate-spin mr-1" />
          Installing…
        </>
      );
    else if (dep.id === 'python') btnLabel = installed ? 'Recreate' : 'Setup';
    else if (installed) btnLabel = 'pinned' in dep && dep.pinned === true ? 'Reinstall' : 'Update';
    else btnLabel = 'Install';

    return (
      <div key={dep.id} className="flex items-center gap-3 py-3 px-4">
        <div className="shrink-0">
          <DepStatusIcon
            state={
              inst === 'installing'
                ? 'busy'
                : inst === 'error'
                  ? 'error'
                  : inst === 'done' || installed
                    ? 'ok'
                    : 'missing'
            }
          />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-baseline gap-2">
            <p className="text-sm font-medium">{dep.label}</p>
            {ver && <span className="text-[11px] font-mono text-muted-foreground">{ver}</span>}
          </div>
          <p className="text-xs text-muted-foreground">{dep.desc}</p>
          {inst === 'installing' && <InstallProgress percent={progress} text={statusTxt} />}
        </div>
        <Button
          size="sm"
          variant={installed && inst !== 'error' ? 'outline' : 'default'}
          onClick={() => handleInstall(dep.id, !!installed)}
          disabled={!!installing || !dep.installable}
          className="shrink-0"
        >
          {btnLabel}
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
            <p className="text-[11px] font-medium text-muted-foreground uppercase tracking-wide">
              Required
            </p>
          </div>
          <div className="divide-y divide-border">{REQUIRED_DEPS.map(renderDep)}</div>
          <div className="px-4 py-2 border-t border-border border-b border-border bg-muted/30">
            <p className="text-[11px] font-medium text-muted-foreground uppercase tracking-wide">
              Optional
            </p>
          </div>
          <div className="divide-y divide-border">{OPTIONAL_DEPS.map(renderDep)}</div>
        </div>
      )}
    </div>
  );
}

function OrpheusDLSection() {
  const [orpheusInstalled, setOrpheusInstalled] = useState(false);
  const [modules, setModules] = useState<Array<{ id: string; label: string; installed: boolean }>>(
    []
  );
  const [enabledModules, setEnabledModules] = useState<Set<string>>(new Set());
  const [installing, setInstalling] = useState<string | null>(null);
  const [installProgress, setInstallProgress] = useState(0);
  const [installStatusText, setInstallStatusText] = useState('');
  const [customUrl, setCustomUrl] = useState('');
  const [customLabel, setCustomLabel] = useState('');

  const fetchStatus = async () => {
    try {
      const [deps, settings] = await Promise.all([
        tauriAPI.orpheus.checkDeps(),
        tauriAPI.settings.get(),
      ]);
      setOrpheusInstalled(deps.orpheus_installed);
      setModules(deps.modules);
      const mods = (settings?.orpheus_dl_enabled_modules ?? 'tidal,qobuz,deezer')
        .split(',')
        .map((m: string) => m.trim())
        .filter(Boolean);
      setEnabledModules(new Set(mods));
    } catch (err) {
      logWarning('system', 'Failed to check OrpheusDL status', errorDetail(err));
    }
  };

  useEffect(() => {
    fetchStatus();
  }, []);

  const persistEnabledModules = async (next: Set<string>) => {
    try {
      const current = await tauriAPI.settings.get();
      await tauriAPI.settings.set({
        ...current,
        orpheus_dl_enabled_modules: [...next].join(','),
      });
    } catch (err) {
      logWarning('system', 'Failed to save enabled modules', errorDetail(err));
    }
  };

  const toggleModule = (id: string, checked: boolean) => {
    const next = new Set(enabledModules);
    if (checked) next.add(id);
    else next.delete(id);
    setEnabledModules(next);
    persistEnabledModules(next);
  };

  const handleInstallCore = async () => {
    if (installing) return;
    setInstalling('core');
    setInstallProgress(0);
    setInstallStatusText('Starting…');
    logInfo('install', 'Installing OrpheusDL', 'Starting OrpheusDL core installation...');

    const cleanup = tauriAPI.orpheus.onInstallProgress((data) => {
      if (data.dependency !== 'orpheus') return;
      setInstallProgress(data.percent);
      setInstallStatusText(data.status);
    });

    try {
      await tauriAPI.orpheus.installCore();
      setOrpheusInstalled(true);
      logInfo('install', 'OrpheusDL installed', 'OrpheusDL core installed successfully.', {
        notify: true,
      });
    } catch (err) {
      logError('install', 'Failed to install OrpheusDL', errorDetail(err));
    } finally {
      cleanup();
      setInstalling(null);
    }
  };

  const handleInstallModule = async (moduleId: string, customGitUrl?: string, label?: string) => {
    if (installing) return;
    const depKey = customGitUrl ? 'custom' : moduleId;
    setInstalling(depKey);
    setInstallProgress(0);
    setInstallStatusText('Starting…');
    const depTag = `orpheus_module_${moduleId || 'custom'}`;

    const cleanup = tauriAPI.orpheus.onInstallProgress((data) => {
      if (data.dependency !== depTag) return;
      setInstallProgress(data.percent);
      setInstallStatusText(data.status);
    });

    try {
      await tauriAPI.orpheus.installModule(moduleId, customGitUrl, label);
      setModules((prev) => {
        const exists = prev.some((m) => m.id === moduleId);
        if (exists) return prev.map((m) => (m.id === moduleId ? { ...m, installed: true } : m));
        return [...prev, { id: moduleId, label: label || moduleId, installed: true }];
      });
      logInfo(
        'install',
        `Module ${moduleId} installed`,
        `OrpheusDL module ${moduleId} installed.`,
        { notify: true }
      );
      if (customGitUrl) {
        setCustomUrl('');
        setCustomLabel('');
      }
    } catch (err) {
      logError('install', `Failed to install module ${moduleId}`, errorDetail(err));
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
            <DepStatusIcon
              state={installing === 'core' ? 'busy' : orpheusInstalled ? 'ok' : 'missing'}
            />
          </div>
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium">OrpheusDL</p>
            <p className="text-xs text-muted-foreground">
              Alternative download backend supporting multiple services
            </p>
            {installing === 'core' && (
              <InstallProgress percent={installProgress} text={installStatusText} />
            )}
          </div>
          <Button
            size="sm"
            variant={orpheusInstalled ? 'outline' : 'default'}
            onClick={handleInstallCore}
            disabled={!!installing}
            className="shrink-0"
          >
            {installing === 'core' ? (
              <>
                <Loader2 className="h-3 w-3 animate-spin mr-1" />
                Installing…
              </>
            ) : orpheusInstalled ? (
              'Reinstall'
            ) : (
              'Install'
            )}
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
                  <DepStatusIcon state={isInstalling ? 'busy' : mod.installed ? 'ok' : 'missing'} />
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium">{mod.label}</p>
                  {isInstalling && (
                    <InstallProgress percent={installProgress} text={installStatusText} />
                  )}
                </div>
                <Button
                  size="sm"
                  variant={mod.installed ? 'outline' : 'default'}
                  onClick={() => handleInstallModule(mod.id)}
                  disabled={!!installing || !orpheusInstalled}
                  className="shrink-0"
                >
                  {isInstalling ? (
                    <>
                      <Loader2 className="h-3 w-3 animate-spin mr-1" />
                      Installing…
                    </>
                  ) : mod.installed ? (
                    'Update'
                  ) : (
                    'Install'
                  )}
                </Button>
              </div>
            );
          })}
        </div>

        <div className="px-4 py-3 border-t border-border bg-muted/20">
          <p className="text-[11px] font-medium text-muted-foreground uppercase tracking-wide mb-2">
            Custom Module
          </p>
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
                  const id =
                    customUrl
                      .split('/')
                      .pop()
                      ?.replace(/^orpheusdl-/, '') ?? 'custom';
                  handleInstallModule(id, customUrl, customLabel.trim() || undefined);
                }}
                disabled={!!installing || !orpheusInstalled || !customUrl.trim()}
                className="shrink-0"
              >
                {installing === 'custom' ? (
                  <>
                    <Loader2 className="h-3 w-3 animate-spin mr-1" />
                    Installing…
                  </>
                ) : (
                  'Install'
                )}
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
        <p className="text-sm text-muted-foreground mt-0.5">
          Install and update all MediaHarbor components
        </p>
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

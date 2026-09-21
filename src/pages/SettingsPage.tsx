import {
  useState,
  useEffect,
  useRef,
  useCallback,
  useMemo,
  createContext,
  useContext,
} from 'react';
import { errorDetail, errorMessage } from '@/utils/errors';
import { useLocation } from 'react-router-dom';
import { invalidateSettingsDerived } from '@/lib/queryClient';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Button } from '@/components/ui/button';
import {
  resolveThemePreference,
  useThemeStore,
  type ThemePreference,
} from '@/stores/useThemeStore';
import { useRadioSources } from '@/features/radio/hooks/useRadioQueries';
import { AnimatePresence, motion } from 'framer-motion';
import { cn } from '@/utils/cn';
import type { WrapperProbeResult } from '@/tauri-bridge';
import { logError, logWarning } from '@/utils/logger';
import type { LogSource } from '@/stores/useLogStore';
import {
  Check,
  ChevronDown,
  Eye,
  EyeOff,
  Loader2,
  SlidersHorizontal,
  KeyRound,
  FileText,
  BoomBox,
  Plus,
  X,
} from 'lucide-react';
import { PlatformIcon, YtDlpIcon } from '@/utils/platforms';
import { normalizePlatform } from '@/utils/platform-data';
import {
  APPLE_COVER_FORMAT_OPTS,
  APPLE_DOWNLOAD_MODE_OPTS,
  APPLE_LOG_LEVEL_OPTS,
  APPLE_MV_CODEC_PRIORITY_OPTS,
  APPLE_MV_REMUX_FORMAT_OPTS,
  APPLE_MV_RESOLUTION_OPTS,
  APPLE_UPLOADED_VIDEO_QUALITY_OPTS,
  CONVERSION_CODEC_OPTS,
  SPOTIFY_AUDIO_DOWNLOAD_MODE_OPTS,
  SPOTIFY_AUDIO_REMUX_MODE_OPTS,
  SPOTIFY_COVER_SIZE_OPTS,
  SPOTIFY_LOG_LEVEL_OPTS,
  SPOTIFY_SESSION_TYPE_OPTS,
  THEME_OPTS,
  TIDAL_DEVICE_TYPE_OPTS,
  TIDAL_VIDEO_QUALITY_OPTS,
} from '@/features/settings/options';
import type { Settings, SettingsSetter } from '@/types/settings';
import { DEEZER_TIER_TO_FORMAT, QUALITY_OPTIONS, deezerTierOf } from '@/utils/constants';
import { tauriAPI } from '@/tauri-bridge';
function Row({
  label,
  help,
  disabled,
  children,
}: {
  label: string;
  help?: string;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div
      className={cn(
        'grid grid-cols-[200px_1fr] items-start gap-4 py-0.5',
        disabled && 'opacity-50'
      )}
    >
      <div className="pt-0.5">
        <Label className="text-sm font-medium text-foreground/90 leading-none">{label}</Label>
        {help && <p className="text-xs text-muted-foreground mt-1 leading-relaxed">{help}</p>}
      </div>
      <div>{children}</div>
    </div>
  );
}
function Section({
  title,
  children,
  disabled,
}: {
  title: string;
  children: React.ReactNode;
  disabled?: boolean;
}) {
  return (
    <div className="rounded-xl border border-border bg-card/50 overflow-hidden">
      <div className="px-4 py-2.5 border-b border-border/60 bg-muted/20 flex items-center justify-between gap-2">
        <h3 className="text-[11px] font-semibold text-muted-foreground uppercase tracking-widest">
          {title}
        </h3>
      </div>
      <fieldset
        disabled={!!disabled}
        className={cn('px-4 py-3 space-y-3', disabled && 'opacity-50 cursor-not-allowed')}
      >
        {children}
      </fieldset>
    </div>
  );
}
function ToggleSection({
  title,
  enabled,
  onToggle,
  children,
  disabled,
}: {
  title: string;
  enabled: boolean;
  onToggle: (v: boolean) => void;
  children: React.ReactNode;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(enabled);
  return (
    <div
      className={cn(
        'rounded-xl border overflow-hidden transition-colors',
        enabled ? 'border-border' : 'border-border/40',
        disabled && 'opacity-60'
      )}
    >
      <div
        className={cn(
          'flex items-center gap-3 px-4 py-2.5 border-b',
          enabled ? 'bg-muted/20 border-border/60' : 'bg-muted/10 border-border/30'
        )}
      >
        <Checkbox
          checked={!!enabled}
          onCheckedChange={(v) => onToggle(!!v)}
          disabled={disabled}
          className="mt-px"
        />
        <button
          onClick={() => setOpen(!open)}
          className="flex items-center gap-1.5 flex-1 text-left"
          disabled={disabled}
        >
          <h3
            className={cn(
              'text-[11px] font-semibold uppercase tracking-widest transition-colors',
              enabled ? 'text-muted-foreground' : 'text-muted-foreground/50'
            )}
          >
            {title}
          </h3>
          <ChevronDown
            className={cn(
              'h-3 w-3 text-muted-foreground/50 transition-transform',
              open ? '' : '-rotate-90'
            )}
          />
        </button>
      </div>
      {open && (
        <fieldset
          disabled={disabled || !enabled}
          className={cn('px-4 py-3 space-y-3 bg-card/50', (!enabled || disabled) && 'opacity-40')}
        >
          {children}
        </fieldset>
      )}
    </div>
  );
}

/// The tier a download starts at when nothing asks. Until now these three services
/// had no control anywhere in Settings — only the per-download dialog.
function DefaultQualityRow({
  platform,
  value,
  onChange,
}: {
  platform: string;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <Row
      label="Default download quality"
      help="Starting tier for this service. The quality dialog overrides it per download."
    >
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger className="w-64">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {(QUALITY_OPTIONS[platform] ?? []).map((o) => (
            <SelectItem key={o.value} value={o.value}>
              {o.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Row>
  );
}

function Check2({
  id,
  label,
  help,
  checked,
  onChange,
  disabled,
}: {
  id: string;
  label: string;
  help?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <div className={cn('flex items-start gap-2 py-0.5', disabled && 'opacity-50')}>
      <Checkbox
        id={id}
        checked={!!checked}
        disabled={disabled}
        onCheckedChange={(v) => onChange(!!v)}
        className="mt-0.5"
      />
      <div>
        <Label htmlFor={id} className="font-normal cursor-pointer">
          {label}
        </Label>
        {help && <p className="text-xs text-muted-foreground">{help}</p>}
      </div>
    </div>
  );
}
type Bound = { s: Partial<Settings>; set: SettingsSetter };

const FieldCtx = createContext<Bound>({ s: {}, set: () => {} });

type KeysOf<T> = {
  [K in keyof Settings]-?: NonNullable<Settings[K]> extends T ? K : never;
}[keyof Settings];
function Chk({
  k,
  label,
  help,
  disabled,
  dflt,
}: {
  k: KeysOf<boolean>;
  label: string;
  help?: string;
  disabled?: boolean;
  dflt?: boolean;
}) {
  const { s, set } = useContext(FieldCtx);
  return (
    <Check2
      id={k}
      label={label}
      help={help}
      disabled={disabled}
      checked={s[k] ?? dflt ?? false}
      onChange={(v) => set(k, v)}
    />
  );
}
function Txt({
  k,
  label,
  help,
  disabled,
  placeholder,
  type,
  fallback,
  w,
}: {
  k: KeysOf<string>;
  label: string;
  help?: string;
  disabled?: boolean;
  placeholder?: string;
  type?: 'password' | 'number';
  fallback?: string;
  w?: string;
}) {
  const { s, set } = useContext(FieldCtx);
  return (
    <Row label={label} help={help} disabled={disabled}>
      <Input
        type={type}
        className={w}
        placeholder={placeholder}
        value={(s[k] ?? '') || fallback || ''}
        onChange={(e) => set(k, e.target.value)}
      />
    </Row>
  );
}
function Pth({
  k,
  label,
  help,
  placeholder,
  on,
}: {
  k: KeysOf<string>;
  label: string;
  help?: string;
  placeholder?: string;
  on: (key: keyof Settings) => void;
}) {
  const { s, set } = useContext(FieldCtx);
  return (
    <Row label={label} help={help}>
      <div className="flex gap-2">
        <Input
          value={s[k] ?? ''}
          placeholder={placeholder}
          onChange={(e) => set(k, e.target.value.replace(/^["']|["']$/g, ''))}
        />
        <Button variant="outline" size="sm" onClick={() => on(k)}>
          Browse
        </Button>
      </div>
    </Row>
  );
}
function Num({
  k,
  label,
  help,
  disabled,
  fallback,
  w,
  min,
  max,
  step,
}: {
  k: KeysOf<number>;
  label: string;
  help?: string;
  disabled?: boolean;
  fallback?: number;
  w?: string;
  min?: number;
  max?: number;
  step?: number;
}) {
  const { s, set } = useContext(FieldCtx);
  return (
    <Row label={label} help={help} disabled={disabled}>
      <Input
        type="number"
        className={w}
        min={min}
        max={max}
        step={step}
        value={s[k] ?? fallback ?? 0}
        onChange={(e) => set(k, Number(e.target.value))}
      />
    </Row>
  );
}
function Sel({
  k,
  label,
  help,
  disabled,
  options,
  fallback,
  placeholder,
  w = 'w-64',
}: {
  k: KeysOf<string>;
  label: string;
  help?: string;
  disabled?: boolean;
  options: readonly (readonly [string, string])[];
  fallback?: string;
  placeholder?: string;
  w?: string;
}) {
  const { s, set } = useContext(FieldCtx);
  return (
    <Row label={label} help={help} disabled={disabled}>
      <Select value={(s[k] ?? '') || fallback || ''} onValueChange={(v) => set(k, v)}>
        <SelectTrigger className={w}>
          <SelectValue placeholder={placeholder} />
        </SelectTrigger>
        <SelectContent>
          {options.map(([value, text]) => (
            <SelectItem key={value} value={value}>
              {text}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Row>
  );
}

/// What every tab body is handed. The tabs need different subsets of it, so they take
/// the whole context rather than each growing its own prop list.
type TabContext = {
  s: Partial<Settings>;
  set: SettingsSetter;
  browse: (key: keyof Settings) => void;
  browseFile: (key: keyof Settings) => void;
};

/// A tab, including how to render it.
///
/// The registry was declarative but the bodies were dispatched by a hand-written
/// `activeTab === 'x' &&` chain that nothing linked back to it, so a new tab could be
/// listed in the sidebar and silently render nothing.
type SettingsTab = {
  id: string;
  label: string;
  icon: React.ElementType | null;
  platform: string | null;
  Body: (ctx: TabContext) => React.ReactNode;
};

/// A gated service tab: `id` doubles as the `enabledServices` key.
const svcTab = (id: string, label: string, Body: SettingsTab['Body']): SettingsTab => ({
  id,
  label,
  icon: null,
  platform: id,
  Body,
});

const GENERAL_TAB: SettingsTab = {
  id: 'general',
  label: 'General',
  icon: SlidersHorizontal,
  platform: null,
  Body: GeneralTab,
};

const GENERIC_TAB: SettingsTab = {
  id: 'ytdlp',
  label: 'YT-DLP',
  icon: YtDlpIcon,
  platform: null,
  Body: YtDlpTab,
};

const DEVELOPER_TABS: SettingsTab[] = [
  { id: 'apikeys', label: 'API Keys', icon: KeyRound, platform: null, Body: ApiKeysTab },
];

const BACKEND_TABS: SettingsTab[] = [
  { id: 'orpheusdl', label: 'OrpheusDL', icon: BoomBox, platform: null, Body: OrpheusDLTab },
];

const ALWAYS_SERVICE_TABS: SettingsTab[] = [
  svcTab('youtube', 'YouTube', YouTubeTab),
  svcTab('ytmusic', 'YT Music', YtMusicTab),
];

const SERVICE_TABS: SettingsTab[] = [
  svcTab('deezer', 'Deezer', DeezerTab),
  svcTab('qobuz', 'Qobuz', QobuzTab),
  svcTab('tidal', 'Tidal', TidalTab),
  svcTab('spotify', 'Spotify', SpotifyTab),
  svcTab('applemusic', 'Apple Music', AppleMusicTab),
];

const ALL_TABS: SettingsTab[] = [
  GENERAL_TAB,
  GENERIC_TAB,
  ...ALWAYS_SERVICE_TABS,
  ...SERVICE_TABS,
  ...DEVELOPER_TABS,
  ...BACKEND_TABS,
];

function visibleTabGroups(enabled: string[]): { label: string | null; tabs: SettingsTab[] }[] {
  const gated = SERVICE_TABS.filter((t) => !!t.platform && enabled.includes(t.platform));
  return [
    { label: null, tabs: [GENERAL_TAB] },
    { label: 'Services', tabs: [...ALWAYS_SERVICE_TABS, ...gated] },
    { label: 'Developer', tabs: DEVELOPER_TABS },
    { label: 'Backends', tabs: [GENERIC_TAB, ...BACKEND_TABS] },
  ];
}

export default function SettingsPage() {
  const location = useLocation();
  const initialTab = new URLSearchParams(location.search).get('tab') || 'general';
  const [activeTab, setActiveTab] = useState(initialTab);

  useEffect(() => {
    const tab = new URLSearchParams(location.search).get('tab');
    if (!tab) return;
    setActiveTab(tab);
  }, [location.search]);

  const [settings, setSettings] = useState<Partial<Settings>>({});
  const [loading, setLoading] = useState(true);
  const [saveState, setSaveState] = useState<'idle' | 'saving' | 'saved'>('idle');
  const saveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const savedTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const isFirstLoad = useRef(true);
  const setTheme = useThemeStore((s) => s.setTheme);

  useEffect(() => {
    tauriAPI.settings
      .get()
      .then((data) => {
        if (data) setSettings(data);
      })
      .catch((err) => {
        logError('settings', 'Failed to load settings', errorDetail(err));
      })
      .finally(() => {
        setLoading(false);
        setTimeout(() => {
          isFirstLoad.current = false;
        }, 50);
      });
  }, []);

  useEffect(() => {
    if (isFirstLoad.current || loading) return;
    clearTimeout(saveTimer.current);
    clearTimeout(savedTimer.current);
    setSaveState('saving');

    saveTimer.current = setTimeout(async () => {
      try {
        await tauriAPI.settings.set(settings as Settings);
        invalidateSettingsDerived();
        if (settings.theme) setTheme(resolveThemePreference(settings.theme as ThemePreference));
        setSaveState('saved');
        savedTimer.current = setTimeout(() => setSaveState('idle'), 2000);
      } catch (err) {
        logError('settings', 'Failed to save settings', errorDetail(err));
        setSaveState('idle');
      }
    }, 900);

    return () => {
      clearTimeout(saveTimer.current);
      clearTimeout(savedTimer.current);
    };
  }, [settings, loading, setTheme]);

  useEffect(() => {
    if (loading) return;
    const svc = SERVICE_TABS.find((t) => t.id === activeTab);
    if (!svc?.platform) return;
    setSettings((prev) => {
      const cur = prev.enabledServices ?? [];
      if (cur.includes(svc.platform as string)) return prev;
      return { ...prev, enabledServices: [...cur, svc.platform as string] };
    });
  }, [activeTab, loading]);

  const set: SettingsSetter = useCallback(
    (key, value) => setSettings((prev) => ({ ...prev, [key]: value })),
    []
  );

  const fieldCtx = useMemo(() => ({ s: settings, set }), [settings, set]);

  const browseFolder = async (key: keyof Settings) => {
    const folder = await tauriAPI.settings.openFolder();
    if (folder) set(key, folder);
  };

  const browseFile = async (key: keyof Settings) => {
    const file = await tauriAPI.settings.openFile();
    if (file) set(key, file);
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const enabledServices = settings.enabledServices ?? [];
  const tabGroups = visibleTabGroups(enabledServices);
  const disabledServices = SERVICE_TABS.filter(
    (t) => !!t.platform && !enabledServices.includes(t.platform)
  );

  const enableService = (platform: string) => {
    setSettings((prev) => {
      const cur = prev.enabledServices ?? [];
      if (cur.includes(platform)) return prev;
      return { ...prev, enabledServices: [...cur, platform] };
    });
    const tab = SERVICE_TABS.find((t) => t.platform === platform);
    if (tab) setActiveTab(tab.id);
  };

  const disableService = (platform: string) => {
    setSettings((prev) => ({
      ...prev,
      enabledServices: (prev.enabledServices ?? []).filter((p) => p !== platform),
    }));
    setActiveTab('general');
  };

  const currentTab =
    ALL_TABS.find((t) => t.id === activeTab) ??
    (tabGroups.flatMap((g) => g.tabs).find((t) => t.id === activeTab) || GENERAL_TAB);
  const TabIcon = currentTab.icon;
  const tabPlatform = currentTab.platform;
  const currentIsGatedService =
    !!tabPlatform && SERVICE_TABS.some((t) => t.platform === tabPlatform);

  return (
    <div className="flex h-full min-h-0">
      <div className="w-52 border-r border-border shrink-0 flex flex-col bg-card/30">
        <div className="p-2.5 space-y-0.5 flex-1 overflow-y-auto">
          {tabGroups.map((group, gi) => (
            <div key={gi} className={gi > 0 ? 'pt-2' : ''}>
              {group.label && (
                <p className="px-3 pb-1.5 pt-0.5 text-[10px] font-semibold text-muted-foreground/50 uppercase tracking-widest">
                  {group.label}
                </p>
              )}
              {group.tabs.map((t) => {
                const Icon = t.icon;
                return (
                  <button
                    key={t.id}
                    onClick={() => setActiveTab(t.id)}
                    className={cn(
                      'w-full flex items-center gap-2.5 px-3 py-2 rounded-lg text-sm transition-colors text-left',
                      activeTab === t.id
                        ? 'bg-primary text-primary-foreground font-medium'
                        : 'text-muted-foreground hover:text-foreground hover:bg-muted/60'
                    )}
                  >
                    {t.platform ? (
                      <PlatformIcon platform={t.platform} size={14} />
                    ) : (
                      Icon && <Icon className="h-3.5 w-3.5 shrink-0" />
                    )}
                    {t.label}
                  </button>
                );
              })}
            </div>
          ))}

          {disabledServices.length > 0 && (
            <div className="pt-2">
              <Select value="" onValueChange={(v) => enableService(v)}>
                <SelectTrigger className="w-full h-9 text-sm text-muted-foreground border-dashed">
                  <span className="flex items-center gap-2">
                    <Plus className="h-3.5 w-3.5 shrink-0" />
                    Add a service
                  </span>
                </SelectTrigger>
                <SelectContent>
                  {disabledServices.map((t) => (
                    <SelectItem key={t.id} value={t.platform as string}>
                      <span className="flex items-center gap-2">
                        <PlatformIcon platform={t.platform as string} size={14} />
                        {t.label}
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
        </div>

        <div className="px-4 py-3 border-t border-border h-11 flex items-center">
          <AnimatePresence mode="wait">
            {saveState !== 'idle' && (
              <motion.div
                key={saveState}
                initial={{ opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
                className="flex items-center gap-1.5 text-xs"
              >
                {saveState === 'saving' && (
                  <>
                    <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />
                    <span className="text-muted-foreground">Saving…</span>
                  </>
                )}
                {saveState === 'saved' && (
                  <>
                    <Check className="h-3 w-3 text-emerald-500" />
                    <span className="text-emerald-500 font-medium">Saved</span>
                  </>
                )}
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>

      <div className="flex-1 flex flex-col min-h-0">
        <div className="flex items-center gap-2.5 px-6 py-4 border-b border-border shrink-0 bg-card/20">
          {tabPlatform ? (
            <PlatformIcon platform={tabPlatform} size={16} />
          ) : (
            TabIcon && <TabIcon className="h-4 w-4 text-muted-foreground" />
          )}
          <h1 className="text-sm font-semibold tracking-tight">{currentTab.label}</h1>
          {tabPlatform && (
            <div className="ml-auto flex items-center gap-2">
              {currentIsGatedService && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 text-xs text-muted-foreground hover:text-foreground"
                  onClick={() => disableService(tabPlatform)}
                >
                  <X className="h-3.5 w-3.5 mr-1" />
                  Remove
                </Button>
              )}
            </div>
          )}
        </div>

        <div className="flex-1 overflow-y-auto p-6 space-y-4">
          <FieldCtx.Provider value={fieldCtx}>
            <currentTab.Body s={settings} set={set} browse={browseFolder} browseFile={browseFile} />
          </FieldCtx.Provider>
        </div>
      </div>
    </div>
  );
}

function FormatGuide() {
  const [open, setOpen] = useState(false);

  const trackVars = [
    ['{title}', 'Track title', 'One More Time'],
    ['{artist}', 'Primary performing artist(s)', 'Daft Punk'],
    ['{albumartist}', 'Album-level artist (may differ from track artist)', 'Daft Punk'],
    ['{album}', 'Album name', 'Discovery'],
    ['{year}', 'Release year (4 digits)', '2001'],
    ['{date}', 'Full release date', '2001-02-26'],
    ['{tracknumber}', 'Track number as-is', '1'],
    ['{tracknumber:02}', 'Track number zero-padded to 2 digits', '01'],
    ['{tracktotal}', 'Total tracks in the album', '14'],
    ['{discnumber}', 'Disc number', '1'],
    ['{disctotal}', 'Total discs', '1'],
    ['{genre}', 'Primary genre from the service', 'Electronic'],
    ['{isrc}', 'ISRC code', 'FRZ019800099'],
    ['{explicit}', '" (Explicit)" if the track is flagged, else empty', ' (Explicit)'],
    ['{label}', 'Record label', 'Virgin Records'],
    ['{composer}', 'Composer / songwriter name(s)', 'Thomas Bangalter'],
    ['{quality}', 'Bit depth and sample rate (no format prefix)', '24-bit ⁄ 192kHz'],
    ['{format}', 'Container format — use with {quality} for full label', 'FLAC'],
  ];

  const folderOnlyVars = [
    ['{albumartist}', 'Album artist — use this for the top-level artist folder', 'Daft Punk'],
    ['{artist}', 'Alias for {albumartist}', 'Daft Punk'],
    ['{album}', 'Album name', 'Discovery'],
    ['{year}', 'Release year', '2001'],
    ['{genre}', 'Album genre', 'Electronic'],
    ['{label}', 'Record label', 'Virgin Records'],
    ['{quality}', 'Bit depth and sample rate', '24-bit ⁄ 192kHz'],
    ['{format}', 'Container format', 'FLAC'],
  ];

  return (
    <div className="rounded-md border border-border overflow-hidden text-xs">
      <button
        className="w-full flex items-center justify-between px-3 py-2 bg-muted/40 hover:bg-muted/60 transition-colors text-left"
        onClick={() => setOpen((o) => !o)}
      >
        <span className="font-medium text-foreground/80">Format variable reference</span>
        <ChevronDown
          className={`h-3.5 w-3.5 text-muted-foreground transition-transform ${open ? 'rotate-180' : ''}`}
        />
      </button>

      {open && (
        <div className="p-3 space-y-4 bg-muted/20">
          <div>
            <p className="font-semibold text-foreground mb-1.5">Track filename variables</p>
            <table className="w-full border-collapse">
              <thead>
                <tr className="text-muted-foreground border-b border-border">
                  <th className="text-left pb-1 pr-4 font-medium w-44">Variable</th>
                  <th className="text-left pb-1 pr-4 font-medium">Description</th>
                  <th className="text-left pb-1 font-medium">Example output</th>
                </tr>
              </thead>
              <tbody>
                {trackVars.map(([v, desc, ex]) => (
                  <tr key={v} className="border-b border-border/40 last:border-0">
                    <td className="py-0.5 pr-4">
                      <code className="text-foreground bg-muted px-1 rounded">{v}</code>
                    </td>
                    <td className="py-0.5 pr-4 text-muted-foreground">{desc}</td>
                    <td className="py-0.5 text-muted-foreground/70">{ex}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div>
            <p className="font-semibold text-foreground mb-1.5">Album folder variables</p>
            <p className="text-muted-foreground mb-1.5">
              Folder names accept album-level variables only — these eight. A bracketed segment
              whose variables are all empty is dropped, so{' '}
              <code className="text-foreground bg-muted px-1 rounded">
                {'{albumartist} - {album} ({year})'}
              </code>{' '}
              loses the empty <code className="text-foreground bg-muted px-1 rounded">()</code> when
              a release has no date.
            </p>
            <table className="w-full border-collapse">
              <thead>
                <tr className="text-muted-foreground border-b border-border">
                  <th className="text-left pb-1 pr-4 font-medium w-44">Variable</th>
                  <th className="text-left pb-1 pr-4 font-medium">Tip</th>
                  <th className="text-left pb-1 font-medium">Example</th>
                </tr>
              </thead>
              <tbody>
                {folderOnlyVars.map(([v, tip, ex]) => (
                  <tr key={v} className="border-b border-border/40 last:border-0">
                    <td className="py-0.5 pr-4">
                      <code className="text-foreground bg-muted px-1 rounded">{v}</code>
                    </td>
                    <td className="py-0.5 pr-4 text-muted-foreground">{tip}</td>
                    <td className="py-0.5 text-muted-foreground/70">{ex}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div>
            <p className="font-semibold text-foreground mb-1">Zero-padding syntax</p>
            <p className="text-muted-foreground">
              Append <code className="text-foreground bg-muted px-1 rounded">:N</code> to any
              numeric variable to zero-pad it to N digits.{' '}
              <code className="text-foreground bg-muted px-1 rounded">{'{tracknumber:02}'}</code> →{' '}
              <code>01</code>, <code>02</code> … <code>14</code> ·{' '}
              <code className="text-foreground bg-muted px-1 rounded">{'{tracknumber:03}'}</code> →{' '}
              <code>001</code>
            </p>
          </div>

          <div>
            <p className="font-semibold text-foreground mb-1.5">Preset examples</p>
            <div className="space-y-1">
              {[
                [
                  'Standard',
                  '{albumartist} - {album} ({year})',
                  '{tracknumber:02}. {artist} - {title}{explicit}',
                ],
                [
                  'With genre',
                  '{genre}/{albumartist} - {album} ({year})',
                  '{tracknumber:02}. {title}{explicit}',
                ],
                [
                  'Label/Year',
                  '{label}/{year} - {albumartist} - {album}',
                  '{tracknumber:02}. {artist} - {title}',
                ],
                [
                  'Disc aware',
                  '{albumartist} - {album} ({year})',
                  '{discnumber}-{tracknumber:02}. {artist} - {title}',
                ],
                ['Minimal', '{albumartist}/{album}', '{tracknumber:02}. {title}'],
              ].map(([name, folder, track]) => (
                <div key={name} className="rounded bg-muted/50 px-2 py-1.5">
                  <span className="font-medium text-foreground">{name}</span>
                  <div className="text-muted-foreground mt-0.5">
                    <span className="text-foreground/50">Folder: </span>
                    <code>{folder}</code>
                  </div>
                  <div className="text-muted-foreground">
                    <span className="text-foreground/50">Track: </span>
                    <code>{track}</code>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/**
 * Which station directories the Radio page asks.
 *
 * The same list has chips on the Radio page itself — this is the canonical place
 * to find it, that is the place you are standing when you want to change it.
 * The user's own stations are not listed: they are data, not a feed.
 */
function RadioDirectories({ s, set }: { s: Partial<Settings>; set: SettingsSetter }) {
  const { sources, isPending, isError } = useRadioSources();
  const enabled = s.radioDirectorySources ?? [];
  const toggleable = sources.filter((d) => d.toggleable);

  if (isPending) {
    return <p className="py-1 text-xs text-muted-foreground">Loading directories…</p>;
  }
  if (isError || toggleable.length === 0) {
    return (
      <p className="py-1 text-xs text-muted-foreground">No station directories are available.</p>
    );
  }
  return (
    <>
      {toggleable.map((directory) => (
        <Check2
          key={directory.id}
          id={`radio-source-${directory.id}`}
          label={directory.label}
          checked={enabled.includes(directory.id)}
          onChange={(on) =>
            set(
              'radioDirectorySources',
              on ? [...enabled, directory.id] : enabled.filter((id) => id !== directory.id)
            )
          }
        />
      ))}
    </>
  );
}

function GeneralTab({
  s,
  set,
  browse,
}: {
  s: Partial<Settings>;
  set: SettingsSetter;
  browse: (key: keyof Settings) => void;
}) {
  return (
    <>
      <Section title="Appearance">
        <Sel k="theme" label="Theme" options={THEME_OPTS} fallback="auto" w="w-40" />
      </Section>

      <Section title="Playback">
        <Row
          label="Volume levelling"
          help="Use the ReplayGain values in your files so tracks play at a consistent loudness"
        >
          <Select
            value={s.replaygain_mode ?? 'off'}
            onValueChange={(v) => set('replaygain_mode', v)}
          >
            <SelectTrigger className="w-40">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="off">Off</SelectItem>
              <SelectItem value="track">Per track</SelectItem>
              <SelectItem value="album">Per album</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Chk
          k="crossfade_enabled"
          label="Crossfade between tracks"
          help="Smoothly blend the end of one track into the beginning of the next"
        />
        {s.crossfade_enabled && (
          <Row label="Crossfade duration" help="Seconds of overlap between tracks">
            <Select
              value={String(s.crossfade_duration ?? 6)}
              onValueChange={(v) => set('crossfade_duration', Number(v))}
            >
              <SelectTrigger className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="2">2 seconds</SelectItem>
                <SelectItem value="4">4 seconds</SelectItem>
                <SelectItem value="6">6 seconds</SelectItem>
                <SelectItem value="8">8 seconds</SelectItem>
                <SelectItem value="10">10 seconds</SelectItem>
                <SelectItem value="12">12 seconds</SelectItem>
              </SelectContent>
            </Select>
          </Row>
        )}
      </Section>

      <Section title="Radio">
        <RadioDirectories s={s} set={set} />
      </Section>

      <Section title="Downloads">
        <Pth k="downloadLocation" label="Download Location" on={browse} />
        <Chk
          k="createPlatformSubfolders"
          label="Create platform subfolders"
          help="Organise downloads into per-platform folders (Spotify/, Tidal/, etc.)"
        />
        <Chk
          k="pipeline_overwrite"
          label="Overwrite files already on disk"
          help="Off: a track whose file is already in the destination folder is kept and skipped."
        />
        <Chk
          k="native_skip_existing"
          label="Skip tracks already downloaded"
          help="Skips a track this service already fetched. Use “Force re-download” in the quality dialog to override once."
          dflt
        />
      </Section>

      <Section title="Tools">
        <Chk k="orpheusDL" label="Prioritize OrpheusDL" />
        <Check2 id="autoUpdate" label="Auto update on launch" checked={s.autoUpdate ?? false} onChange={(v) => set('autoUpdate', v)} />
      </Section>

      <Section title="File Naming (Native downloads)">
        <Txt
          k="filepaths_folder_format"
          label="Album folder"
          fallback="{albumartist} - {album} ({year})"
        />
        <Txt
          k="filepaths_track_format"
          label="Track filename"
          fallback="{tracknumber:02}. {artist} - {title}{explicit}"
        />
        <FormatGuide />
        <Num
          k="filepaths_truncate_to"
          label="Max filename length"
          help="0 = unlimited"
          fallback={120}
          w="w-28"
        />
        <Chk
          k="filepaths_restrict_characters"
          label="Restrict special characters in filenames"
          help={
            'Replaces < > : " \\ | ? * with lookalike characters. / and control characters are always replaced so a track title can never create a folder.'
          }
          dflt
        />
        <Chk
          k="disc_subdirectories"
          label="Create disc subdirectories for multi-disc albums"
          help="Creates Disc 1/, Disc 2/, etc. subfolders when an album spans multiple discs"
          dflt
        />
        <Chk
          k="save_playlist_file"
          label="Save an .m3u8 next to downloaded albums and playlists"
          help="Lists the tracks that actually landed, with paths relative to the folder so it stays portable."
          dflt
        />
      </Section>

      <Section title="Artwork & Metadata (Native downloads)">
        <Chk k="embed_cover" label="Embed cover art in audio file" dflt />
        <Chk
          k="save_cover"
          label="Save a cover file next to each track"
          help="Writes <track name>.cover.jpg beside every track. Useful for playlist folders, which hold unrelated albums."
        />
        <Chk
          k="save_album_cover"
          label="Save cover.jpg per album folder"
          help="What most players and library scanners look for. Independent of the per-track file above."
        />
        <Row
          label="Cover art size"
          help="Longest edge in pixels; the nearest size the service publishes is used."
        >
          <Select
            value={String(s.pipeline_cover_size ?? 1280)}
            onValueChange={(v) => set('pipeline_cover_size', Number(v))}
          >
            <SelectTrigger className="w-40">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="1280">1280 px</SelectItem>
              <SelectItem value="1080">1080 px</SelectItem>
              <SelectItem value="640">640 px</SelectItem>
              <SelectItem value="320">320 px</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Chk
          k="meta_exclude_tags_check"
          label="Leave some tags out of downloaded files"
          help="Everything else is still written. Useful when a tag your player derives itself keeps getting overwritten."
        />
        {s.meta_exclude_tags_check && (
          <Txt
            k="excluded_tags"
            label="Tags to skip"
            help="Comma separated. Accepted names: title, artist, album, album_artist, genre, comment, lyrics, isrc, upc, copyright, label, composer, conductor, performer, lyricist, producer, engineer, mixer, description, grouping, date, tracknumber, discnumber."
            placeholder="comment, composer"
          />
        )}
      </Section>

      <Section title="Lyrics (native downloaders)">
        <Chk
          k="embed_lyrics"
          label="Embed lyrics in the audio file"
          help="Writes plain lyrics into the track's own tag. YouTube and YT Music use yt-dlp's subtitle options instead."
          dflt
        />
        <Chk
          k="save_lrc_files"
          label="Save synced lyrics alongside each track"
          help="Writes a .lrc/.ttml file next to the audio, so lyrics work offline. The source service is tried first."
        />
        {s.save_lrc_files && (
          <Check2
            id="native_synced_lyrics_format"
            label="Use TTML when the service gives word-by-word timings"
            help="LRC is written otherwise, and almost every player reads it. One file per track either way."
            checked={s.native_synced_lyrics_format === 'ttml'}
            onChange={(v) => set('native_synced_lyrics_format', v ? 'ttml' : 'lrc')}
          />
        )}
        <p className="pt-2 text-xs font-medium text-muted-foreground">
          Fallback sources when the service has none
        </p>
        <p className="pb-1 text-xs text-muted-foreground">
          These apply to every service, not just the one they are named after.
        </p>
        <Chk
          k="deezer_lrc_public_fallback"
          label="Deezer public lyrics"
          help="Word-synced, needs no ARL; a saved ARL widens the results."
          dflt
        />
        <Chk
          k="lyrics_fallback_lrclib"
          label="LRCLIB (community lyrics)"
          help="Community line-synced lyrics, matched by title, artist and duration."
          dflt
        />
      </Section>

      <Section title="Conversion (native downloaders)">
        <Chk
          k="conversion_check"
          label="Convert after download"
          help="Re-encodes finished downloads to change format, downsample hi-res, or hit a target bitrate."
        />
        {s.conversion_check && (
          <>
            <Sel
              k="conversion_codec"
              label="Output format"
              help="Tags and art are written after the re-encode. A lossless target cannot recover a lossy source. Matching files and Atmos are skipped."
              options={CONVERSION_CODEC_OPTS}
              fallback="FLAC"
              w="w-40"
            />
            {['MP3', 'AAC', 'OPUS', 'VORBIS'].includes(
              (s.conversion_codec || 'FLAC').toUpperCase()
            ) && (
              <Row label="Bitrate (kbps)">
                <Input
                  type="number"
                  min={32}
                  max={512}
                  step={8}
                  value={s.conversion_lossy_bitrate ?? 320}
                  onChange={(e) => set('conversion_lossy_bitrate', Number(e.target.value))}
                  onBlur={(e) => {
                    const n = Number(e.target.value);
                    set(
                      'conversion_lossy_bitrate',
                      Number.isFinite(n) && n > 0 ? Math.min(512, Math.max(32, Math.round(n))) : 320
                    );
                  }}
                  className="w-28"
                />
              </Row>
            )}
            <Row label="Max sampling rate" help="Downsample above this.">
              <Select
                value={
                  s.conversion_sampling_rate != null
                    ? String(s.conversion_sampling_rate)
                    : 'original'
                }
                onValueChange={(v) =>
                  set('conversion_sampling_rate', v === 'original' ? null : Number(v))
                }
              >
                <SelectTrigger className="w-40">
                  <SelectValue placeholder="Original" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="original">Original</SelectItem>
                  <SelectItem value="44100">44.1 kHz</SelectItem>
                  <SelectItem value="48000">48 kHz</SelectItem>
                  <SelectItem value="88200">88.2 kHz</SelectItem>
                  <SelectItem value="96000">96 kHz</SelectItem>
                  <SelectItem value="176400">176.4 kHz</SelectItem>
                  <SelectItem value="192000">192 kHz</SelectItem>
                </SelectContent>
              </Select>
            </Row>
            <Row label="Max bit depth" help="Reduce above this.">
              <Select
                value={s.conversion_bit_depth != null ? String(s.conversion_bit_depth) : 'original'}
                onValueChange={(v) =>
                  set('conversion_bit_depth', v === 'original' ? null : Number(v))
                }
              >
                <SelectTrigger className="w-40">
                  <SelectValue placeholder="Original" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="original">Original</SelectItem>
                  <SelectItem value="16">16-bit</SelectItem>
                  <SelectItem value="24">24-bit</SelectItem>
                </SelectContent>
              </Select>
            </Row>
          </>
        )}
      </Section>
    </>
  );
}

const SPONSORBLOCK_CATEGORIES = [
  'all',
  'default',
  'sponsor',
  'selfpromo',
  'interaction',
  'intro',
  'outro',
  'preview',
  'filler',
  'music_offtopic',
  'hook',
  'poi_highlight',
  'chapter',
];

function CategoryMultiSelect({
  id,
  value,
  onChange,
}: {
  id: string;
  value: string;
  onChange: (v: string) => void;
}) {
  const selected = new Set(
    value
      .split(',')
      .map((c) => c.trim())
      .filter(Boolean)
  );
  const toggle = (cat: string, on: boolean) => {
    const next = new Set(selected);
    if (on) next.add(cat);
    else next.delete(cat);
    onChange(Array.from(next).join(','));
  };
  return (
    <div className="flex flex-wrap gap-x-4 gap-y-1.5">
      {SPONSORBLOCK_CATEGORIES.map((cat) => (
        <div key={cat} className="flex items-center gap-1.5">
          <Checkbox
            id={`${id}-${cat}`}
            checked={selected.has(cat)}
            onCheckedChange={(v) => toggle(cat, !!v)}
          />
          <Label htmlFor={`${id}-${cat}`} className="font-normal text-xs cursor-pointer">
            {cat}
          </Label>
        </div>
      ))}
    </div>
  );
}

function SubtitlesSection() {
  return (
    <Section title="Subtitles">
      <Chk k="add_subtitle_to_file" label="Embed subtitles" />
      <Txt k="sub_langs" label="Subtitle languages" placeholder="en.*,-live_chat" w="w-64" />
      <Chk k="write_auto_subs" label="Include auto-generated captions" help="--write-auto-subs" />
      <Chk k="convert_subs_srt" label="Convert subtitles to SRT" help="--convert-subs srt" />
    </Section>
  );
}

function SponsorBlockSection({ s, set }: { s: Partial<Settings>; set: SettingsSetter }) {
  return (
    <Section title="SponsorBlock">
      <Chk k="no_sponsorblock" label="Disable SponsorBlock entirely" />
      {!s.no_sponsorblock && (
        <>
          <Row label="Mark categories" help="Categories to mark as chapters.">
            <CategoryMultiSelect
              id="sb-mark"
              value={s.sponsorblock_mark || 'all'}
              onChange={(v) => set('sponsorblock_mark', v)}
            />
          </Row>
          <Row label="Remove categories" help="Categories to cut out of the file.">
            <CategoryMultiSelect
              id="sb-remove"
              value={s.sponsorblock_remove || ''}
              onChange={(v) => set('sponsorblock_remove', v)}
            />
          </Row>
          <Txt
            k="sponsorblock_chapter_title"
            label="Chapter title template"
            fallback="[SponsorBlock]: %(category_names)l"
          />
          <Txt k="sponsorblock_api_url" label="API URL" fallback="https://sponsor.ajay.app" />
        </>
      )}
    </Section>
  );
}

function YouTubeExtractionSection({ s }: { s: Partial<Settings> }) {
  return (
    <Section title="YouTube Extraction">
      <Txt
        k="player_client"
        label="Player clients"
        help="--extractor-args youtube:player_client= — leave empty to use yt-dlp's default (recommended). Avoid 'tv' (DRM-experiment) unless you have a PO-token provider."
        placeholder="default (yt-dlp's choice)"
        w="w-64"
      />
      <Txt
        k="ejs_remote_components"
        label="JS challenge solver"
        help="--remote-components — fetches yt-dlp's EJS n-sig solver so formats resolve (required for cookie-authenticated downloads). Clear to disable; 'ejs:npm' uses the runtime's package manager."
        placeholder="ejs:github"
        w="w-64"
      />
      <Chk
        k="pot_provider_enabled"
        label="Enable PO-token provider"
        help="Advanced — only for members-only / age-gated formats that require a PO token."
      />
      {s.pot_provider_enabled && (
        <>
          <Txt
            k="po_token"
            label="PO token"
            help="Manual PO token forwarded to the extractor."
            w="w-64"
          />
          <Chk k="pot_trace" label="PO-token debug trace" help="pot_trace=true" />
        </>
      )}
    </Section>
  );
}

function YtDlpTab({
  s,
  set,
  browseFile,
}: {
  s: Partial<Settings>;
  set: SettingsSetter;
  browseFile: (key: keyof Settings) => void;
}) {
  return (
    <>
      <Section title="Output">
        <Txt
          k="download_output_template"
          label="Output template"
          help="yt-dlp filename template"
          fallback="%(title)s.%(ext)s"
        />
        <Num k="max_downloads" label="Max downloads" help="0 = unlimited" w="w-28" />
        <Num k="max_retries" label="Max retries" fallback={5} w="w-28" />
        <Chk k="continue" label="Continue partially downloaded files" />
        <Chk
          k="no_overwrites"
          label="Skip files that already exist"
          help="Passes --no-overwrites; complements the dedup ledger."
          dflt
        />
        <Chk k="use_aria2" label="Use aria2c for downloading" />
        {s.use_aria2 && (
          <Txt
            k="aria2c_args"
            label="aria2c arguments"
            help="--downloader-args aria2c:<...>"
            fallback="-x16 -s16 -k1M"
            w="w-64"
          />
        )}
      </Section>

      <Section title="Format & Quality">
        <Num
          k="concurrent_fragments"
          label="Concurrent fragments"
          help="-N parallel DASH/HLS fragments — biggest speed win."
          fallback={4}
          min={1}
          w="w-28"
        />
        <Row label="Remux container" help="--remux-video target; None = no remux.">
          <Select
            value={s.remux_format ? s.remux_format : 'none'}
            onValueChange={(v) => set('remux_format', v === 'none' ? '' : v)}
          >
            <SelectTrigger className="w-40">
              <SelectValue placeholder="None" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">None</SelectItem>
              <SelectItem value="mp4">mp4</SelectItem>
              <SelectItem value="mkv">mkv</SelectItem>
              <SelectItem value="webm">webm</SelectItem>
              <SelectItem value="mov">mov</SelectItem>
              <SelectItem value="avi">avi</SelectItem>
              <SelectItem value="flv">flv</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Chk
          k="convert_thumbnails_jpg"
          label="Convert thumbnails to JPG"
          help="--convert-thumbnails jpg when embedding (YouTube serves webp/avif)."
          dflt
        />
        <Chk
          k="faststart_mp4"
          label="Optimize mp4 for streaming"
          help="Adds +faststart for seekable mp4 output."
          dflt
        />
      </Section>

      <Section title="Metadata">
        <Chk k="add_metadata" label="Add metadata to files" />
        <Chk k="embed_chapters" label="Embed chapters" />
      </Section>

      <Section title="Network">
        <Chk k="use_cookies" label="Use cookies from file" />
        {s.use_cookies && (
          <>
            <Pth
              k="cookies"
              label="Cookies file path"
              placeholder="Path to cookies.txt"
              on={browseFile}
            />
            <Pth
              k="ytdlp_cookies_path"
              label="yt-dlp cookies override"
              placeholder="Leave empty to use the shared cookies file"
              on={browseFile}
            />
          </>
        )}
        <Chk k="use_proxy" label="Use proxy" />
        {s.use_proxy && <Txt k="proxy_url" label="Proxy URL" />}

        <Chk k="download_speed_limit" label="Limit download speed" />
        {s.download_speed_limit && (
          <Row label="Speed limit">
            <div className="flex gap-2 items-center">
              <Input
                type="number"
                value={s.speed_limit_value ?? 0}
                onChange={(e) => set('speed_limit_value', Number(e.target.value))}
                className="w-28"
              />
              <Select
                value={s.speed_limit_type || 'M'}
                onValueChange={(v) => set('speed_limit_type', v)}
              >
                <SelectTrigger className="w-24">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="K">KB/s</SelectItem>
                  <SelectItem value="M">MB/s</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </Row>
        )}

        <Num
          k="socket_timeout"
          label="Socket timeout"
          help="--socket-timeout seconds"
          fallback={15}
          min={1}
          w="w-28"
        />
        <Txt k="throttled_rate" label="Throttled rate" placeholder="100K" w="w-32" />
      </Section>

      <Section title="Resilience">
        <Txt
          k="fragment_retries"
          label="Fragment retries"
          help="--fragment-retries (number or 'infinite')."
          fallback="10"
          w="w-32"
        />
        <Txt
          k="extractor_retries"
          label="Extractor retries"
          help="--extractor-retries"
          fallback="3"
          w="w-32"
        />
        <Txt
          k="file_access_retries"
          label="File access retries"
          help="--file-access-retries; resilience against Windows AV file locks."
          fallback="3"
          w="w-32"
        />
        <Txt
          k="sleep_requests"
          label="Sleep between requests"
          help="--sleep-requests seconds (anti-throttle)."
          w="w-32"
        />
        <Txt
          k="sleep_interval"
          label="Sleep interval"
          help="--sleep-interval seconds (anti-throttle)."
          w="w-32"
        />
        <Txt
          k="max_sleep_interval"
          label="Max sleep interval"
          help="--max-sleep-interval seconds (anti-throttle)."
          w="w-32"
        />
      </Section>

      <Section title="Network — general">
        <Txt
          k="cookies_from_browser"
          label="Cookies from browser"
          help="BROWSER[:PROFILE] — e.g. chrome, firefox:default. Fixes 'Sign in to confirm' errors."
          placeholder="chrome"
          w="w-64"
        />
        <Txt
          k="geo_bypass_country"
          label="Geo-bypass country"
          help="--geo-bypass-country (2-letter code; optional)."
          placeholder="US"
          w="w-28"
        />
      </Section>

      <Section title="Authentication">
        <Chk k="use_authentication" label="Use username/password" />
        {s.use_authentication && (
          <>
            <Txt k="username" label="Username" w="w-64" />
            <Txt k="password" label="Password" type="password" w="w-64" />
          </>
        )}
      </Section>

      <Section title="Advanced">
        <Txt
          k="postprocessor_args"
          label="Postprocessor args"
          help="Raw --postprocessor-args passthrough."
        />
        <Txt
          k="extra_args"
          label="Extra yt-dlp args"
          help="Free-form arguments appended to every yt-dlp call."
        />
        <Num
          k="trim_filenames"
          label="Trim filenames"
          help="--trim-filenames length; 0 = off (avoids ENAMETOOLONG)."
          min={0}
          w="w-28"
        />
        <Chk
          k="restrict_filenames"
          label="Restrict filenames to ASCII"
          help="--restrict-filenames"
        />
        <Chk k="windows_filenames" label="Windows-safe filenames" help="--windows-filenames" />
        <Chk
          k="set_mtime"
          label="Set file modified time from source"
          help="Off passes --no-mtime."
        />
      </Section>
    </>
  );
}

function YouTubeTab({
  s,
  set,
  browseFile,
}: {
  s: Partial<Settings>;
  set: SettingsSetter;
  browseFile: (key: keyof Settings) => void;
}) {
  return (
    <>
      <Section title="Cookies">
        <Pth
          k="youtube_cookies_path"
          label="YouTube cookies override"
          help="Leave empty to use the shared cookies file from the yt-dlp tab."
          placeholder="Leave empty to use the shared cookies file"
          on={browseFile}
        />
      </Section>

      <YouTubeExtractionSection s={s} />

      <Section title="Extensions">
        <Chk k="yt_override_download_extension" label="Override YouTube video extension" />
        {s.yt_override_download_extension && (
          <Txt k="youtubeVideoExtensions" label="Video extension" fallback="mp4" w="w-28" />
        )}
      </Section>

      <SponsorBlockSection s={s} set={set} />

      <SubtitlesSection />
    </>
  );
}

function YtMusicTab({
  s,
  browseFile,
}: {
  s: Partial<Settings>;
  browseFile: (key: keyof Settings) => void;
}) {
  return (
    <>
      <Section title="Cookies">
        <Pth
          k="ytmusic_cookies_path"
          label="YouTube Music cookies override"
          help="Leave empty to use the shared cookies file from the yt-dlp tab."
          placeholder="Leave empty to use the shared cookies file"
          on={browseFile}
        />
      </Section>

      <Section title="Playback history">
        <Chk k="ytmusic_sync_playback_history" label="Sync playback history to YouTube Music" />
      </Section>

      <Section title="Extensions">
        <Chk k="ytm_override_download_extension" label="Override YouTube Music extension" />
        {s.ytm_override_download_extension && (
          <Txt k="youtubeAudioExtensions" label="Audio extension" fallback="mp3" w="w-28" />
        )}
      </Section>
    </>
  );
}

function DeezerTab({ s, set }: { s: Partial<Settings>; set: SettingsSetter }) {
  return (
    <>
      <Section title="Authentication">
        <div className="rounded-md bg-muted/50 border border-border p-3 text-sm text-muted-foreground space-y-1">
          <p className="font-medium text-foreground">How to get your ARL token</p>
          <ol className="list-decimal list-inside space-y-0.5 text-xs">
            <li>
              Open <strong>deezer.com</strong> in your browser and log in
            </li>
            <li>
              Open DevTools (F12) → Application → Cookies → <code>https://www.deezer.com</code>
            </li>
            <li>
              Copy the value of the <code>arl</code> cookie and paste it below
            </li>
          </ol>
          <p className="text-xs mt-1">
            Free accounts stream at 128 kbps MP3. Premium unlocks 320 kbps. HiFi unlocks FLAC.
          </p>
        </div>
        <Txt
          k="deezer_arl"
          label="ARL Token"
          type="password"
          placeholder="Paste your ARL cookie value here"
        />
      </Section>

      <Section title="Quality">
        <DefaultQualityRow
          platform="deezer"
          value={deezerTierOf(s.deezer_quality)}
          onChange={(v) => set('deezer_quality', DEEZER_TIER_TO_FORMAT[v] ?? 'FLAC')}
        />
      </Section>

      <Section title="Telemetry & playback history">
        <Chk k="deezer_telemetry_enabled" label="Send anti-ban telemetry" />
        <Chk k="deezer_sync_playback_history" label="Sync playback history to Deezer" />
      </Section>
    </>
  );
}

function QobuzTab({ s, set }: { s: Partial<Settings>; set: SettingsSetter }) {
  return (
    <>
      <Section title="Authentication">
        <Check2 id="qobuz_token_or_email" label="Use user ID + auth token instead of email/password"
          help="Enable if you have a user_auth_token from the Qobuz API"
          checked={!!s.qobuz_token_or_email} onChange={(v) => set('qobuz_token_or_email', v)} />
        <Row label={s.qobuz_token_or_email ? 'User ID' : 'Email'}>
          <Input value={s.qobuz_email_or_userid || ''} onChange={(e) => set('qobuz_email_or_userid', e.target.value)} />
        </Row>
        <Row label={s.qobuz_token_or_email ? 'Auth Token' : 'Password'}>
          <Input type="password" value={s.qobuz_password_or_token || ''} onChange={(e) => set('qobuz_password_or_token', e.target.value)} />
        </Row>

      <Section title="Quality">
        <DefaultQualityRow
          platform="qobuz"
          value={String(s.qobuz_quality ?? 27)}
          onChange={(v) => set('qobuz_quality', Number(v))}
        />
      </Section>

      <Section title="Options">
        <Chk k="qobuz_download_booklets" label="Download booklets (PDFs)" />
      </Section>

      <Section title="Telemetry & playback history">
        <Chk k="qobuz_telemetry_enabled" label="Send the client's session handshake" />
        <Chk k="qobuz_sync_playback_history" label="Sync playback history to Qobuz" />
      </Section>

      <Section title="Download Filters">
        <Chk k="qobuz_filters_extras" label="Exclude extras (bonus tracks, interludes)" />
        <Chk k="qobuz_non_remaster" label="Exclude non-remastered versions" />
        <Chk k="qobuz_non_studio_albums" label="Exclude non-studio albums (live, compilations)" />
        <Chk k="qobuz_non_albums" label="Exclude non-album releases (singles, EPs)" />
        <Chk k="qobuz_features" label="Exclude feature appearances" />
        <Chk k="qobuz_repeats" label="Exclude repeated albums (duplicates)" />
      </Section>

      <Section title="Advanced — App credentials override">
        <p className="text-xs text-muted-foreground leading-relaxed">
          Normally these are worked out automatically: your token carries the date it was created,
          and MediaHarbor reads the app credentials out of the Qobuz web player build that was live
          at the time. Override them here only if that fails. Leave blank to detect them.
        </p>
        <Txt
          k="qobuz_app_id"
          label="App ID"
          help="9-digit Qobuz client app_id"
          placeholder="Auto-detected"
        />
        <Txt
          k="qobuz_secrets"
          label="App Secret"
          help="32-char hex secret used to sign track URLs"
          placeholder="Auto-detected"
        />
      </Section>
    </>
  );
}

function TidalTab({ s, set }: { s: Partial<Settings>; set: SettingsSetter }) {
  const hasToken = !!s.tidal_access_token;
  const [authState, setAuthState] = useState<'idle' | 'waiting' | 'loading' | 'success' | 'error'>('idle');
  const [codeVerifier, setCodeVerifier] = useState('');
  const [redirectUrl, setRedirectUrl] = useState('');
  const [errorMsg, setErrorMsg] = useState('');

  async function startLogin() {
    if (!window.electron) return;
    setAuthState('loading');
    setErrorMsg('');
    try {
      const { codeVerifier: cv, authUrl } = await window.electron.tidalAuth.startAuth();
      setCodeVerifier(cv);
      if (authUrl) await window.electron.updates.openRelease(authUrl);
      setAuthState('waiting');
    } catch (e: unknown) {
      setErrorMsg(e instanceof Error ? e.message : 'Failed to open Tidal login');
      setAuthState('error');
    }
  }

  async function submitRedirect() {
    if (!window.electron) return;
    if (!redirectUrl.trim()) return;
    setAuthState('loading');
    setErrorMsg('');
    try {
      const tokens = await window.electron.tidalAuth.exchangeCode({ redirectUrl: redirectUrl.trim(), codeVerifier });
      Object.entries(tokens).forEach(([k, v]) => set(k as keyof Settings, v));
      setAuthState('success');
      setRedirectUrl('');
    } catch (e: unknown) {
      setErrorMsg(typeof e === 'string' ? e : (e instanceof Error ? e.message : 'Failed to exchange code'));
      setAuthState('error');
    }
  }

  function logout() {
    set('tidal_access_token', '');
    set('tidal_refresh_token', '');
    set('tidal_token_expiry', '');
    set('tidal_user_id', '');
    set('tidal_country_code', '');
    setAuthState('idle');
  }

  return (
    <>
      <Section title="Authentication">
        {hasToken ? (
          <div className="flex items-center justify-between rounded-md bg-green-500/10 border border-green-500/30 px-3 py-2">
            <div className="flex items-center gap-2 text-sm text-green-500">
              <Check className="h-4 w-4" />
              <span>{authState === 'success' ? 'Successfully logged in to Tidal!' : 'Logged in to Tidal'}</span>
              {s.tidal_user_id && <span className="text-green-500/70 text-xs">· User {s.tidal_user_id}{s.tidal_country_code ? ` (${s.tidal_country_code})` : ''}</span>}
            </div>
            <Button variant="ghost" size="sm" className="text-muted-foreground hover:text-destructive h-7 text-xs" onClick={logout}>
              Log out
            </Button>
          </div>
        ) : (
          <div className="flex items-center gap-2 rounded-md bg-amber-500/10 border border-amber-500/30 px-3 py-2 text-sm text-amber-500">
            <span className="h-2 w-2 rounded-full bg-amber-500 inline-block" />
            <span>Not logged in — Tidal playback and downloads will not work</span>
          </div>
        )}

        {authState === 'idle' || authState === 'error' ? (
          <div className="space-y-2">
            <p className="text-xs text-muted-foreground">
              Click the button below to open the Tidal login page in your browser. After logging in, Tidal will redirect you to a URL — paste that URL back here.
            </p>
            <Button onClick={startLogin} className="w-full sm:w-auto">
              {hasToken ? 'Re-authenticate with Tidal' : 'Login with Tidal'}
            </Button>
            {authState === 'error' && errorMsg && (
              <p className="text-xs text-destructive">{errorMsg}</p>
            )}
          </div>
        ) : authState === 'waiting' ? (
          <div className="space-y-3 rounded-md bg-muted/50 border border-border p-3">
            <p className="text-sm font-medium">Complete login in your browser</p>
            <ol className="text-xs text-muted-foreground list-decimal list-inside space-y-1">
              <li>A Tidal login page has opened in your browser — sign in there.</li>
              <li>After signing in, your browser will show a page that may not load (that&apos;s normal).</li>
              <li>Copy the full URL from your browser&apos;s address bar and paste it below.</li>
            </ol>
            <div className="flex gap-2">
              <Input
                value={redirectUrl}
                onChange={(e) => setRedirectUrl(e.target.value)}
                placeholder="https://tidal.com/android/login/auth?code=..."
                className="flex-1 text-xs"
                onKeyDown={(e) => e.key === 'Enter' && submitRedirect()}
              />
              <Button onClick={submitRedirect} disabled={!redirectUrl.trim()}>
                Submit
              </Button>
            </div>
            <button className="text-xs text-muted-foreground hover:text-foreground underline" onClick={() => setAuthState('idle')}>
              Cancel
            </button>
          </div>
        ) : authState === 'loading' ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            <span>Exchanging tokens…</span>
          </div>
        ) : null}
      </Section>

      <Section title="Quality">
        <DefaultQualityRow
          platform="tidal"
          value={String(s.tidal_quality ?? 3)}
          onChange={(v) => set('tidal_quality', Number(v))}
        />
      </Section>

      <ToggleSection
        title="Videos"
        enabled={s.tidal_download_videos ?? false}
        onToggle={(v) => set('tidal_download_videos', v)}
      >
        <Sel
          k="tidal_video_quality"
          label="Max resolution"
          help="The closest variant at or below this height is taken"
          options={TIDAL_VIDEO_QUALITY_OPTS}
          fallback="1080p"
          w="w-32"
        />
      </ToggleSection>

      <Section title="Telemetry & playback history">
        <Chk k="tidal_telemetry_enabled" label="Send anti-ban telemetry" />
        <Chk k="tidal_sync_playback_history" label="Sync playback history to Tidal" />
        <Txt
          k="tidal_device_model"
          label="Reported device model"
          help="Every field below must describe the same real device. Defaults are a Galaxy A16 5G; the app version is fetched automatically."
          placeholder="SM-A166B"
          w="w-64"
        />
        <Txt
          k="tidal_device_vendor"
          label="Device vendor"
          help="As Android reports it (samsung, Google, OnePlus). Must match the model."
          placeholder="samsung"
          w="w-64"
        />
        <Sel
          k="tidal_device_type"
          label="Device type"
          help="Must match the model's form factor."
          options={TIDAL_DEVICE_TYPE_OPTS}
          fallback="phone"
        />
        <Txt
          k="tidal_os_version"
          label="Android OS version"
          help="Android major version, e.g. 14."
          placeholder="14"
          w="w-64"
        />
        <Row label="Screen resolution" help="Width × height in pixels, portrait.">
          <div className="flex items-center gap-2">
            <Input
              type="number"
              value={s.tidal_screen_width ?? 1080}
              onChange={(e) => set('tidal_screen_width', Number(e.target.value))}
              placeholder="1080"
              className="w-28"
            />
            <span className="text-muted-foreground">×</span>
            <Input
              type="number"
              value={s.tidal_screen_height ?? 2340}
              onChange={(e) => set('tidal_screen_height', Number(e.target.value))}
              placeholder="2340"
              className="w-28"
            />
          </div>
        </Row>
      </Section>
    </>
  );
}

function SpotifyTab({
  s,
  set,
  browseFile,
}: {
  s: Partial<Settings>;
  set: SettingsSetter;
  browseFile: (key: keyof Settings) => void;
}) {
  const [loginStatus, setLoginStatus] = useState<'idle' | 'loading' | 'success' | 'error'>('idle');
  const [loginMessage, setLoginMessage] = useState('');
  const backend = s.spotify_downloader_backend || 'native';
  const isVotify = backend === 'votify';

  useEffect(() => {
    if (!s.spotify_cookies_path) return;
    tauriAPI.spotifyAccount
      ?.getStatus()
      .then((status) => {
        if (status?.loggedIn && status?.profile) {
          setLoginStatus('success');
          setLoginMessage(
            `Connected as ${status.profile.name || status.profile.id || 'Spotify user'}`
          );
        } else {
          setLoginStatus((prev) => (prev === 'loading' ? prev : 'idle'));
        }
      })
      .catch((err: unknown) => {
        logWarning('settings', 'Spotify status check failed', errorDetail(err));
      });
  }, [s.spotify_cookies_path]);

  const handleLogin = async () => {
    if (!s.spotify_cookies_path?.trim()) {
      setLoginStatus('error');
      setLoginMessage('Cookies file path is required. Set it in the Authentication section below.');
      return;
    }
    setLoginStatus('loading');
    setLoginMessage('Opening Spotify login…');
    try {
      const result = await tauriAPI.spotifyAccount?.login();
      setLoginStatus('success');
      setLoginMessage(`Connected as ${result?.name || result?.id || 'Spotify user'}`);
    } catch (err: unknown) {
      setLoginStatus('error');
      setLoginMessage(errorMessage(err));
    }
  };

  const handleLogout = async () => {
    try {
      await tauriAPI.spotifyAccount?.logout();
      setLoginStatus('idle');
      setLoginMessage('');
    } catch (err: unknown) {
      setLoginMessage(errorMessage(err));
    }
  };

  return (
    <>
      <Section title="Downloader">
        <Row
          label="Backend"
          help="Native needs no setup. Votify needs Python but adds podcasts, music videos, and FLAC."
        >
          <Select value={backend} onValueChange={(v) => set('spotify_downloader_backend', v)}>
            <SelectTrigger className="w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="native">Native (recommended)</SelectItem>
              <SelectItem value="votify">Votify (legacy CLI)</SelectItem>
            </SelectContent>
          </Select>
        </Row>
      </Section>

      <Section title="Spotify Account">
        <div className="rounded-md border border-blue-500/30 bg-blue-500/5 px-4 py-3 text-xs text-blue-600 dark:text-blue-400 mb-2">
          Connect your Spotify account for personalized search. Uses your cookies file from the
          Authentication section below — no extra setup needed.
        </div>
        <div className="flex items-center gap-3 pt-1">
          {loginStatus !== 'success' ? (
            <Button
              variant="outline"
              size="sm"
              onClick={handleLogin}
              disabled={loginStatus === 'loading'}
            >
              {loginStatus === 'loading' && <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />}
              {loginStatus === 'loading' ? 'Connecting…' : '🎵 Login with Spotify'}
            </Button>
          ) : (
            <Button variant="outline" size="sm" onClick={handleLogout}>
              Disconnect
            </Button>
          )}
          {loginStatus === 'success' && (
            <span className="text-xs text-green-600 dark:text-green-400 flex items-center gap-1">
              <Check className="h-3.5 w-3.5" /> {loginMessage}
            </span>
          )}
          {loginStatus === 'error' && (
            <span className="text-xs text-red-600 dark:text-red-400">{loginMessage}</span>
          )}
        </div>
      </Section>

      <Section title="Authentication">
        <Pth
          k="spotify_cookies_path"
          label="Cookies Path"
          help="Netscape format cookies file from spotify.com. Used by both backends."
          placeholder="Path to spotify.com cookies.txt"
          on={browseFile}
        />
        <Pth
          k="spotify_wvd_path"
          label="WVD Path"
          help=".wvd file for DRM decryption (required for native Spotify AAC downloads)"
          placeholder="Path to .wvd file"
          on={browseFile}
        />
        <fieldset
          disabled={!isVotify}
          className={cn(!isVotify && 'opacity-50 cursor-not-allowed space-y-3', 'space-y-3')}
        >
          <Sel
            k="spotify_session_type"
            label="Session Type"
            help="How votify authenticates with Spotify"
            options={SPOTIFY_SESSION_TYPE_OPTS}
            fallback="librespot"
            w="w-36"
          />
          {(s.spotify_session_type || 'librespot') === 'desktop' && (
            <Pth
              k="spotify_dll_path"
              label="Spotify DLL Path"
              help="Spotify DLL file for desktop session decryption"
              placeholder="Path to Spotify DLL"
              on={browseFile}
            />
          )}
          <Chk k="spotify_no_drm" label="No DRM (only download non-DRM content)" />
        </fieldset>
      </Section>

      <Section title="Audio">
        <Sel
          k="spotify_audio_download_mode"
          label="Download mode"
          help={
            isVotify
              ? 'Which tool Votify uses to fetch the audio'
              : 'Buffered fetches the file first so the progress bar is exact; Stream lets FFmpeg pull it directly'
          }
          options={SPOTIFY_AUDIO_DOWNLOAD_MODE_OPTS}
          fallback="ytdlp"
          w="w-36"
        />
        <Sel
          k="spotify_audio_remux_mode"
          label="Audio remux mode"
          help={!isVotify ? 'The native backend always remuxes with FFmpeg' : undefined}
          options={SPOTIFY_AUDIO_REMUX_MODE_OPTS}
          fallback="ffmpeg"
          w="w-36"
          disabled={!isVotify}
        />
        <Sel
          k="spotify_cover_size"
          label="Cover size"
          options={SPOTIFY_COVER_SIZE_OPTS}
          fallback="large"
          w="w-32"
        />
      </Section>

      <Section title="Files & Metadata">
        <Chk
          k="spotify_overwrite"
          label="Overwrite existing files"
          help="When off, a track already on disk is left alone and counted as skipped"
        />
        <Chk
          k="spotify_save_cover_file"
          label="Save cover file"
          help="The native downloader follows General → Artwork & Metadata."
          disabled={!isVotify}
        />
        <Chk
          k="spotify_save_playlist_file"
          label="Save playlist file"
          help="The native downloader follows General → File Naming."
          disabled={!isVotify}
        />
        <Chk
          k="spotify_no_synced_lyrics_file"
          label="Don't create synced lyrics file"
          help="Suppresses the sidecar for Spotify only; embedded lyrics are unaffected."
        />
        <Chk k="spotify_synced_lyrics_only" label="Download synced lyrics only (no audio)" />
      </Section>

      <Section title="Output Templates" disabled={!isVotify}>
        <Txt
          k="spotify_album_folder_template"
          label="Album folder"
          help="{album_artist}, {album}"
          fallback="{album_artist}/{album}"
        />
        <Txt
          k="spotify_single_disc_file_template"
          label="Single disc file"
          help="{track}, {title}"
          fallback="{track:02d} {title}"
        />
        <Txt
          k="spotify_multi_disc_file_template"
          label="Multi disc file"
          help="{disc}, {track}, {title}"
          fallback="{disc}-{track:02d} {title}"
        />
        <Txt
          k="spotify_playlist_file_template"
          label="Playlist file"
          help="{playlist_title}, {track}, {title}"
          fallback="Playlists/{playlist_title}/{track:02d} {title}"
        />
        <Num
          k="spotify_truncate"
          label="Truncate"
          help="Max length for file/folder names"
          fallback={40}
          w="w-28"
        />
      </Section>

      <Section title="Advanced" disabled={!isVotify}>
        <Num
          k="spotify_wait_interval"
          label="Wait interval (seconds)"
          help="Delay between downloads"
          fallback={10}
          w="w-28"
        />
        <Sel
          k="spotify_log_level"
          label="Log level"
          options={SPOTIFY_LOG_LEVEL_OPTS}
          fallback="INFO"
          w="w-32"
        />
        <Chk k="spotify_no_exceptions" label="Don't print exceptions" />
      </Section>

      <Section title="Telemetry & playback history">
        <Chk k="spotify_sync_playback_history" label="Sync playback history to Spotify" />
        <Chk k="spotify_telemetry_enabled" label="Send Spotify client telemetry" />
      </Section>
    </>
  );
}

function AppleMusicTab({
  s,
  set,
  browseFile,
}: {
  s: Partial<Settings>;
  set: SettingsSetter;
  browse?: (key: keyof Settings) => void;
  browseFile: (key: keyof Settings) => void;
}) {
  const backend = s.apple_downloader_backend || 'native';
  const isGamdl = backend === 'gamdl';
  return (
    <>
      <Section title="Downloader">
        <Row
          label="Backend"
          help="Native needs no setup. Gamdl needs Python but adds music videos, ALAC, Atmos, and spatial audio.
"
        >
          <Select value={backend} onValueChange={(v) => set('apple_downloader_backend', v)}>
            <SelectTrigger className="w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="native">Native (recommended)</SelectItem>
              <SelectItem value="gamdl">Gamdl (legacy CLI)</SelectItem>
            </SelectContent>
          </Select>
        </Row>
      </Section>

      <Section title="Authentication">
        <Pth
          k="apple_cookies_path"
          label="Cookies path"
          help="music.apple.com cookies (Netscape format). Used by both backends."
          on={browseFile}
        />
        <Pth
          k="apple_wvd_path"
          label=".wvd path"
          help="Widevine device file — both backends need it to decrypt Apple Music's protected HLS audio."
          placeholder="Path to .wvd file"
          on={browseFile}
        />
      </Section>

      <Section title="Output" disabled={!isGamdl}>
        <Row
          label="Output path"
          help="Derived from General → Downloads on every save, so editing it here has no effect."
        >
          <Input value={s.apple_output_path || ''} readOnly className="opacity-70" />
        </Row>
        <Txt
          k="apple_temp_path"
          label="Temp path"
          help="Must be an absolute path; a relative one is reset on load."
        />
        <Sel
          k="apple_download_mode"
          label="Download mode"
          options={APPLE_DOWNLOAD_MODE_OPTS}
          fallback="ytdlp"
          w="w-40"
        />
        <Num k="apple_truncate" label="Max filename length" fallback={40} w="w-28" />
      </Section>

      <Section title="Cover Art">
        <Sel
          k="apple_cover_format"
          label="Format"
          options={APPLE_COVER_FORMAT_OPTS}
          fallback="jpg"
          w="w-28"
        />
        <Num k="apple_cover_size" label="Max size (px)" fallback={1200} w="w-28" />
        <Chk
          k="apple_save_cover"
          label="Save cover art file"
          help="The native downloader follows General → Artwork & Metadata."
          disabled={!isGamdl}
        />
      </Section>

      <Section title="Files (native downloader)">
        <Chk k="apple_overwrite" label="Overwrite existing files" />
        <Chk k="apple_synced_lyrics_only" label="Download synced lyrics only (no audio)" />
        <Txt
          k="apple_language"
          label="Metadata language"
          help="e.g. en-US, ja-JP"
          fallback="en-US"
          w="w-28"
        />
      </Section>

      <ToggleSection
        title="Lyrics"
        enabled={!s.apple_no_synced_lyrics}
        onToggle={(v) => set('apple_no_synced_lyrics', !v)}
        disabled={!isGamdl}
      >
        <Row label="Synced lyrics format">
          <Select
            value={s.apple_synced_lyrics_format || 'lrc'}
            onValueChange={(v) => set('apple_synced_lyrics_format', v)}
          >
            <SelectTrigger className="w-28">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="lrc">LRC</SelectItem>
              <SelectItem value="ttml">TTML (word-synced)</SelectItem>
              <SelectItem value="both">Both</SelectItem>
              {isGamdl && <SelectItem value="srt">SRT</SelectItem>}
            </SelectContent>
          </Select>
        </Row>
      </ToggleSection>

      <Section title="Playback history">
        <Chk k="apple_sync_playback_history" label="Sync playback history to Apple Music" />
      </Section>

      <Section title="Extra Tags" disabled={!isGamdl}>
        <Chk k="apple_use_album_date" label="Use album release date for songs" />
        <Txt
          k="apple_date_tag_template"
          label="Date tag template"
          help="strftime format"
          fallback="%Y-%m-%dT%H:%M:%SZ"
        />
        <Txt
          k="apple_exclude_tags"
          label="Exclude tags"
          help="Comma-separated"
          placeholder="e.g. lyrics,comment"
        />
      </Section>

      <Section title="File Templates" disabled={!isGamdl}>
        <Txt
          k="apple_template_folder_album"
          label="Album folder"
          help="{album_artist}, {album}"
          fallback="{album_artist}/{album}"
        />
        <Txt
          k="apple_template_folder_compilation"
          label="Compilation folder"
          fallback="Compilations/{album}"
        />
        <Txt
          k="apple_template_folder_no_album"
          label="No-album folder"
          fallback="{album_artist}/Unknown Album"
        />
        <Txt
          k="apple_playlist_folder_template"
          label="Playlist folder"
          help="{playlist_name}"
          fallback="Playlists/{playlist_name}"
        />
        <Txt
          k="apple_template_file_single_disc"
          label="Single-disc file"
          help="{track}, {title}"
          fallback="{track:02d} {title}"
        />
        <Txt
          k="apple_template_file_multi_disc"
          label="Multi-disc file"
          help="{disc}, {track}, {title}"
          fallback="{disc}-{track:02d} {title}"
        />
        <Txt k="apple_template_file_no_album" label="No-album file" fallback="{title}" />
        <Txt
          k="apple_template_file_playlist"
          label="Playlist file"
          fallback="Playlists/{playlist_title}/{track:02d} {title}"
        />
      </Section>

      <Section title="Music Video">
        <Sel
          k="apple_mv_codec_priority"
          label="Codec priority"
          options={APPLE_MV_CODEC_PRIORITY_OPTS}
          fallback="h264"
          w="w-28"
        />
        <Sel
          k="apple_mv_remux_format"
          label="Remux format"
          options={APPLE_MV_REMUX_FORMAT_OPTS}
          fallback="m4v"
          w="w-28"
        />
        <Sel
          k="apple_mv_resolution"
          label="Max resolution"
          options={APPLE_MV_RESOLUTION_OPTS}
          fallback="1080p"
          w="w-28"
        />
        <Sel
          k="apple_uploaded_video_quality"
          label="Uploaded video quality"
          help={
            !isGamdl
              ? 'Applies to Apple Music post videos, which the native backend does not download'
              : undefined
          }
          options={APPLE_UPLOADED_VIDEO_QUALITY_OPTS}
          fallback="best"
          w="w-28"
          disabled={!isGamdl}
        />
      </Section>

      <Section title="Custom Tool Paths" disabled={!isGamdl}>
        {[
          ['apple_ffmpeg_path', 'FFmpeg', 'ffmpeg'],
          ['apple_nm3u8dlre_path', 'N_m3u8DL-RE', 'N_m3u8DL-RE'],
        ].map(([key, label, placeholder]) => (
          <Row key={key} label={label}>
            <div className="flex gap-2">
              <Input
                value={(s[key as keyof Settings] as string) || ''}
                onChange={(e) => set(key as keyof Settings, e.target.value)}
                placeholder={placeholder as string}
              />
              <Button variant="outline" size="sm" onClick={() => browseFile(key as keyof Settings)}>
                Browse
              </Button>
            </div>
          </Row>
        ))}
      </Section>

      <ToggleSection
        title="Wrapper (ALAC / lossless)"
        enabled={s.apple_use_wrapper ?? false}
        onToggle={(v) => set('apple_use_wrapper', v)}
      >
        <p className="text-xs text-muted-foreground leading-relaxed">
          ALAC and the other FairPlay renditions need a{' '}
          <code className="bg-muted px-1 rounded">glomatico/wrapper-v2</code> daemon you run
          yourself (Docker, Linux x86_64 or arm64). MediaHarbor connects to it — it cannot install
          or start it — and checks it is signed in before each download.
        </p>
        <Txt
          k="apple_wrapper_email"
          label="Apple ID / username"
          help="Your Apple Music sign-in. MediaHarbor signs the daemon in when it is running but logged out."
          placeholder="you@example.com"
        />
        <Row
          label="Password"
          help="Apple ID or app-specific password. MediaHarbor prompts for the two-factor code when a download starts."
        >
          <SecretInput
            value={s.apple_wrapper_password || ''}
            onChange={(v) => set('apple_wrapper_password', v)}
            placeholder="xxxx-xxxx-xxxx-xxxx"
          />
        </Row>
        <Txt
          k="apple_wrapper_url"
          label="Wrapper URL"
          help="Account, playback and status endpoint."
          placeholder="http://127.0.0.1"
        />
        <Txt
          k="apple_wrapper_decrypt_host"
          label="Decrypt host"
          help="Host of the batch-decrypt socket."
          placeholder="127.0.0.1"
        />
        <Txt k="apple_wrapper_decrypt_port" label="Decrypt port" placeholder="10020" w="w-28" />
        <WrapperConnectionTest enabled={s.apple_use_wrapper ?? false} />
      </ToggleSection>

      <Section title="Misc" disabled={!isGamdl}>
        <Row
          label="Artist auto-select"
          help="Auto-select content type when downloading an artist URL"
        >
          <Select
            value={s.apple_artist_auto_select || 'ask'}
            onValueChange={(v) => set('apple_artist_auto_select', v === 'ask' ? '' : v)}
          >
            <SelectTrigger className="w-48">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="ask">Ask interactively</SelectItem>
              <SelectItem value="main-albums">Main Albums</SelectItem>
              <SelectItem value="compilation-albums">Compilations</SelectItem>
              <SelectItem value="live-albums">Live Albums</SelectItem>
              <SelectItem value="singles-eps">Singles & EPs</SelectItem>
              <SelectItem value="all-albums">All Albums</SelectItem>
              <SelectItem value="top-songs">Top Songs</SelectItem>
              <SelectItem value="music-videos">Music Videos</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Chk
          k="apple_save_playlist"
          label="Save playlist file"
          help="The native downloader follows General → File Naming."
        />
        <Chk k="apple_no_exceptions" label="Don't print exceptions" />
        <Sel
          k="apple_log_level"
          label="Log level"
          options={APPLE_LOG_LEVEL_OPTS}
          fallback="INFO"
          w="w-32"
        />
      </Section>
    </>
  );
}

function WrapperConnectionTest({ enabled }: { enabled: boolean }) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<WrapperProbeResult | null>(null);
  const [code, setCode] = useState('');

  const run = useCallback(async (signIn: boolean, twoFactor?: string, signOut = false) => {
    setBusy(true);
    try {
      const r = await tauriAPI.app.probeAppleWrapper(signIn, twoFactor, signOut);
      setResult(r ?? null);
      if (r && !r.needsTwoFactor) setCode('');
    } catch (e) {
      setResult({
        reachable: false,
        authenticated: false,
        needsTwoFactor: false,
        state: '',
        playbackReady: false,
        version: '',
        runtime: '',
        appleId: null,
        error: errorMessage(e),
      });
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    if (enabled) void run(false);
  }, [enabled, run]);

  const signedIn = result?.reachable === true && result.authenticated;

  return (
    <div className="space-y-2 pt-1">
      <div className="flex items-center gap-2">
        <Button variant="secondary" size="sm" disabled={busy} onClick={() => run(false)}>
          Test connection
        </Button>
        {signedIn ? (
          <Button
            variant="secondary"
            size="sm"
            disabled={busy}
            onClick={() => run(false, undefined, true)}
          >
            Sign out
          </Button>
        ) : (
          <Button variant="secondary" size="sm" disabled={busy} onClick={() => run(true)}>
            Sign in
          </Button>
        )}
        {busy && <span className="text-xs text-muted-foreground">Contacting daemon…</span>}
      </div>

      {result && (
        <div className="text-xs space-y-1 rounded-md border border-border bg-muted/30 p-2">
          <p className={result.reachable ? 'text-green-500' : 'text-destructive'}>
            {result.reachable ? 'Daemon reachable' : 'Daemon unreachable'}
            {result.version && ` — v${result.version}`}
            {result.runtime && ` (${result.runtime})`}
          </p>
          {result.reachable && (
            <>
              <p className={result.authenticated ? 'text-green-500' : 'text-muted-foreground'}>
                {result.authenticated
                  ? `Signed in${result.appleId ? ` as ${result.appleId}` : ''}`
                  : `Not signed in${result.state ? ` (${result.state})` : ''}`}
              </p>
              {result.authenticated && !result.playbackReady && (
                <p className="text-amber-500">
                  FairPlay stack not ready — check the daemon logs; downloads will fail.
                </p>
              )}
            </>
          )}
          {result.error && <p className="text-destructive break-words">{result.error}</p>}
        </div>
      )}

      {result?.needsTwoFactor && (
        <div className="flex items-center gap-2">
          <Input
            value={code}
            onChange={(e) => setCode(e.target.value)}
            placeholder="Verification code"
            className="w-44"
            onKeyDown={(e) => e.key === 'Enter' && code.trim() && run(true, code)}
          />
          <Button
            variant="secondary"
            size="sm"
            disabled={busy || !code.trim()}
            onClick={() => run(true, code)}
          >
            Submit code
          </Button>
        </div>
      )}
    </div>
  );
}

function SecretInput({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
}) {
  const [show, setShow] = useState(false);
  return (
    <div className="relative flex items-center">
      <Input
        type={show ? 'text' : 'password'}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder || '••••••••'}
        className="pr-9 font-mono text-xs"
      />
      <button
        type="button"
        className="absolute right-2 text-muted-foreground hover:text-foreground"
        onClick={() => setShow((s) => !s)}
        tabIndex={-1}
      >
        {show ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
      </button>
    </div>
  );
}

function RawSettingsEditor({ prominent }: { prominent?: boolean }) {
  const [content, setContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const raw = await tauriAPI.orpheus?.readSettings();
      setContent(raw ?? '');
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setLoading(false);
    }
  };

  const save = async () => {
    if (content === null) return;
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      await tauriAPI.orpheus?.writeSettings(content);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  if (content === null) {
    return (
      <div className="space-y-1">
        <Button
          variant="outline"
          size={prominent ? 'default' : 'sm'}
          onClick={load}
          disabled={loading}
          className={prominent ? 'w-full justify-start gap-2 font-medium' : 'gap-1.5'}
        >
          {loading ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <FileText className="h-4 w-4" />
          )}
          Edit settings.json directly
        </Button>
        {error && <p className="text-xs text-destructive">{error}</p>}
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <textarea
        value={content}
        onChange={(e) => setContent(e.target.value)}
        className="w-full h-96 font-mono text-xs rounded-md border border-border bg-muted/20 p-3 resize-y focus:outline-none focus:ring-1 focus:ring-ring"
        spellCheck={false}
      />
      {error && <p className="text-xs text-destructive">{error}</p>}
      <div className="flex items-center gap-2">
        <Button size="sm" onClick={save} disabled={saving}>
          {saving ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" />
          ) : saved ? (
            <Check className="h-3.5 w-3.5 mr-1.5" />
          ) : null}
          {saved ? 'Saved' : 'Save'}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            setContent(null);
            setError(null);
          }}
        >
          Close
        </Button>
      </div>
    </div>
  );
}

const ORPHEUS_SKIP_GENERAL = new Set(['download_path', 'download_quality']);

function isSensitiveKey(key: string) {
  return /(password|secret|token|web_access_token|kc1_key|secret_key|app_secret|dev_key)/i.test(
    key
  );
}

function toLabel(key: string) {
  const s = key.replace(/_/g, ' ');
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function setNestedPath(
  obj: Record<string, unknown>,
  path: string[],
  value: unknown
): Record<string, unknown> {
  if (path.length === 0) return obj;
  const [head, ...rest] = path;
  if (rest.length === 0) return { ...obj, [head]: value };
  return {
    ...obj,
    [head]: setNestedPath((obj[head] as Record<string, unknown>) ?? {}, rest, value),
  };
}

function renderLeaf(
  path: string[],
  key: string,
  val: unknown,
  config: Record<string, unknown>,
  setConfig: (c: Record<string, unknown>) => void
): React.ReactNode {
  const id = path.join('_');
  const update = (v: unknown) => setConfig(setNestedPath(config, path, v));
  if (typeof val === 'boolean') {
    return (
      <Check2
        key={id}
        id={id}
        label={toLabel(key)}
        checked={val}
        onChange={update as (v: boolean) => void}
      />
    );
  }
  if (typeof val === 'number') {
    return (
      <Row key={id} label={toLabel(key)}>
        <Input
          type="number"
          value={val}
          onChange={(e) => update(Number(e.target.value))}
          className="w-28"
        />
      </Row>
    );
  }
  if (typeof val === 'string') {
    if (isSensitiveKey(key)) {
      return (
        <Row key={id} label={toLabel(key)}>
          <SecretInput value={val} onChange={update as (v: string) => void} />
        </Row>
      );
    }
    return (
      <Row key={id} label={toLabel(key)}>
        <Input value={val} onChange={(e) => update(e.target.value)} />
      </Row>
    );
  }
  return null;
}

function OrpheusConfigEditor() {
  const [config, setConfig] = useState<Record<string, unknown> | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [notFound, setNotFound] = useState(false);

  useEffect(() => {
    setLoading(true);
    tauriAPI.orpheus
      ?.readSettings()
      .then((raw) => {
        if (!raw || raw.trim() === '') {
          setNotFound(true);
        } else {
          try {
            setConfig(JSON.parse(raw));
          } catch {
            setError('Failed to parse settings.json');
          }
        }
      })
      .catch((e: unknown) => setError(errorMessage(e)))
      .finally(() => setLoading(false));
  }, []);

  const save = async () => {
    if (!config) return;
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      await tauriAPI.orpheus?.writeSettings(JSON.stringify(config, null, 2));
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  if (loading) return <p className="text-xs text-muted-foreground py-1">Loading settings.json…</p>;
  if (notFound)
    return (
      <p className="text-xs text-muted-foreground py-1">
        Settings file not found — it will be created on first download.
      </p>
    );
  if (!config) return null;

  const globalObj = config['global'] as Record<string, unknown> | undefined;
  const modulesObj = config['modules'] as Record<string, unknown> | undefined;

  return (
    <div className="space-y-3">
      {globalObj &&
        Object.entries(globalObj).map(([subKey, subVal]) => {
          if (typeof subVal !== 'object' || subVal === null || Array.isArray(subVal)) return null;
          const subObj = subVal as Record<string, unknown>;
          const entries = Object.entries(subObj).filter(
            ([k]) => !(subKey === 'general' && ORPHEUS_SKIP_GENERAL.has(k))
          );
          if (entries.length === 0) return null;
          return (
            <Section key={subKey} title={toLabel(subKey)}>
              {entries.map(([leafKey, leafVal]) =>
                renderLeaf(['global', subKey, leafKey], leafKey, leafVal, config, setConfig)
              )}
            </Section>
          );
        })}

      {modulesObj &&
        Object.entries(modulesObj).map(([modName, modVal]) => {
          if (typeof modVal !== 'object' || modVal === null || Array.isArray(modVal)) return null;
          const modObj = modVal as Record<string, unknown>;
          if (Object.keys(modObj).length === 0) return null;
          return (
            <Section key={modName} title={`${toLabel(modName)} (module)`}>
              {Object.entries(modObj).map(([leafKey, leafVal]) =>
                renderLeaf(['modules', modName, leafKey], leafKey, leafVal, config, setConfig)
              )}
            </Section>
          );
        })}

      {error && <p className="text-xs text-destructive">{error}</p>}
      <Button size="sm" onClick={save} disabled={saving || !config}>
        {saving ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" />
        ) : saved ? (
          <Check className="h-3.5 w-3.5 mr-1.5" />
        ) : null}
        {saved ? 'Saved' : 'Save settings'}
      </Button>
    </div>
  );
}

function OrpheusDLTab({ s, set }: { s: Partial<Settings>; set: SettingsSetter }) {
  return (
    <ToggleSection
      title="OrpheusDL"
      enabled={s.orpheusDL ?? false}
      onToggle={(v) => set('orpheusDL', v)}
    >
      <RawSettingsEditor prominent />
      <OrpheusConfigEditor />
    </ToggleSection>
  );
}

function ApiKeysTab({ s, set }: { s: Partial<Settings>; set: SettingsSetter }) {
  return (
    <>
      <div className="rounded-md border border-yellow-500/30 bg-yellow-500/5 px-4 py-3 text-xs text-yellow-600 dark:text-yellow-400 mb-2">
        These credentials are stored locally in your user settings file and are never sent to our
        servers. They are optional for platforms where you are already signed in with cookies (e.g.
        Spotify).
      </div>

      <Section title="Spotify Search API">
        <Row label="Client ID" help="Optional when signed in with cookies">
          <SecretInput
            value={s.spotify_client_id || ''}
            onChange={(v) => set('spotify_client_id', v)}
            placeholder="Your Spotify client ID"
          />
        </Row>
        <Row label="Client Secret" help="Keep this private">
          <SecretInput
            value={s.spotify_client_secret || ''}
            onChange={(v) => set('spotify_client_secret', v)}
            placeholder="Your Spotify client secret"
          />
        </Row>
      </Section>

      <Section title="Tidal Search API">
        <Row label="Client ID" help="From your Tidal Developer Portal app">
          <SecretInput
            value={s.tidal_client_id || ''}
            onChange={(v) => set('tidal_client_id', v)}
            placeholder="Your Tidal client ID"
          />
        </Row>
        <Row label="Client Secret" help="Keep this private">
          <SecretInput
            value={s.tidal_client_secret || ''}
            onChange={(v) => set('tidal_client_secret', v)}
            placeholder="Your Tidal client secret"
          />
        </Row>
      </Section>

      <Section title="YouTube Data API v3">
        <Row label="API Key" help="From your Google Cloud Console project">
          <SecretInput
            value={s.youtube_api_key || ''}
            onChange={(v) => set('youtube_api_key', v)}
            placeholder="Your YouTube Data API v3 key"
          />
        </Row>
      </Section>
    </>
  );
}

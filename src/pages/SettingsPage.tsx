import { useState, useEffect, useRef } from 'react';
import { useLocation } from 'react-router-dom';
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
import { useThemeStore } from '@/stores/useThemeStore';
import { AnimatePresence, motion } from 'framer-motion';
import { cn } from '@/utils/cn';
import { logError, logWarning } from '@/utils/logger';
import {
  Check, ChevronDown, Eye, EyeOff, Loader2,
  SlidersHorizontal, KeyRound, FileText, BoomBox
} from 'lucide-react';
import { PlatformIcon } from '@/utils/platforms';
import type { Settings, SettingsSetter } from '@/types/settings';

function Row({ label, help, children }: { label: string; help?: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[200px_1fr] items-start gap-4 py-0.5">
      <div className="pt-0.5">
        <Label className="text-sm font-medium text-foreground/90 leading-none">{label}</Label>
        {help && <p className="text-xs text-muted-foreground mt-1 leading-relaxed">{help}</p>}
      </div>
      <div>{children}</div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-border bg-card/50 overflow-hidden">
      <div className="px-4 py-2.5 border-b border-border/60 bg-muted/20">
        <h3 className="text-[11px] font-semibold text-muted-foreground uppercase tracking-widest">{title}</h3>
      </div>
      <div className="px-4 py-3 space-y-3">{children}</div>
    </div>
  );
}

function ToggleSection({ title, enabled, onToggle, children }: {
  title: string;
  enabled: boolean;
  onToggle: (v: boolean) => void;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(enabled);
  return (
    <div className={cn('rounded-xl border overflow-hidden transition-colors', enabled ? 'border-border' : 'border-border/40')}>
      <div className={cn('flex items-center gap-3 px-4 py-2.5 border-b', enabled ? 'bg-muted/20 border-border/60' : 'bg-muted/10 border-border/30')}>
        <Checkbox checked={!!enabled} onCheckedChange={(v) => onToggle(!!v)} className="mt-px" />
        <button onClick={() => setOpen(!open)} className="flex items-center gap-1.5 flex-1 text-left">
          <h3 className={cn('text-[11px] font-semibold uppercase tracking-widest transition-colors', enabled ? 'text-muted-foreground' : 'text-muted-foreground/50')}>{title}</h3>
          <ChevronDown className={cn('h-3 w-3 text-muted-foreground/50 transition-transform', open ? '' : '-rotate-90')} />
        </button>
      </div>
      {open && (
        <div className={cn('px-4 py-3 space-y-3 bg-card/50', !enabled && 'opacity-40 pointer-events-none')}>
          {children}
        </div>
      )}
    </div>
  );
}

function Check2({
  id,
  label,
  help,
  checked,
  onChange,
}: {
  id: string;
  label: string;
  help?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-start gap-2 py-0.5">
      <Checkbox id={id} checked={!!checked} onCheckedChange={(v) => onChange(!!v)} className="mt-0.5" />
      <div>
        <Label htmlFor={id} className="font-normal cursor-pointer">{label}</Label>
        {help && <p className="text-xs text-muted-foreground">{help}</p>}
      </div>
    </div>
  );
}

const TAB_GROUPS = [
  {
    label: null as string | null,
    tabs: [{ id: 'general', label: 'General', icon: SlidersHorizontal as React.ElementType | null, platform: null as string | null }],
  },
  {
    label: 'Services',
    tabs: [
      { id: 'ytdlp',      label: 'YT-DLP',      icon: null, platform: 'youtube' },
      { id: 'deezer',     label: 'Deezer',       icon: null, platform: 'deezer' },
      { id: 'qobuz',      label: 'Qobuz',        icon: null, platform: 'qobuz' },
      { id: 'tidal',      label: 'Tidal',        icon: null, platform: 'tidal' },
      { id: 'spotify',    label: 'Spotify',      icon: null, platform: 'spotify' },
      { id: 'applemusic', label: 'Apple Music',  icon: null, platform: 'applemusic' },
    ],
  },
  {
    label: 'Developer',
    tabs: [{ id: 'apikeys', label: 'API Keys', icon: KeyRound as React.ElementType | null, platform: null as string | null }],
  },
  {
    label: 'Backends',
    tabs: [{ id: 'orpheusdl', label: 'OrpheusDL', icon: BoomBox as React.ElementType | null, platform: null as string | null }],
  },
];

const ALL_TABS = TAB_GROUPS.flatMap((g) => g.tabs);

export default function SettingsPage() {
  const location = useLocation();
  const initialTab = new URLSearchParams(location.search).get('tab') || 'general';
  const [activeTab, setActiveTab] = useState(initialTab);

  useEffect(() => {
    const tab = new URLSearchParams(location.search).get('tab');
    if (tab) setActiveTab(tab);
  }, [location.search]);
  const [settings, setSettings] = useState<Partial<Settings>>({});
  const [loading, setLoading] = useState(true);
  const [saveState, setSaveState] = useState<'idle' | 'saving' | 'saved'>('idle');
  const saveTimer = useRef<ReturnType<typeof setTimeout>>();
  const savedTimer = useRef<ReturnType<typeof setTimeout>>();
  const isFirstLoad = useRef(true);
  const setTheme = useThemeStore((s) => s.setTheme);

  useEffect(() => {
    window.electron?.settings.get().then((data) => {
      if (data) setSettings(data);
    }).catch((err) => {
      logError('settings', 'Failed to load settings', err instanceof Error ? (err.stack || err.message) : String(err));
    }).finally(() => {
      setLoading(false);
      setTimeout(() => { isFirstLoad.current = false; }, 50);
    });
  }, []);

  useEffect(() => {
    if (isFirstLoad.current || loading) return;
    clearTimeout(saveTimer.current);
    clearTimeout(savedTimer.current);
    setSaveState('saving');

    saveTimer.current = setTimeout(async () => {
      try {
        await window.electron?.settings.set(settings);
        if (settings.theme === 'dark') setTheme('dark');
        else if (settings.theme === 'light') setTheme('light');
        else if (settings.theme === 'auto') {
          setTheme(window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
        }
        setSaveState('saved');
        savedTimer.current = setTimeout(() => setSaveState('idle'), 2000);
      } catch (err) {
        logError('settings', 'Failed to save settings', err instanceof Error ? (err.stack || err.message) : String(err));
        setSaveState('idle');
      }
    }, 900);

    return () => { clearTimeout(saveTimer.current); clearTimeout(savedTimer.current); };
  }, [settings, loading, setTheme]);

  const set: SettingsSetter = (key, value) =>
    setSettings((prev) => ({ ...prev, [key]: value }));

  const browseFolder = async (key: keyof Settings) => {
    const folder = await window.electron?.settings.openFolder();
    if (folder) set(key, folder);
  };

  const browseFile = async (key: keyof Settings) => {
    const file = await window.electron?.settings.openFile();
    if (file) set(key, file);
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const currentTab = ALL_TABS.find((t) => t.id === activeTab) ?? ALL_TABS[0];
  const TabIcon = currentTab.icon;
  const tabPlatform = currentTab.platform;

  return (
    <div className="flex h-full min-h-0">
      <div className="w-52 border-r border-border shrink-0 flex flex-col bg-card/30">
        <div className="p-2.5 space-y-0.5 flex-1 overflow-y-auto">
          {TAB_GROUPS.map((group, gi) => (
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
                    {t.platform
                      ? <PlatformIcon platform={t.platform} size={14} />
                      : Icon && <Icon className="h-3.5 w-3.5 shrink-0" />
                    }
                    {t.label}
                  </button>
                );
              })}
            </div>
          ))}
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
                  <><Loader2 className="h-3 w-3 animate-spin text-muted-foreground" /><span className="text-muted-foreground">Saving…</span></>
                )}
                {saveState === 'saved' && (
                  <><Check className="h-3 w-3 text-emerald-500" /><span className="text-emerald-500 font-medium">Saved</span></>
                )}
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>

      <div className="flex-1 flex flex-col min-h-0">
        <div className="flex items-center gap-2.5 px-6 py-4 border-b border-border shrink-0 bg-card/20">
          {tabPlatform
            ? <PlatformIcon platform={tabPlatform} size={16} />
            : TabIcon && <TabIcon className="h-4 w-4 text-muted-foreground" />
          }
          <h1 className="text-sm font-semibold tracking-tight">{currentTab.label}</h1>
        </div>

        <div className="flex-1 overflow-y-auto p-6 space-y-4">
          {activeTab === 'general'    && <GeneralTab    s={settings} set={set} browse={browseFolder} />}
          {activeTab === 'ytdlp'      && <YtDlpTab      s={settings} set={set} />}
          {activeTab === 'deezer'     && <DeezerTab     s={settings} set={set} />}
          {activeTab === 'qobuz'      && <QobuzTab      s={settings} set={set} />}
          {activeTab === 'tidal'      && <TidalTab      s={settings} set={set} />}
          {activeTab === 'spotify'    && <SpotifyTab    s={settings} set={set} browseFile={browseFile} />}
          {activeTab === 'applemusic' && <AppleMusicTab s={settings} set={set} browse={browseFolder} browseFile={browseFile} />}
          {activeTab === 'apikeys'    && <ApiKeysTab    s={settings} set={set} />}
          {activeTab === 'orpheusdl' && <OrpheusDLTab  s={settings} set={set} />}
        </div>
      </div>
    </div>
  );
}

function FormatGuide() {
  const [open, setOpen] = useState(false);

  const trackVars = [
    ['{title}',            'Track title',                                        'One More Time'],
    ['{artist}',           'Primary performing artist(s)',                       'Daft Punk'],
    ['{albumartist}',      'Album-level artist (may differ from track artist)',  'Daft Punk'],
    ['{album}',            'Album name',                                         'Discovery'],
    ['{year}',             'Release year (4 digits)',                            '2001'],
    ['{date}',             'Full release date',                                  '2001-02-26'],
    ['{tracknumber}',      'Track number as-is',                                 '1'],
    ['{tracknumber:02}',   'Track number zero-padded to 2 digits',              '01'],
    ['{tracktotal}',       'Total tracks in the album',                          '14'],
    ['{discnumber}',       'Disc number',                                        '1'],
    ['{disctotal}',        'Total discs',                                        '1'],
    ['{genre}',            'Primary genre from the service',                     'Electronic'],
    ['{isrc}',             'ISRC code',                                          'FRZ019800099'],
    ['{explicit}',         '" (Explicit)" if the track is flagged, else empty',  ' (Explicit)'],
    ['{label}',            'Record label',                                       'Virgin Records'],
    ['{composer}',         'Composer / songwriter name(s)',                      'Thomas Bangalter'],
  ];

  const folderOnlyVars = [
    ['{albumartist}', 'Album artist — use this for the top-level artist folder', 'Daft Punk'],
    ['{album}',       'Album name',                                               'Discovery'],
    ['{year}',        'Release year',                                             '2001'],
    ['{genre}',       'Album genre',                                              'Electronic'],
    ['{label}',       'Record label',                                             'Virgin Records'],
  ];

  return (
    <div className="rounded-md border border-border overflow-hidden text-xs">
      <button
        className="w-full flex items-center justify-between px-3 py-2 bg-muted/40 hover:bg-muted/60 transition-colors text-left"
        onClick={() => setOpen(o => !o)}
      >
        <span className="font-medium text-foreground/80">Format variable reference</span>
        <ChevronDown className={`h-3.5 w-3.5 text-muted-foreground transition-transform ${open ? 'rotate-180' : ''}`} />
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
                    <td className="py-0.5 pr-4"><code className="text-foreground bg-muted px-1 rounded">{v}</code></td>
                    <td className="py-0.5 pr-4 text-muted-foreground">{desc}</td>
                    <td className="py-0.5 text-muted-foreground/70">{ex}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div>
            <p className="font-semibold text-foreground mb-1.5">Album folder variables</p>
            <p className="text-muted-foreground mb-1.5">All track variables also work in folder format. Folder-specific recommendations:</p>
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
                    <td className="py-0.5 pr-4"><code className="text-foreground bg-muted px-1 rounded">{v}</code></td>
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
              Append <code className="text-foreground bg-muted px-1 rounded">:N</code> to any numeric variable to zero-pad it to N digits.
              {' '}<code className="text-foreground bg-muted px-1 rounded">{'{tracknumber:02}'}</code> → <code>01</code>, <code>02</code> … <code>14</code>
              {' '}·{' '}<code className="text-foreground bg-muted px-1 rounded">{'{tracknumber:03}'}</code> → <code>001</code>
            </p>
          </div>

          <div>
            <p className="font-semibold text-foreground mb-1.5">Preset examples</p>
            <div className="space-y-1">
              {[
                ['Standard',       '{albumartist} - {album} ({year})',                     '{tracknumber:02}. {artist} - {title}{explicit}'],
                ['With genre',     '{genre}/{albumartist} - {album} ({year})',             '{tracknumber:02}. {title}{explicit}'],
                ['Label/Year',     '{label}/{year} - {albumartist} - {album}',             '{tracknumber:02}. {artist} - {title}'],
                ['Disc aware',     '{albumartist} - {album} ({year})',                     '{discnumber}-{tracknumber:02}. {artist} - {title}'],
                ['Minimal',        '{albumartist}/{album}',                                '{tracknumber:02}. {title}'],
              ].map(([name, folder, track]) => (
                <div key={name} className="rounded bg-muted/50 px-2 py-1.5">
                  <span className="font-medium text-foreground">{name}</span>
                  <div className="text-muted-foreground mt-0.5">
                    <span className="text-foreground/50">Folder: </span><code>{folder}</code>
                  </div>
                  <div className="text-muted-foreground">
                    <span className="text-foreground/50">Track:  </span><code>{track}</code>
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

function GeneralTab({ s, set, browse }: { s: Partial<Settings>; set: SettingsSetter; browse: (key: keyof Settings) => void }) {
  return (
    <>
      <Section title="Appearance">
        <Row label="Theme">
          <Select value={s.theme || 'auto'} onValueChange={(v) => set('theme', v)}>
            <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="auto">Auto (System)</SelectItem>
              <SelectItem value="dark">Dark</SelectItem>
              <SelectItem value="light">Light</SelectItem>
            </SelectContent>
          </Select>
        </Row>
      </Section>

      <Section title="Downloads">
        <Row label="Download Location">
          <div className="flex gap-2">
            <Input value={s.downloadLocation || ''} onChange={(e) => set('downloadLocation', e.target.value)} />
            <Button variant="outline" size="sm" onClick={() => browse('downloadLocation')}>Browse</Button>
          </div>
        </Row>
        <Check2 id="createPlatformSubfolders" label="Create platform subfolders"
          help="Organise downloads into per-platform folders (Spotify/, Tidal/, etc.)"
          checked={s.createPlatformSubfolders ?? false} onChange={(v) => set('createPlatformSubfolders', v)} />
      </Section>

      <Section title="Tools">
        <Check2 id="orpheusDL" label="Prioritize OrpheusDL" checked={s.orpheusDL ?? false} onChange={(v) => set('orpheusDL', v)} />
        <Check2 id="autoUpdate" label="Auto update on launch" checked={s.autoUpdate ?? false} onChange={(v) => set('autoUpdate', v)} />
      </Section>

      <Section title="File Naming (Tidal / Qobuz / Deezer)">
        <Row label="Album folder">
          <Input value={s.filepaths_folder_format || '{albumartist} - {album} ({year})'} onChange={(e) => set('filepaths_folder_format', e.target.value)} />
        </Row>
        <Row label="Track filename">
          <Input value={s.filepaths_track_format || '{tracknumber:02}. {artist} - {title}{explicit}'} onChange={(e) => set('filepaths_track_format', e.target.value)} />
        </Row>
        <FormatGuide />
        <Row label="Max filename length" help="Characters, 0 = unlimited">
          <Input type="number" value={s.filepaths_truncate_to ?? 120} onChange={(e) => set('filepaths_truncate_to', Number(e.target.value))} className="w-28" />
        </Row>
        <Check2 id="filepaths_restrict_characters" label="Restrict special characters in filenames"
          help={'Replaces < > : " / \\ | ? * with underscores'}
          checked={s.filepaths_restrict_characters !== false} onChange={(v) => set('filepaths_restrict_characters', v)} />
        <Check2 id="disc_subdirectories" label="Create disc subdirectories for multi-disc albums"
          help="Creates Disc 1/, Disc 2/, etc. subfolders when an album spans multiple discs"
          checked={s.disc_subdirectories !== false} onChange={(v) => set('disc_subdirectories', v)} />
      </Section>

      <Section title="Artwork & Metadata (Tidal / Qobuz / Deezer)">
        <Check2 id="embed_cover" label="Embed cover art in audio file" checked={s.embed_cover !== false} onChange={(v) => set('embed_cover', v)} />
        <Check2 id="save_cover" label="Save cover.jpg alongside each track" checked={!!s.save_cover} onChange={(v) => set('save_cover', v)} />
      </Section>

      <Section title="Conversion (Tidal / Qobuz / Deezer)">
        <Check2 id="conversion_check" label="Convert after download"
          help="Re-encodes downloaded files. Use to convert to a different format, downsample hi-res, or get a specific bitrate."
          checked={!!s.conversion_check} onChange={(v) => set('conversion_check', v)} />
        {s.conversion_check && (<>
          <Row label="Output format">
            <Select value={s.conversion_codec || 'FLAC'} onValueChange={(v) => set('conversion_codec', v)}>
              <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="FLAC">FLAC</SelectItem>
                <SelectItem value="ALAC">ALAC</SelectItem>
                <SelectItem value="MP3">MP3</SelectItem>
                <SelectItem value="AAC">AAC</SelectItem>
                <SelectItem value="OPUS">Opus</SelectItem>
                <SelectItem value="VORBIS">Ogg Vorbis</SelectItem>
              </SelectContent>
            </Select>
          </Row>
          {['MP3','AAC','OPUS','VORBIS'].includes((s.conversion_codec || 'FLAC').toUpperCase()) && (
            <Row label="Bitrate (kbps)">
              <Input type="number" value={s.conversion_lossy_bitrate ?? 320} onChange={(e) => set('conversion_lossy_bitrate', Number(e.target.value))} className="w-28" />
            </Row>
          )}
          <Row label="Max sampling rate" help="Downsample if source exceeds this.">
            <Select value={s.conversion_sampling_rate != null ? String(s.conversion_sampling_rate) : 'original'} onValueChange={(v) => set('conversion_sampling_rate', v === 'original' ? null : Number(v))}>
              <SelectTrigger className="w-40"><SelectValue placeholder="Original" /></SelectTrigger>
              <SelectContent>
                <SelectItem value="original">Original</SelectItem>
                <SelectItem value="44100">44.1 kHz</SelectItem>
                <SelectItem value="48000">48 kHz</SelectItem>
                <SelectItem value="88200">88.2 kHz</SelectItem>
                <SelectItem value="96000">96 kHz</SelectItem>
              </SelectContent>
            </Select>
          </Row>
          <Row label="Max bit depth" help="Reduce bit depth if source exceeds this.">
            <Select value={s.conversion_bit_depth != null ? String(s.conversion_bit_depth) : 'original'} onValueChange={(v) => set('conversion_bit_depth', v === 'original' ? null : Number(v))}>
              <SelectTrigger className="w-40"><SelectValue placeholder="Original" /></SelectTrigger>
              <SelectContent>
                <SelectItem value="original">Original</SelectItem>
                <SelectItem value="16">16-bit</SelectItem>
                <SelectItem value="24">24-bit</SelectItem>
              </SelectContent>
            </Select>
          </Row>
        </>)}
      </Section>
    </>
  );
}

function YtDlpTab({ s, set }: { s: Partial<Settings>; set: SettingsSetter }) {
  return (
    <>
      <Section title="Output">
        <Row label="Output template" help="yt-dlp filename template">
          <Input value={s.download_output_template || '%(title)s.%(ext)s'} onChange={(e) => set('download_output_template', e.target.value)} />
        </Row>
        <Row label="Max downloads" help="0 = unlimited">
          <Input type="number" value={s.max_downloads ?? 0} onChange={(e) => set('max_downloads', Number(e.target.value))} className="w-28" />
        </Row>
        <Row label="Max retries">
          <Input type="number" value={s.max_retries ?? 5} onChange={(e) => set('max_retries', Number(e.target.value))} className="w-28" />
        </Row>
        <Check2 id="continue" label="Continue partially downloaded files" checked={s.continue ?? false} onChange={(v) => set('continue', v)} />
        <Check2 id="use_aria2" label="Use aria2c for downloading" checked={s.use_aria2 ?? false} onChange={(v) => set('use_aria2', v)} />
      </Section>

      <Section title="Metadata & Subtitles">
        <Check2 id="add_metadata" label="Add metadata to files" checked={s.add_metadata ?? false} onChange={(v) => set('add_metadata', v)} />
        <Check2 id="embed_chapters" label="Embed chapters" checked={s.embed_chapters ?? false} onChange={(v) => set('embed_chapters', v)} />
        <Check2 id="add_subtitle_to_file" label="Embed subtitles" checked={s.add_subtitle_to_file ?? false} onChange={(v) => set('add_subtitle_to_file', v)} />
      </Section>

      <Section title="Network">
        <Check2 id="use_cookies" label="Use cookies from file" checked={s.use_cookies ?? false} onChange={(v) => set('use_cookies', v)} />
        {s.use_cookies && (
          <Row label="Cookies file path">
            <Input value={s.cookies || ''} onChange={(e) => set('cookies', e.target.value)} />
          </Row>
        )}
        <Row label="Cookies from browser" help="e.g. chrome, firefox">
          <Input value={s.cookies_from_browser || ''} onChange={(e) => set('cookies_from_browser', e.target.value)} className="w-48" />
        </Row>

        <Check2 id="use_proxy" label="Use proxy" checked={s.use_proxy ?? false} onChange={(v) => set('use_proxy', v)} />
        {s.use_proxy && (
          <Row label="Proxy URL">
            <Input value={s.proxy_url || ''} onChange={(e) => set('proxy_url', e.target.value)} />
          </Row>
        )}

        <Check2 id="download_speed_limit" label="Limit download speed" checked={s.download_speed_limit ?? false} onChange={(v) => set('download_speed_limit', v)} />
        {s.download_speed_limit && (
          <Row label="Speed limit">
            <div className="flex gap-2 items-center">
              <Input type="number" value={s.speed_limit_value ?? 0} onChange={(e) => set('speed_limit_value', Number(e.target.value))} className="w-28" />
              <Select value={s.speed_limit_type || 'M'} onValueChange={(v) => set('speed_limit_type', v)}>
                <SelectTrigger className="w-24"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="K">KB/s</SelectItem>
                  <SelectItem value="M">MB/s</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </Row>
        )}
      </Section>

      <Section title="Authentication">
        <Check2 id="use_authentication" label="Use username/password" checked={s.use_authentication ?? false} onChange={(v) => set('use_authentication', v)} />
        {s.use_authentication && (
          <>
            <Row label="Username">
              <Input value={s.username || ''} onChange={(e) => set('username', e.target.value)} className="w-64" />
            </Row>
            <Row label="Password">
              <Input type="password" value={s.password || ''} onChange={(e) => set('password', e.target.value)} className="w-64" />
            </Row>
          </>
        )}
      </Section>

      <Section title="Extensions">
        <Check2 id="yt_override_download_extension" label="Override YouTube video extension" checked={s.yt_override_download_extension ?? false} onChange={(v) => set('yt_override_download_extension', v)} />
        {s.yt_override_download_extension && (
          <Row label="Video extension">
            <Input value={s.youtubeVideoExtensions || 'mp4'} onChange={(e) => set('youtubeVideoExtensions', e.target.value)} className="w-28" />
          </Row>
        )}
        <Check2 id="ytm_override_download_extension" label="Override YouTube Music extension" checked={s.ytm_override_download_extension ?? false} onChange={(v) => set('ytm_override_download_extension', v)} />
        {s.ytm_override_download_extension && (
          <Row label="Audio extension">
            <Input value={s.youtubeAudioExtensions || 'mp3'} onChange={(e) => set('youtubeAudioExtensions', e.target.value)} className="w-28" />
          </Row>
        )}
      </Section>

      <Section title="SponsorBlock">
        <Check2 id="no_sponsorblock" label="Disable SponsorBlock entirely" checked={s.no_sponsorblock ?? false} onChange={(v) => set('no_sponsorblock', v)} />
        {!s.no_sponsorblock && (
          <>
            <Row label="Mark categories" help="Comma-separated categories to mark">
              <Input value={s.sponsorblock_mark || 'all'} onChange={(e) => set('sponsorblock_mark', e.target.value)} />
            </Row>
            <Row label="Remove categories" help="Comma-separated categories to cut out">
              <Input value={s.sponsorblock_remove || ''} onChange={(e) => set('sponsorblock_remove', e.target.value)} />
            </Row>
            <Row label="Chapter title template">
              <Input value={s.sponsorblock_chapter_title || '[SponsorBlock]: %(category_names)l'} onChange={(e) => set('sponsorblock_chapter_title', e.target.value)} />
            </Row>
            <Row label="API URL">
              <Input value={s.sponsorblock_api_url || 'https://sponsor.ajay.app'} onChange={(e) => set('sponsorblock_api_url', e.target.value)} />
            </Row>
          </>
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
            <li>Open <strong>deezer.com</strong> in your browser and log in</li>
            <li>Open DevTools (F12) → Application → Cookies → <code>https://www.deezer.com</code></li>
            <li>Copy the value of the <code>arl</code> cookie and paste it below</li>
          </ol>
          <p className="text-xs mt-1">Free accounts stream at 128 kbps MP3. Premium unlocks 320 kbps. HiFi unlocks FLAC.</p>
        </div>
        <Row label="ARL Token">
          <Input type="password" value={s.deezer_arl || ''} onChange={(e) => set('deezer_arl', e.target.value)} placeholder="Paste your ARL cookie value here" />
        </Row>
      </Section>

      <Section title="Lyrics">
        <Check2 id="deezer_save_lrc_files" label="Save synced lyrics (.lrc) alongside each track" help="Requires a paid account; free accounts may not have synced lyrics" checked={!!s.save_lrc_files} onChange={(v) => set('save_lrc_files', v)} />
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
      </Section>

      <Section title="Options">
        <Check2 id="qobuz_download_booklets" label="Download booklets (PDFs)" checked={!!s.qobuz_download_booklets} onChange={(v) => set('qobuz_download_booklets', v)} />
      </Section>

      <Section title="Download Filters">
        <Check2 id="qobuz_filters_extras" label="Exclude extras (bonus tracks, interludes)" checked={!!s.qobuz_filters_extras} onChange={(v) => set('qobuz_filters_extras', v)} />
        <Check2 id="qobuz_non_remaster" label="Exclude non-remastered versions" checked={!!s.qobuz_non_remaster} onChange={(v) => set('qobuz_non_remaster', v)} />
        <Check2 id="qobuz_non_studio_albums" label="Exclude non-studio albums (live, compilations)" checked={!!s.qobuz_non_studio_albums} onChange={(v) => set('qobuz_non_studio_albums', v)} />
        <Check2 id="qobuz_non_albums" label="Exclude non-album releases (singles, EPs)" checked={!!s.qobuz_non_albums} onChange={(v) => set('qobuz_non_albums', v)} />
        <Check2 id="qobuz_features" label="Exclude feature appearances" checked={!!s.qobuz_features} onChange={(v) => set('qobuz_features', v)} />
        <Check2 id="qobuz_repeats" label="Exclude repeated albums (duplicates)" checked={!!s.qobuz_repeats} onChange={(v) => set('qobuz_repeats', v)} />
      </Section>

      <Section title="Advanced (auto-filled)">
        <Row label="App ID" help="Auto-fetched from Qobuz on each login">
          <Input value={s.qobuz_app_id || ''} onChange={(e) => set('qobuz_app_id', e.target.value)} placeholder="Auto-detected" />
        </Row>
        <Row label="Secrets" help="Auto-fetched — comma-separated">
          <Input value={Array.isArray(s.qobuz_secrets) ? s.qobuz_secrets.join(', ') : (s.qobuz_secrets || '')}
            onChange={(e) => set('qobuz_secrets', e.target.value)} placeholder="Auto-detected" />
        </Row>
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

      <Section title="Lyrics">
        <Check2 id="tidal_save_lrc_files" label="Save synced lyrics (.lrc) alongside each track" help="Fetched from Tidal's lyrics API; not all tracks have synced lyrics" checked={!!s.save_lrc_files} onChange={(v) => set('save_lrc_files', v)} />
      </Section>
    </>
  );
}

function SpotifyTab({ s, set, browseFile }: { s: Partial<Settings>; set: SettingsSetter; browseFile: (key: keyof Settings) => void }) {
  const [loginStatus, setLoginStatus] = useState<'idle' | 'loading' | 'success' | 'error'>('idle');
  const [loginMessage, setLoginMessage] = useState('');

  useEffect(() => {
    if (!s.spotify_cookies_path) return;
    window.electron?.spotifyAccount?.getStatus().then((status) => {
      if (status?.loggedIn && status?.profile) {
        setLoginStatus('success');
        setLoginMessage(`Connected as ${status.profile.name || status.profile.id || 'Spotify user'}`);
      } else {
        setLoginStatus((prev) => prev === 'loading' ? prev : 'idle');
      }
    }).catch((err: unknown) => {
      logWarning('settings', 'Spotify status check failed', err instanceof Error ? (err.stack || err.message) : String(err));
    });
  }, [s.spotify_cookies_path]);

  const handleLogin = async () => {
    setLoginStatus('loading');
    setLoginMessage('Opening Spotify login…');
    try {
      const result = await window.electron?.spotifyAccount?.login();
      setLoginStatus('success');
      setLoginMessage(`Connected as ${result?.name || result?.id || 'Spotify user'}`);
    } catch (err: unknown) {
      setLoginStatus('error');
      setLoginMessage(err instanceof Error ? err.message : 'Login failed');
    }
  };

  const handleLogout = async () => {
    try {
      await window.electron?.spotifyAccount?.logout();
      setLoginStatus('idle');
      setLoginMessage('');
    } catch (err: unknown) {
      setLoginMessage(err instanceof Error ? err.message : 'Logout failed');
    }
  };

  return (
    <>
      <Section title="Spotify Account">
        <div className="rounded-md border border-blue-500/30 bg-blue-500/5 px-4 py-3 text-xs text-blue-600 dark:text-blue-400 mb-2">
          Connect your Spotify account for personalized search. Uses your cookies file from the Authentication section below — no extra setup needed.
        </div>
        <div className="flex items-center gap-3 pt-1">
          {loginStatus !== 'success' ? (
            <Button variant="outline" size="sm" onClick={handleLogin} disabled={loginStatus === 'loading'}>
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
        <Row label="Cookies Path" help="Netscape format cookies file from spotify.com">
          <Input value={s.spotify_cookies_path || ''} onChange={(e) => set('spotify_cookies_path', e.target.value)} placeholder="Path to spotify.com cookies.txt" />
          <Button variant="outline" size="sm" onClick={() => browseFile('spotify_cookies_path')}>Browse</Button>
        </Row>
        <Row label="WVD Path" help=".wvd file for DRM decryption">
          <Input value={s.spotify_wvd_path || ''} onChange={(e) => set('spotify_wvd_path', e.target.value)} placeholder="Path to .wvd file" />
          <Button variant="outline" size="sm" onClick={() => browseFile('spotify_wvd_path')}>Browse</Button>
        </Row>
        <Check2 id="spotify_no_drm" label="No DRM (only download non-DRM content)" checked={s.spotify_no_drm ?? false} onChange={(v) => set('spotify_no_drm', v)} />
      </Section>

      <Section title="Audio">
        <Row label="Download mode">
          <Select value={s.spotify_audio_download_mode || 'ytdlp'} onValueChange={(v) => set('spotify_audio_download_mode', v)}>
            <SelectTrigger className="w-36"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="ytdlp">yt-dlp</SelectItem>
              <SelectItem value="aria2c">aria2c</SelectItem>
              <SelectItem value="curl">curl</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Row label="Audio remux mode">
          <Select value={s.spotify_audio_remux_mode || 'ffmpeg'} onValueChange={(v) => set('spotify_audio_remux_mode', v)}>
            <SelectTrigger className="w-36"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="ffmpeg">FFmpeg</SelectItem>
              <SelectItem value="mp4box">MP4Box</SelectItem>
              <SelectItem value="mp4decrypt">mp4decrypt</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Row label="Cover size">
          <Select value={s.spotify_cover_size || 'large'} onValueChange={(v) => set('spotify_cover_size', v)}>
            <SelectTrigger className="w-32"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="large">Large</SelectItem>
              <SelectItem value="extra-large">Extra Large</SelectItem>
              <SelectItem value="medium">Medium</SelectItem>
              <SelectItem value="small">Small</SelectItem>
            </SelectContent>
          </Select>
        </Row>
      </Section>

      <Section title="Files & Metadata">
        <Check2 id="spotify_overwrite" label="Overwrite existing files" checked={s.spotify_overwrite ?? false} onChange={(v) => set('spotify_overwrite', v)} />
        <Check2 id="spotify_save_cover_file" label="Save cover file" checked={s.spotify_save_cover_file ?? false} onChange={(v) => set('spotify_save_cover_file', v)} />
        <Check2 id="spotify_save_playlist_file" label="Save playlist file" checked={s.spotify_save_playlist_file ?? false} onChange={(v) => set('spotify_save_playlist_file', v)} />
        <Check2 id="spotify_no_synced_lyrics_file" label="Don't create synced lyrics file" checked={s.spotify_no_synced_lyrics_file ?? false} onChange={(v) => set('spotify_no_synced_lyrics_file', v)} />
        <Check2 id="spotify_synced_lyrics_only" label="Download synced lyrics only (no audio)" checked={s.spotify_synced_lyrics_only ?? false} onChange={(v) => set('spotify_synced_lyrics_only', v)} />
      </Section>

      <Section title="Output Templates">
        <Row label="Album folder" help="{album_artist}, {album}">
          <Input value={s.spotify_album_folder_template || '{album_artist}/{album}'} onChange={(e) => set('spotify_album_folder_template', e.target.value)} />
        </Row>
        <Row label="Single disc file" help="{track}, {title}">
          <Input value={s.spotify_single_disc_file_template || '{track:02d} {title}'} onChange={(e) => set('spotify_single_disc_file_template', e.target.value)} />
        </Row>
        <Row label="Multi disc file" help="{disc}, {track}, {title}">
          <Input value={s.spotify_multi_disc_file_template || '{disc}-{track:02d} {title}'} onChange={(e) => set('spotify_multi_disc_file_template', e.target.value)} />
        </Row>
        <Row label="Playlist file" help="{playlist_title}, {track}, {title}">
          <Input value={s.spotify_playlist_file_template || 'Playlists/{playlist_title}/{track:02d} {title}'} onChange={(e) => set('spotify_playlist_file_template', e.target.value)} />
        </Row>
        <Row label="Truncate" help="Max length for file/folder names">
          <Input type="number" value={s.spotify_truncate ?? 40} onChange={(e) => set('spotify_truncate', Number(e.target.value))} className="w-28" />
        </Row>
      </Section>

      <Section title="Advanced">
        <Row label="Wait interval (seconds)" help="Delay between downloads">
          <Input type="number" value={s.spotify_wait_interval ?? 10} onChange={(e) => set('spotify_wait_interval', Number(e.target.value))} className="w-28" />
        </Row>
        <Row label="Log level">
          <Select value={s.spotify_log_level || 'INFO'} onValueChange={(v) => set('spotify_log_level', v)}>
            <SelectTrigger className="w-32"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="DEBUG">DEBUG</SelectItem>
              <SelectItem value="INFO">INFO</SelectItem>
              <SelectItem value="WARNING">WARNING</SelectItem>
              <SelectItem value="ERROR">ERROR</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Check2 id="spotify_no_exceptions" label="Don't print exceptions" checked={s.spotify_no_exceptions ?? false} onChange={(v) => set('spotify_no_exceptions', v)} />
      </Section>
    </>
  );
}

function AppleMusicTab({ s, set, browseFile }: { s: Partial<Settings>; set: SettingsSetter; browse?: (key: keyof Settings) => void; browseFile: (key: keyof Settings) => void }) {
  return (
    <>
      <Section title="Authentication">
        <Row label="Cookies path">
          <div className="flex gap-2">
            <Input value={s.apple_cookies_path || ''} onChange={(e) => set('apple_cookies_path', e.target.value)} />
            <Button variant="outline" size="sm" onClick={() => browseFile('apple_cookies_path')}>Browse</Button>
          </div>
        </Row>
        <Row label="Metadata language" help="e.g. en-US, ja-JP">
          <Input value={s.apple_language || 'en-US'} onChange={(e) => set('apple_language', e.target.value)} className="w-28" />
        </Row>
      </Section>

      <Section title="Output">
        <Row label="Output path">
          <Input value={s.apple_output_path || 'Apple Music'} onChange={(e) => set('apple_output_path', e.target.value)} />
        </Row>
        <Row label="Temp path">
          <Input value={s.apple_temp_path || 'temp'} onChange={(e) => set('apple_temp_path', e.target.value)} />
        </Row>
        <Row label="Download mode">
          <Select value={s.apple_download_mode || 'ytdlp'} onValueChange={(v) => set('apple_download_mode', v)}>
            <SelectTrigger className="w-40"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="ytdlp">yt-dlp</SelectItem>
              <SelectItem value="nm3u8dlre">N_m3u8DL-RE</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Row label="Max filename length">
          <Input type="number" value={s.apple_truncate ?? 40} onChange={(e) => set('apple_truncate', Number(e.target.value))} className="w-28" />
        </Row>
      </Section>

      <Section title="Cover Art">
        <Row label="Format">
          <Select value={s.apple_cover_format || 'jpg'} onValueChange={(v) => set('apple_cover_format', v)}>
            <SelectTrigger className="w-28"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="jpg">JPG</SelectItem>
              <SelectItem value="png">PNG</SelectItem>
              <SelectItem value="raw">RAW</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Row label="Max size (px)">
          <Input type="number" value={s.apple_cover_size ?? 1200} onChange={(e) => set('apple_cover_size', Number(e.target.value))} className="w-28" />
        </Row>
        <Check2 id="apple_save_cover" label="Save cover art file" checked={s.apple_save_cover ?? false} onChange={(v) => set('apple_save_cover', v)} />
      </Section>

      <ToggleSection title="Lyrics" enabled={!s.apple_no_synced_lyrics} onToggle={(v) => set('apple_no_synced_lyrics', !v)}>
        <Row label="Synced lyrics format">
          <Select value={s.apple_synced_lyrics_format || 'lrc'} onValueChange={(v) => set('apple_synced_lyrics_format', v)}>
            <SelectTrigger className="w-28"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="lrc">LRC</SelectItem>
              <SelectItem value="srt">SRT</SelectItem>
              <SelectItem value="ttml">TTML</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Check2 id="apple_synced_lyrics_only" label="Synced lyrics only (skip if unavailable)" checked={s.apple_synced_lyrics_only ?? false} onChange={(v) => set('apple_synced_lyrics_only', v)} />
      </ToggleSection>

      <ToggleSection title="Extra Tags" enabled={s.apple_fetch_extra_tags ?? false} onToggle={(v) => set('apple_fetch_extra_tags', v)}>
        <Check2 id="apple_use_album_date" label="Use album release date for songs" checked={s.apple_use_album_date ?? false} onChange={(v) => set('apple_use_album_date', v)} />
        <Row label="Date tag template" help="strftime format">
          <Input value={s.apple_date_tag_template || '%Y-%m-%dT%H:%M:%SZ'} onChange={(e) => set('apple_date_tag_template', e.target.value)} />
        </Row>
        <Row label="Exclude tags" help="Comma-separated">
          <Input value={s.apple_exclude_tags || ''} onChange={(e) => set('apple_exclude_tags', e.target.value)} placeholder="e.g. lyrics,comment" />
        </Row>
      </ToggleSection>

      <Section title="File Templates">
        <Row label="Album folder" help="{album_artist}, {album}">
          <Input value={s.apple_template_folder_album || '{album_artist}/{album}'} onChange={(e) => set('apple_template_folder_album', e.target.value)} />
        </Row>
        <Row label="Compilation folder">
          <Input value={s.apple_template_folder_compilation || 'Compilations/{album}'} onChange={(e) => set('apple_template_folder_compilation', e.target.value)} />
        </Row>
        <Row label="No-album folder">
          <Input value={s.apple_template_folder_no_album || '{album_artist}/Unknown Album'} onChange={(e) => set('apple_template_folder_no_album', e.target.value)} />
        </Row>
        <Row label="Single-disc file" help="{track}, {title}">
          <Input value={s.apple_template_file_single_disc || '{track:02d} {title}'} onChange={(e) => set('apple_template_file_single_disc', e.target.value)} />
        </Row>
        <Row label="Multi-disc file" help="{disc}, {track}, {title}">
          <Input value={s.apple_template_file_multi_disc || '{disc}-{track:02d} {title}'} onChange={(e) => set('apple_template_file_multi_disc', e.target.value)} />
        </Row>
        <Row label="No-album file">
          <Input value={s.apple_template_file_no_album || '{title}'} onChange={(e) => set('apple_template_file_no_album', e.target.value)} />
        </Row>
        <Row label="Playlist file">
          <Input value={s.apple_template_file_playlist || 'Playlists/{playlist_title}/{track:02d} {title}'} onChange={(e) => set('apple_template_file_playlist', e.target.value)} />
        </Row>
      </Section>

      <ToggleSection title="Music Video" enabled={s.apple_mv_enabled ?? false} onToggle={(v) => set('apple_mv_enabled', v)}>
        <Row label="Codec priority">
          <Select value={s.apple_mv_codec_priority || 'h264'} onValueChange={(v) => set('apple_mv_codec_priority', v)}>
            <SelectTrigger className="w-28"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="h264">H.264</SelectItem>
              <SelectItem value="h265">H.265</SelectItem>
              <SelectItem value="ask">Ask</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Row label="Remux mode">
          <Select value={s.apple_remux_mode || 'ffmpeg'} onValueChange={(v) => set('apple_remux_mode', v)}>
            <SelectTrigger className="w-36"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="ffmpeg">FFmpeg</SelectItem>
              <SelectItem value="mp4box">MP4Box</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Row label="Remux format">
          <Select value={s.apple_mv_remux_format || 'm4v'} onValueChange={(v) => set('apple_mv_remux_format', v)}>
            <SelectTrigger className="w-28"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="m4v">M4V</SelectItem>
              <SelectItem value="mp4">MP4</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Row label="Max resolution">
          <Select value={s.apple_mv_resolution || '1080p'} onValueChange={(v) => set('apple_mv_resolution', v)}>
            <SelectTrigger className="w-28"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="240p">240p</SelectItem>
              <SelectItem value="360p">360p</SelectItem>
              <SelectItem value="480p">480p</SelectItem>
              <SelectItem value="540p">540p</SelectItem>
              <SelectItem value="720p">720p</SelectItem>
              <SelectItem value="1080p">1080p</SelectItem>
              <SelectItem value="1440p">1440p</SelectItem>
              <SelectItem value="2160p">2160p</SelectItem>
            </SelectContent>
          </Select>
        </Row>
        <Row label="Uploaded video quality">
          <Select value={s.apple_uploaded_video_quality || 'best'} onValueChange={(v) => set('apple_uploaded_video_quality', v)}>
            <SelectTrigger className="w-28"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="best">Best</SelectItem>
              <SelectItem value="ask">Ask</SelectItem>
            </SelectContent>
          </Select>
        </Row>
      </ToggleSection>

      <ToggleSection title="Custom Tool Paths" enabled={s.apple_custom_paths_enabled ?? false} onToggle={(v) => set('apple_custom_paths_enabled', v)}>
        {[
          ['apple_ffmpeg_path',     'FFmpeg',       'ffmpeg'],
          ['apple_mp4box_path',     'MP4Box',       'MP4Box'],
          ['apple_mp4decrypt_path', 'mp4decrypt',   'mp4decrypt'],
          ['apple_nm3u8dlre_path',  'N_m3u8DL-RE',  'N_m3u8DL-RE'],
          ['apple_wvd_path',        '.wvd file',    ''],
        ].map(([key, label, placeholder]) => (
          <Row key={key} label={label}>
            <div className="flex gap-2">
              <Input value={(s[key as keyof Settings] as string) || ''} onChange={(e) => set(key as keyof Settings, e.target.value)} placeholder={placeholder as string} />
              <Button variant="outline" size="sm" onClick={() => browseFile(key as keyof Settings)}>Browse</Button>
            </div>
          </Row>
        ))}
      </ToggleSection>

      <ToggleSection title="Wrapper" enabled={s.apple_use_wrapper ?? false} onToggle={(v) => set('apple_use_wrapper', v)}>
        <Row label="Wrapper account URL">
          <Input value={s.apple_wrapper_account_url || ''} onChange={(e) => set('apple_wrapper_account_url', e.target.value)} placeholder="https://..." />
        </Row>
        <Row label="Wrapper decrypt IP">
          <Input value={s.apple_wrapper_decrypt_ip || ''} onChange={(e) => set('apple_wrapper_decrypt_ip', e.target.value)} placeholder="127.0.0.1:8080" />
        </Row>
      </ToggleSection>

      <Section title="Misc">
        <Check2 id="apple_save_playlist" label="Save playlist file" checked={s.apple_save_playlist ?? false} onChange={(v) => set('apple_save_playlist', v)} />
        <Check2 id="apple_overwrite" label="Overwrite existing files" checked={s.apple_overwrite ?? false} onChange={(v) => set('apple_overwrite', v)} />
        <Check2 id="apple_no_exceptions" label="Don't print exceptions" checked={s.apple_no_exceptions ?? false} onChange={(v) => set('apple_no_exceptions', v)} />
        <Row label="Log level">
          <Select value={s.apple_log_level || 'INFO'} onValueChange={(v) => set('apple_log_level', v)}>
            <SelectTrigger className="w-32"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="DEBUG">Debug</SelectItem>
              <SelectItem value="INFO">Info</SelectItem>
              <SelectItem value="WARNING">Warning</SelectItem>
              <SelectItem value="ERROR">Error</SelectItem>
            </SelectContent>
          </Select>
        </Row>
      </Section>
    </>
  );
}

function SecretInput({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder?: string }) {
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
        onClick={() => setShow(s => !s)}
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
      const raw = await window.electron?.orpheus?.readSettings();
      setContent(raw ?? '');
    } catch (e) {
      setError(String(e));
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
      await window.electron?.orpheus?.writeSettings(content);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
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
          {loading ? <Loader2 className="h-4 w-4 animate-spin" /> : <FileText className="h-4 w-4" />}
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
          {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" /> : saved ? <Check className="h-3.5 w-3.5 mr-1.5" /> : null}
          {saved ? 'Saved' : 'Save'}
        </Button>
        <Button variant="ghost" size="sm" onClick={() => { setContent(null); setError(null); }}>Close</Button>
      </div>
    </div>
  );
}

const ORPHEUS_SKIP_GENERAL = new Set(['download_path', 'download_quality']);

function isSensitiveKey(key: string) {
  return /(password|secret|token|web_access_token|kc1_key|secret_key|app_secret|dev_key)/i.test(key);
}

function toLabel(key: string) {
  const s = key.replace(/_/g, ' ');
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function setNestedPath(obj: Record<string, unknown>, path: string[], value: unknown): Record<string, unknown> {
  if (path.length === 0) return obj;
  const [head, ...rest] = path;
  if (rest.length === 0) return { ...obj, [head]: value };
  return { ...obj, [head]: setNestedPath((obj[head] as Record<string, unknown>) ?? {}, rest, value) };
}

function renderLeaf(
  path: string[],
  key: string,
  val: unknown,
  config: Record<string, unknown>,
  setConfig: (c: Record<string, unknown>) => void,
): React.ReactNode {
  const id = path.join('_');
  const update = (v: unknown) => setConfig(setNestedPath(config, path, v));
  if (typeof val === 'boolean') {
    return <Check2 key={id} id={id} label={toLabel(key)} checked={val} onChange={update as (v: boolean) => void} />;
  }
  if (typeof val === 'number') {
    return (
      <Row key={id} label={toLabel(key)}>
        <Input type="number" value={val} onChange={(e) => update(Number(e.target.value))} className="w-28" />
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
    window.electron?.orpheus?.readSettings()
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
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  const save = async () => {
    if (!config) return;
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      await window.electron?.orpheus?.writeSettings(JSON.stringify(config, null, 2));
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  if (loading) return <p className="text-xs text-muted-foreground py-1">Loading settings.json…</p>;
  if (notFound) return <p className="text-xs text-muted-foreground py-1">Settings file not found — it will be created on first download.</p>;
  if (!config) return null;

  const globalObj = config['global'] as Record<string, unknown> | undefined;
  const modulesObj = config['modules'] as Record<string, unknown> | undefined;

  return (
    <div className="space-y-3">
      {globalObj && Object.entries(globalObj).map(([subKey, subVal]) => {
        if (typeof subVal !== 'object' || subVal === null || Array.isArray(subVal)) return null;
        const subObj = subVal as Record<string, unknown>;
        const entries = Object.entries(subObj).filter(([k]) => !(subKey === 'general' && ORPHEUS_SKIP_GENERAL.has(k)));
        if (entries.length === 0) return null;
        return (
          <Section key={subKey} title={toLabel(subKey)}>
            {entries.map(([leafKey, leafVal]) =>
              renderLeaf(['global', subKey, leafKey], leafKey, leafVal, config, setConfig)
            )}
          </Section>
        );
      })}

      {modulesObj && Object.entries(modulesObj).map(([modName, modVal]) => {
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
        {saving ? <Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" /> : saved ? <Check className="h-3.5 w-3.5 mr-1.5" /> : null}
        {saved ? 'Saved' : 'Save settings'}
      </Button>
    </div>
  );
}

function OrpheusDLTab({ s, set }: { s: Partial<Settings>; set: SettingsSetter }) {
  return (
    <ToggleSection title="OrpheusDL" enabled={s.orpheusDL ?? false} onToggle={(v) => set('orpheusDL', v)}>
      <RawSettingsEditor prominent />
      <OrpheusConfigEditor />
    </ToggleSection>
  );
}


function ApiKeysTab({ s, set }: { s: Partial<Settings>; set: SettingsSetter }) {
  return (
    <>
      <div className="rounded-md border border-yellow-500/30 bg-yellow-500/5 px-4 py-3 text-xs text-yellow-600 dark:text-yellow-400 mb-2">
        These credentials are stored locally in your user settings file and are never sent to our servers.
        They are required for search features on the respective platforms.
      </div>

      <Section title="Spotify Search API">
        <Row label="Client ID" help="From your Spotify Developer Dashboard app">
          <SecretInput value={s.spotify_client_id || ''} onChange={(v) => set('spotify_client_id', v)} placeholder="Your Spotify client ID" />
        </Row>
        <Row label="Client Secret" help="Keep this private">
          <SecretInput value={s.spotify_client_secret || ''} onChange={(v) => set('spotify_client_secret', v)} placeholder="Your Spotify client secret" />
        </Row>
      </Section>

      <Section title="Tidal Search API">
        <Row label="Client ID" help="From your Tidal Developer Portal app">
          <SecretInput value={s.tidal_client_id || ''} onChange={(v) => set('tidal_client_id', v)} placeholder="Your Tidal client ID" />
        </Row>
        <Row label="Client Secret" help="Keep this private">
          <SecretInput value={s.tidal_client_secret || ''} onChange={(v) => set('tidal_client_secret', v)} placeholder="Your Tidal client secret" />
        </Row>
      </Section>

      <Section title="YouTube Data API v3">
        <Row label="API Key" help="From your Google Cloud Console project">
          <SecretInput value={s.youtube_api_key || ''} onChange={(v) => set('youtube_api_key', v)} placeholder="Your YouTube Data API v3 key" />
        </Row>
      </Section>
    </>
  );
}

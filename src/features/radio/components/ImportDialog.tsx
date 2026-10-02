import { useCallback, useState } from 'react';
import { FileAudio, Link2, Loader2, Server } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { cn } from '@/utils/cn';
import { errorMessage } from '@/utils/errors';
import { type RadioStation } from '@/features/radio/api';
import { stationSubtitle } from '@/features/radio/format';
import { useSaveStations } from '@/features/radio/hooks/useRadioMutations';
import { StationIcon } from '@/features/radio/components/StationIcon';
import { tauriAPI } from '@/tauri-bridge';

type Mode = 'url' | 'file' | 'icecast';

const MODES: { id: Mode; icon: typeof Link2; label: string; hint: string; placeholder: string }[] =
  [
    {
      id: 'url',
      icon: Link2,
      label: 'URL or cURL',
      hint: 'A stream URL, an HLS manifest, a .pls/.m3u playlist — or paste a whole curl command and its headers come with it.',
      placeholder: 'https://ice6.somafm.com/groovesalad-128-mp3',
    },
    {
      id: 'file',
      icon: FileAudio,
      label: 'Playlist file',
      hint: 'A .pls or .m3u saved on this computer. Every server it lists is kept as a fallback.',
      placeholder: '',
    },
    {
      id: 'icecast',
      icon: Server,
      label: 'Icecast host',
      hint: 'Any Icecast or AzuraCast server. Its mounts are read from the standard status document.',
      placeholder: 'stream.nightride.fm',
    },
  ];

export function ImportDialog({
  open,
  onClose,
  onReview,
}: {
  open: boolean;
  onClose: () => void;
  /** Hands a single found station to the editor before anything is written. */
  onReview: (station: RadioStation) => void;
}) {
  const [mode, setMode] = useState<Mode>('url');
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [found, setFound] = useState<RadioStation[]>([]);
  const [picked, setPicked] = useState<ReadonlySet<string>>(new Set());
  const save = useSaveStations();

  const active = MODES.find((m) => m.id === mode) as (typeof MODES)[number];

  const reset = useCallback(() => {
    setText('');
    setError(null);
    setFound([]);
    setPicked(new Set());
  }, []);

  const probe = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      if (mode === 'icecast') {
        const stations = await tauriAPI.radio.importIcecast(text);
        setFound(stations);
        setPicked(new Set(stations.map((s) => s.key)));
      } else {
        // A single stream goes straight to the editor: the auto-derived name is
        // a guess, and saving first would leave the user with a station they
        // then have to go and find in order to fix.
        const parsed = mode === 'url' ? await tauriAPI.radio.parseCurl(text) : null;
        const station = parsed
          ? await tauriAPI.radio.importUrl(parsed.url, undefined, parsed.headers)
          : await tauriAPI.radio.importPlaylist(text);
        reset();
        onReview(station);
      }
    } catch (e) {
      setError(e);
      setFound([]);
    } finally {
      setBusy(false);
    }
  }, [mode, text, reset, onReview]);

  const pickFile = useCallback(async () => {
    const path = await tauriAPI.settings.openFile();
    if (!path) return;
    setText(path);
    setBusy(true);
    setError(null);
    try {
      const station = await tauriAPI.radio.importPlaylist(path);
      reset();
      onReview(station);
    } catch (e) {
      setError(e);
      setFound([]);
    } finally {
      setBusy(false);
    }
  }, [reset, onReview]);

  const commit = () => {
    const stations = found.filter((s) => picked.has(s.key));
    if (stations.length === 0) return;
    save.mutate(stations, {
      onSuccess: () => {
        reset();
        onClose();
      },
    });
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          reset();
          onClose();
        }
      }}
    >
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Add a station</DialogTitle>
          <DialogDescription>
            Nothing is saved until you confirm, so a mistyped address costs nothing.
          </DialogDescription>
        </DialogHeader>

        <div className="flex gap-1" role="tablist" aria-label="Import method">
          {MODES.map((m) => {
            const Icon = m.icon;
            return (
              <button
                key={m.id}
                type="button"
                role="tab"
                aria-selected={mode === m.id}
                onClick={() => {
                  setMode(m.id);
                  reset();
                }}
                className={cn(
                  'inline-flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-1.5 text-xs font-medium transition-colors',
                  'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
                  mode === m.id
                    ? 'bg-[var(--service-accent)]/15 text-foreground'
                    : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
                )}
              >
                <Icon className="h-3.5 w-3.5" />
                {m.label}
              </button>
            );
          })}
        </div>

        <p className="text-xs text-muted-foreground/70">{active.hint}</p>

        {mode === 'file' ? (
          <div className="flex items-center gap-2">
            <Input
              readOnly
              value={text}
              placeholder="No file chosen"
              className="flex-1 bg-muted/30"
            />
            <Button variant="outline" onClick={pickFile} disabled={busy}>
              Choose…
            </Button>
          </div>
        ) : (
          <div className="flex items-center gap-2">
            <Input
              autoFocus
              value={text}
              placeholder={active.placeholder}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && text.trim() && !busy) {
                  e.preventDefault();
                  void probe();
                }
              }}
              className="flex-1 bg-muted/30"
            />
            <Button onClick={() => void probe()} disabled={busy || !text.trim()}>
              {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : 'Next'}
            </Button>
          </div>
        )}

        {error !== null && (
          <p className="whitespace-pre-wrap break-words rounded-md border border-destructive/30 bg-destructive/5 p-2.5 text-xs text-muted-foreground">
            {errorMessage(error)}
          </p>
        )}

        {found.length > 0 && (
          <div className="max-h-60 space-y-1 overflow-y-auto rounded-md border border-border/40 p-1.5">
            {found.map((station) => (
              <label
                key={station.key}
                className="flex cursor-pointer items-center gap-2.5 rounded p-1.5 hover:bg-muted/40"
              >
                <Checkbox
                  checked={picked.has(station.key)}
                  onCheckedChange={(checked) =>
                    setPicked((prev) => {
                      const next = new Set(prev);
                      if (checked) next.add(station.key);
                      else next.delete(station.key);
                      return next;
                    })
                  }
                />
                <StationIcon
                  src={station.favicon}
                  className="h-8 w-8 shrink-0 overflow-hidden rounded bg-muted/50"
                  iconClassName="h-3.5 w-3.5"
                />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm">{station.name}</span>
                  <span className="block truncate text-[10px] uppercase tracking-wide text-muted-foreground/50">
                    {stationSubtitle(station)}
                    {station.altUrls.length > 0 && ` · ${station.altUrls.length} mirrors`}
                  </span>
                </span>
              </label>
            ))}
          </div>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={commit} disabled={picked.size === 0 || save.isPending}>
            {save.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              `Save ${picked.size || ''}`.trim()
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

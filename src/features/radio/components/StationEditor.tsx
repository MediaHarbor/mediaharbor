import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ImagePlus, Loader2, RotateCcw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { CoverCropDialog } from '@/features/library/components/CoverCropDialog';
import { StationIcon } from '@/features/radio/components/StationIcon';
import {
  Field,
  HeaderTable,
  ProbeResult,
  UrlList,
} from '@/features/radio/components/StationEditorFields';
import { errorMessage } from '@/utils/errors';
import { useStationCoverUrl } from '@/features/radio/hooks/useStationCover';
import {
  changedFields,
  STREAM_KINDS,
  toEdit,
  toForm,
  type FormState,
} from '@/features/radio/stationForm';
import {
  tauriAPI,
  type RadioStation,
  type RadioStreamKind,
  type RadioStreamProbe,
} from '@/tauri-bridge';

/** The plain-text fields, behind a disclosure because most stations never need them. */
const DETAIL_FIELDS: [keyof FormState, string, string][] = [
  ['tags', 'Genres', 'jazz, ambient'],
  ['country', 'Country', 'Germany'],
  ['countryCode', 'Country code', 'DE'],
  ['language', 'Language', 'english'],
  ['codec', 'Codec', 'MP3'],
  ['bitrate', 'Bitrate (kbps)', '128'],
  ['homepage', 'Website', 'https://…'],
  ['favicon', 'Icon URL', 'https://…'],
];

interface Props {
  /** The station to edit, or the freshly imported one awaiting its first save. */
  station: RadioStation;
  /** True when the station has not been written to the library yet. */
  unsaved?: boolean;
  onClose: () => void;
  onSaved?: (station: RadioStation) => void;
}

/**
 * Edits everything about a station.
 *
 * For a directory station only the *changed* fields are stored, so the rest keep
 * tracking whatever the directory says — that is what "Reset" puts back.
 *
 * The caller mounts this only when there is a station, and keys it by station,
 * so the form seeds itself once from props rather than being nullable and
 * re-filled by an effect on every change of subject.
 */
export function StationEditor({ station, unsaved = false, onClose, onSaved }: Props) {
  const [form, setForm] = useState<FormState>(() => toForm(station));
  const [base, setBase] = useState<RadioStation>(station);
  const [coverFile, setCoverFile] = useState<File | null>(null);
  const [coverId, setCoverId] = useState<string | null>(station.coverId ?? null);
  const [probe, setProbe] = useState<RadioStreamProbe | null>(null);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const coverUrl = useStationCoverUrl(coverId, form.favicon || null);

  const key = station.key;

  useEffect(() => {
    // A saved station is re-read so the editor can tell a directory value from a
    // change of yours; an unsaved import has no directory row to compare with.
    if (unsaved) return;
    let cancelled = false;
    void tauriAPI.radio
      .stationDetail(key)
      .then((detail) => {
        if (!cancelled && detail) setBase(detail.base);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [key, unsaved]);

  const set = useCallback(<K extends keyof FormState>(field: K, value: FormState[K]) => {
    setForm((prev) => ({ ...prev, [field]: value }));
  }, []);

  const baseForm = useMemo(() => toForm(base), [base]);
  const overridden = useMemo(() => changedFields(form, baseForm), [form, baseForm]);

  const revert = useCallback(
    (field: keyof FormState) => setForm((prev) => ({ ...prev, [field]: baseForm[field] })),
    [baseForm]
  );

  const test = useCallback(async () => {
    setTesting(true);
    setProbe(null);
    try {
      const edit = toEdit(form);
      setProbe(await tauriAPI.radio.testStream(form.streamUrl.trim(), edit.headers));
    } catch (e) {
      setProbe({ ok: false, status: 0, streamKind: form.streamKind, message: errorMessage(e) });
    } finally {
      setTesting(false);
    }
  }, [form]);

  /** Accepts a whole `curl` command, which is how these headers are found. */
  const pasteCurl = useCallback(async () => {
    try {
      const text = await navigator.clipboard.readText();
      const parsed = await tauriAPI.radio.parseCurl(text);
      setForm((prev) => ({
        ...prev,
        streamUrl: parsed.url || prev.streamUrl,
        headers: Object.entries(parsed.headers),
      }));
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  }, []);

  const save = useCallback(async () => {
    if (!form.name.trim() || !form.streamUrl.trim()) {
      setError('A station needs a name and a stream URL.');
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const edit = toEdit(form);
      if (unsaved) {
        // Nothing exists to diff against yet, so the import is written first and
        // the edit lands on top of it.
        await tauriAPI.radio.saveStations([station]);
      }
      await tauriAPI.radio.updateStation(key, edit);
      onSaved?.({ ...station, ...edit, headers: edit.headers ?? {} });
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  }, [form, key, unsaved, station, onSaved, onClose]);

  const resetAll = useCallback(async () => {
    try {
      await tauriAPI.radio.resetStation(key);
      setForm(toForm(base));
      setCoverId(null);
    } catch (e) {
      setError(errorMessage(e));
    }
  }, [key, base]);

  return (
    <>
      <Dialog open onOpenChange={(next) => !next && onClose()}>
        <DialogContent className="max-h-[86vh] overflow-y-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{unsaved ? 'Add station' : 'Edit station'}</DialogTitle>
            <DialogDescription>
              {station.source === 'custom'
                ? 'This station is yours, so everything here is simply what it is.'
                : 'Only what you change is kept — everything else keeps following the directory.'}
            </DialogDescription>
          </DialogHeader>

          <div className="flex gap-4">
            <div className="shrink-0 space-y-2">
              <button
                type="button"
                onClick={() => fileRef.current?.click()}
                className="group relative block h-28 w-28 overflow-hidden rounded-lg border border-border/50 bg-muted/30"
                title="Choose a cover image"
              >
                <StationIcon src={coverUrl} className="h-full w-full" iconClassName="h-8 w-8" />
                <span className="absolute inset-0 flex items-center justify-center bg-background/70 opacity-0 transition-opacity group-hover:opacity-100">
                  <ImagePlus className="h-5 w-5" />
                </span>
              </button>
              <input
                ref={fileRef}
                type="file"
                accept="image/*"
                className="hidden"
                onChange={(e) => {
                  const file = e.target.files?.[0] ?? null;
                  setCoverFile(file);
                  e.target.value = '';
                }}
              />
              {coverId && (
                <button
                  type="button"
                  className="w-full text-[11px] text-muted-foreground hover:text-foreground"
                  onClick={() => {
                    setCoverId(null);
                    void tauriAPI.radio.setCover(key, null).catch(() => {});
                  }}
                >
                  Use the station&rsquo;s own icon
                </button>
              )}
            </div>

            <div className="min-w-0 flex-1 space-y-3">
              <Field
                label="Name"
                overridden={overridden.has('name')}
                onRevert={() => revert('name')}
              >
                <Input value={form.name} onChange={(e) => set('name', e.target.value)} />
              </Field>
              <Field
                label="Stream URL"
                overridden={overridden.has('streamUrl')}
                onRevert={() => revert('streamUrl')}
              >
                <div className="flex gap-2">
                  <Input
                    value={form.streamUrl}
                    onChange={(e) => set('streamUrl', e.target.value)}
                    className="flex-1 font-mono text-xs"
                  />
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void test()}
                    disabled={testing}
                  >
                    {testing ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : 'Test'}
                  </Button>
                </div>
              </Field>
              {probe && <ProbeResult probe={probe} />}
              <Field
                label="Stream type"
                overridden={overridden.has('streamKind')}
                onRevert={() => revert('streamKind')}
              >
                <Select
                  value={form.streamKind}
                  onValueChange={(v) => set('streamKind', v as RadioStreamKind)}
                >
                  <SelectTrigger className="w-48">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {STREAM_KINDS.map((k) => (
                      <SelectItem key={k.value} value={k.value}>
                        {k.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </Field>
            </div>
          </div>

          <UrlList
            urls={form.altUrls}
            overridden={overridden.has('altUrls')}
            onRevert={() => revert('altUrls')}
            onChange={(urls) => set('altUrls', urls)}
          />

          <HeaderTable
            headers={form.headers}
            onChange={(headers) => set('headers', headers)}
            onPasteCurl={() => void pasteCurl()}
          />

          <details className="rounded-lg border border-border/40 p-3">
            <summary className="cursor-pointer text-sm font-medium">Details</summary>
            <div className="mt-3 grid grid-cols-2 gap-3">
              {DETAIL_FIELDS.map(([field, label, placeholder]) => (
                <Field
                  key={field}
                  label={label}
                  overridden={overridden.has(field)}
                  onRevert={() => revert(field)}
                >
                  <Input
                    value={form[field] as string}
                    placeholder={placeholder}
                    inputMode={field === 'bitrate' ? 'numeric' : undefined}
                    onChange={(e) => set(field, e.target.value as never)}
                  />
                </Field>
              ))}
            </div>
          </details>

          {error && (
            <p className="whitespace-pre-wrap break-words rounded-md border border-destructive/30 bg-destructive/5 p-2.5 text-xs">
              {error}
            </p>
          )}

          <DialogFooter className="sm:justify-between">
            {!unsaved && overridden.size > 0 ? (
              <Button variant="ghost" size="sm" onClick={() => void resetAll()}>
                <RotateCcw className="mr-1.5 h-3.5 w-3.5" /> Reset all
              </Button>
            ) : (
              <span />
            )}
            <div className="flex gap-2">
              <Button variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button onClick={() => void save()} disabled={saving}>
                {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : unsaved ? 'Add' : 'Save'}
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <CoverCropDialog
        open={!!coverFile}
        onOpenChange={(next) => !next && setCoverFile(null)}
        file={coverFile}
        onConfirm={async (jpegBase64) => {
          if (unsaved) await tauriAPI.radio.saveStations([station]);
          await tauriAPI.radio.setCover(key, jpegBase64);
          const detail = await tauriAPI.radio.stationDetail(key);
          setCoverId(detail?.base.coverId ?? null);
          setCoverFile(null);
        }}
      />
    </>
  );
}

import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Pencil, Loader2 } from 'lucide-react';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { errorMessage, libraryKeys, type TrackDto } from '@/features/library/api';
import { useNotificationStore } from '@/stores/useNotificationStore';
import { formatDurationShort } from '@/utils/formatters';
import { tauriAPI } from '@/tauri-bridge';

interface TrackInfoPanelProps {
  track: TrackDto | null;
  onOpenChange: (open: boolean) => void;
}

const EDITABLE = [
  ['title', 'Title'],
  ['artist', 'Artist'],
  ['album', 'Album'],
  ['albumArtist', 'Album artist'],
  ['year', 'Year'],
  ['genre', 'Genre'],
  ['trackNo', 'Track number'],
  ['trackTotal', 'Tracks on release'],
  ['discNo', 'Disc number'],
  ['discTotal', 'Discs on release'],
  ['composer', 'Composer'],
  ['lyricist', 'Lyricist'],
  ['producer', 'Producer'],
  ['label', 'Label'],
  ['isrc', 'ISRC'],
  ['barcode', 'Barcode'],
  ['bpm', 'BPM'],
  ['grouping', 'Grouping'],
] as const;

type EditKey = (typeof EDITABLE)[number][0];

const NUMERIC: ReadonlySet<string> = new Set([
  'trackNo',
  'trackTotal',
  'discNo',
  'discTotal',
  'bpm',
]);

function initialEdits(t: TrackDto): Record<EditKey, string> {
  return {
    title: t.title ?? '',
    artist: t.artist ?? '',
    album: t.album ?? '',
    albumArtist: t.album_artist ?? '',
    year: t.date ?? t.year ?? '',
    genre: t.genre ?? '',
    trackNo: t.track_no ? String(t.track_no) : '',
    trackTotal: t.track_total ? String(t.track_total) : '',
    discNo: t.disc_no ? String(t.disc_no) : '',
    discTotal: t.disc_total ? String(t.disc_total) : '',
    composer: t.composer ?? '',
    lyricist: t.lyricist ?? '',
    producer: t.producer ?? '',
    label: t.label ?? '',
    isrc: t.isrc ?? '',
    barcode: t.barcode ?? '',
    bpm: t.bpm ? String(t.bpm) : '',
    grouping: t.grouping ?? '',
  };
}

function TagEditor({ track, onDone }: { track: TrackDto; onDone: () => void }) {
  const [edits, setEdits] = useState(() => initialEdits(track));
  const [saving, setSaving] = useState(false);
  const notify = useNotificationStore((s) => s.addNotification);
  const queryClient = useQueryClient();

  const save = async () => {
    setSaving(true);
    try {
      const req: Record<string, unknown> = { path: track.path };
      for (const [key] of EDITABLE) {
        const raw = edits[key].trim();
        req[key] = NUMERIC.has(key) ? (raw ? Number(raw) : null) : raw;
      }
      await tauriAPI.library.writeTags(req as Record<string, unknown> & { path: string });
      await queryClient.invalidateQueries({ queryKey: libraryKeys.all });
      notify({ type: 'success', title: 'Tags saved', message: track.path });
      onDone();
    } catch (e) {
      notify({ type: 'error', title: 'Could not write tags', message: errorMessage(e) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-3">
        {EDITABLE.map(([key, label]) => (
          <div key={key} className="space-y-1">
            <Label htmlFor={`tag-${key}`} className="text-[11px] text-muted-foreground/70">
              {label}
            </Label>
            <Input
              id={`tag-${key}`}
              value={edits[key]}
              inputMode={NUMERIC.has(key) ? 'numeric' : undefined}
              onChange={(e) => setEdits((p) => ({ ...p, [key]: e.target.value }))}
              className="h-8 text-[13px]"
            />
          </div>
        ))}
      </div>
      <p className="text-[11px] text-muted-foreground/60">
        Separate several composers or artists with a semicolon. Writes to the file itself and
        rescans it; embedded lyrics and ReplayGain are left alone.
      </p>
      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onDone} disabled={saving}>
          Cancel
        </Button>
        <Button size="sm" onClick={() => void save()} disabled={saving}>
          {saving && <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />}
          Save tags
        </Button>
      </div>
    </div>
  );
}

interface Row {
  label: string;
  value: string;
}

function bytes(n: number): string {
  if (n <= 0) return '—';
  const units = ['B', 'KB', 'MB', 'GB'];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

function khz(rate: number): string {
  const v = rate / 1000;
  return `${Number.isInteger(v) ? v : v.toFixed(1)} kHz`;
}

function numbered(no?: number | null, total?: number | null): string | null {
  if (!no && !total) return null;
  if (no && total) return `${no} of ${total}`;
  return String(no ?? total);
}

function section(title: string, rows: Array<Row | null>): { title: string; rows: Row[] } | null {
  const kept = rows.filter((r): r is Row => r !== null && r.value.trim() !== '');
  return kept.length > 0 ? { title, rows: kept } : null;
}

function text(label: string, value?: string | null): Row | null {
  return value ? { label, value } : null;
}

export function TrackInfoPanel({ track, onOpenChange }: TrackInfoPanelProps) {
  const [editing, setEditing] = useState(false);
  if (!track) return null;

  const sections = [
    section('Release', [
      text('Album', track.album),
      text('Album artist', track.album_artist),
      { label: 'Track', value: numbered(track.track_no, track.track_total) ?? '' },
      { label: 'Disc', value: numbered(track.disc_no, track.disc_total) ?? '' },
      text('Released', track.date ?? track.year),
      text('Originally released', track.original_date),
      text('Genre', track.genre),
      text('Label', track.label),
      text('Grouping', track.grouping),
      text('Copyright', track.copyright),
    ]),
    section('Audio', [
      text('Codec', track.codec),
      { label: 'Sample rate', value: track.sample_rate ? khz(track.sample_rate) : '' },
      { label: 'Bit depth', value: track.bit_depth ? `${track.bit_depth}-bit` : '' },
      { label: 'Bitrate', value: track.bitrate ? `${track.bitrate} kbps` : '' },
      { label: 'Channels', value: track.channels ? String(track.channels) : '' },
      {
        label: 'Duration',
        value: track.duration_secs ? formatDurationShort(track.duration_secs) : '',
      },
    ]),
    section('Credits', [
      text('Composer', track.composer),
      text('Lyricist', track.lyricist),
      text('Producer', track.producer),
      text('Conductor', track.conductor),
      text('Performer', track.performer),
      text('Engineer', track.engineer),
      text('Mixer', track.mixer),
    ]),
    section('Identifiers', [
      text('ISRC', track.isrc),
      text('Barcode', track.barcode),
      text('MusicBrainz recording', track.mb_recording_id),
      text('MusicBrainz release', track.mb_release_id),
    ]),
    section('Analysis', [
      { label: 'BPM', value: track.bpm ? String(track.bpm) : '' },
      text('ReplayGain (track)', track.rg_track_gain),
      text('Peak (track)', track.rg_track_peak),
      text('ReplayGain (album)', track.rg_album_gain),
      text('Peak (album)', track.rg_album_peak),
    ]),
    section('File', [
      { label: 'Size', value: bytes(track.size) },
      { label: 'Lyrics', value: track.has_lyrics ? 'Embedded' : '' },
      text('Comment', track.comment),
      { label: 'Plays', value: track.play_count ? String(track.play_count) : '' },
      { label: 'Path', value: track.path },
    ]),
  ].filter((s): s is { title: string; rows: Row[] } => s !== null);

  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="truncate">{track.title ?? 'Track info'}</DialogTitle>
          {track.artist && <p className="text-sm text-muted-foreground">{track.artist}</p>}
        </DialogHeader>
        {editing ? (
          <TagEditor track={track} onDone={() => setEditing(false)} />
        ) : (
          <>
            <div className="flex justify-end">
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => setEditing(true)}
              >
                <Pencil className="h-3.5 w-3.5" />
                Edit tags
              </Button>
            </div>
            <div className="space-y-5">
              {sections.map((s) => (
                <section key={s.title}>
                  <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/50">
                    {s.title}
                  </h3>
                  <dl className="space-y-1">
                    {s.rows.map((r) => (
                      <div key={r.label} className="flex gap-4 text-[13px]">
                        <dt className="w-44 shrink-0 text-muted-foreground/70">{r.label}</dt>
                        <dd className="min-w-0 flex-1 break-words font-medium">{r.value}</dd>
                      </div>
                    ))}
                  </dl>
                </section>
              ))}
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

import type { RadioStation, RadioStationEdit, RadioStreamKind } from '@/tauri-bridge';

/** Every field the editor owns, as plain strings so the inputs stay controlled. */
export interface FormState {
  name: string;
  streamUrl: string;
  streamKind: RadioStreamKind;
  altUrls: string[];
  tags: string;
  country: string;
  countryCode: string;
  language: string;
  codec: string;
  bitrate: string;
  homepage: string;
  favicon: string;
  headers: [string, string][];
}

export const STREAM_KINDS: { value: RadioStreamKind; label: string }[] = [
  { value: 'direct', label: 'Direct stream' },
  { value: 'hls', label: 'HLS (.m3u8)' },
  { value: 'playlistFile', label: 'Playlist file' },
];

export function toForm(station: RadioStation): FormState {
  return {
    name: station.name,
    streamUrl: station.streamUrl,
    streamKind: station.streamKind,
    altUrls: station.altUrls ?? [],
    tags: station.tags ?? '',
    country: station.country ?? '',
    countryCode: station.countryCode ?? '',
    language: station.language ?? '',
    codec: station.codec ?? '',
    bitrate: station.bitrate ? String(station.bitrate) : '',
    homepage: station.homepage ?? '',
    favicon: station.favicon ?? '',
    headers: Object.entries(station.headers ?? {}),
  };
}

export function toEdit(form: FormState): RadioStationEdit {
  const headers: Record<string, string> = {};
  for (const [name, value] of form.headers) {
    const key = name.trim();
    if (key) headers[key] = value.trim();
  }
  return {
    name: form.name.trim(),
    streamUrl: form.streamUrl.trim(),
    streamKind: form.streamKind,
    altUrls: form.altUrls.map((u) => u.trim()).filter(Boolean),
    tags: form.tags.trim(),
    country: form.country.trim(),
    countryCode: form.countryCode.trim(),
    language: form.language.trim(),
    codec: form.codec.trim(),
    bitrate: form.bitrate.trim() ? Number(form.bitrate) : 0,
    homepage: form.homepage.trim(),
    favicon: form.favicon.trim(),
    headers,
  };
}

/**
 * Which fields differ from what the directory published — the "changed" badges,
 * and whether "Reset all" is worth offering.
 *
 * Compared field by field rather than by `JSON.stringify`ing the two whole
 * forms: this runs on every keystroke.
 */
export function changedFields(form: FormState, base: FormState): Set<keyof FormState> {
  const changed = new Set<keyof FormState>();
  for (const field of Object.keys(form) as (keyof FormState)[]) {
    if (!same(form[field], base[field])) changed.add(field);
  }
  return changed;
}

function same(a: FormState[keyof FormState], b: FormState[keyof FormState]): boolean {
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((v, i) => String(v) === String(b[i]));
  }
  return a === b;
}

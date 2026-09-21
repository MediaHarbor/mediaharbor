import type { Platform, QualityOption } from '@/types';
import { APPLEMUSIC_WRAPPER_QUALITY, QUALITY_OPTIONS, deezerTierOf } from '@/utils/constants';
import type { Settings } from '@/types/settings';

interface BackendConfig {
  defaultKey: string;
  alt: { backend: string; key: string };
  /// Extra option lists to search when labelling an already-downloaded row, after the
  /// two backend keys. The wrapper ladder is no longer selectable but still labels
  /// history rows recorded while it was.
  extraKeys?: string[];
  /// Narrows the offered ladder for this service. Returns the list unchanged when the
  /// condition it cares about does not apply.
  filter?: (list: QualityOption[], opts: QualityFilterOpts) => QualityOption[];
  /// The tier saved in Settings for this service, in the option list's own vocabulary.
  saved?: (settings: Settings) => string | undefined;
}

export interface QualityFilterOpts {
  spotifyFree?: boolean;
  appleWrapper?: boolean;
}

/// Everything per-service about quality, in one table.
///
/// This was four parallel registries — backend keys, label-search keys, filters and a
/// switch over saved settings — each of which had to be remembered separately when a
/// service was added.
const NATIVE_BACKENDS: Record<string, BackendConfig> = {
  spotify: {
    defaultKey: 'spotify_native',
    alt: { backend: 'votify', key: 'spotify_votify' },
    filter: (list, opts) => (opts.spotifyFree ? list.filter((o) => o.value !== 'aac-high') : list),
    saved: (s) => s.spotify_native_quality || undefined,
  },
  applemusic: {
    defaultKey: 'applemusic_native',
    alt: { backend: 'gamdl', key: 'applemusic_gamdl' },
    extraKeys: ['applemusic_wrapper'],
    filter: (list, opts) => (opts.appleWrapper ? [...APPLEMUSIC_WRAPPER_QUALITY, ...list] : list),
    saved: (s) => s.apple_native_quality || undefined,
  },
  tidal: {
    defaultKey: 'tidal',
    alt: { backend: '', key: 'tidal' },
    saved: (s) => (s.tidal_quality != null ? String(s.tidal_quality) : undefined),
  },
  qobuz: {
    defaultKey: 'qobuz',
    alt: { backend: '', key: 'qobuz' },
    saved: (s) => (s.qobuz_quality != null ? String(s.qobuz_quality) : undefined),
  },
  deezer: {
    defaultKey: 'deezer',
    alt: { backend: '', key: 'deezer' },
    saved: (s) => deezerTierOf(s.deezer_quality),
  },
};

export function optionsKeyFor(platform: Platform | string, backend: string | null): string {
  const cfg = NATIVE_BACKENDS[platform];
  if (!cfg) return String(platform);
  return backend && backend === cfg.alt.backend ? cfg.alt.key : cfg.defaultKey;
}

function candidateKeys(platform: string): string[] {
  const cfg = NATIVE_BACKENDS[platform];
  if (!cfg) return [platform];
  return [...new Set([cfg.defaultKey, cfg.alt.key, ...(cfg.extraKeys ?? []), platform])];
}

export function resolveQualityOptions(
  platform: Platform | string,
  backend: string | null,
  opts?: QualityFilterOpts
): QualityOption[] {
  const list = QUALITY_OPTIONS[optionsKeyFor(platform, backend)] || QUALITY_OPTIONS[platform] || [];
  const cfg = NATIVE_BACKENDS[String(platform)];
  return cfg?.filter ? cfg.filter(list, opts || {}) : list;
}

/// `<option list>:<code>` → label, built once. `qualityLabel` runs per rendered
/// download-history row, and a linear scan of up to four ladders per row is work the
/// list re-did on every render.
const LABEL_BY_KEYED_VALUE = new Map<string, string>(
  Object.entries(QUALITY_OPTIONS).flatMap(([key, list]) =>
    list.map((o) => [`${key}:${o.value}`, o.label] as const)
  )
);

export function qualityLabel(platform: string | null | undefined, code: string): string {
  if (!platform) return code;
  for (const key of candidateKeys(platform)) {
    const label = LABEL_BY_KEYED_VALUE.get(`${key}:${code}`);
    if (label) return label;
  }
  return code;
}

/// The quality a download should start on: the one saved in Settings for that
/// service. Without this the dialog opened on the first entry of the list every
/// time, so the saved default was only ever honoured when it happened to be first.
///
/// Returns undefined when the saved value is not among the options actually on
/// offer — a Spotify free account, or a backend that publishes a different ladder.
export function savedDefaultQuality(
  platform: Platform | string,
  options: QualityOption[],
  settings: Settings | null | undefined
): string | undefined {
  if (!settings) return undefined;
  const saved = NATIVE_BACKENDS[String(platform)]?.saved?.(settings);
  return options.some((o) => o.value === saved) ? saved : undefined;
}

/// The YT Music dialog offers a 0-10 slider rather than a ladder; this is the wire
/// value it maps to. Written out at both call sites before, with no shared constant.
export function ytMusicSliderQuality(value: number): string {
  return String(Math.round((10 - value) * 0.9));
}

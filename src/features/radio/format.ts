import type { RadioFacetKind, RadioStation } from '@/tauri-bridge';

const FACET_LABELS: Record<RadioFacetKind, string> = {
  tags: 'Genres',
  countries: 'Countries',
  languages: 'Languages',
  codecs: 'Quality',
};

export function facetLabel(kind: RadioFacetKind): string {
  return FACET_LABELS[kind];
}

/** `MP3 128k`, or whichever half of it the directory actually knows. */
export function qualityLabel(station: RadioStation): string | null {
  if (station.codec && station.bitrate) return `${station.codec} ${station.bitrate}k`;
  return station.codec ?? (station.bitrate ? `${station.bitrate}k` : null);
}

/** The first few tags, for the one line a card has room for. */
export function tagLabel(station: RadioStation, max = 3): string | null {
  if (!station.tags) return null;
  const tags = station.tags
    .split(',')
    .map((t) => t.trim())
    .filter(Boolean);
  return tags.length ? tags.slice(0, max).join(' · ') : null;
}

/**
 * The one line under a station's name.
 *
 * A card has room for one fact and a row has room for three, which is why the
 * two used to compute it separately — and why the same station read "MP3 128k"
 * as a card and "MP3 128k · jazz · Germany" as a row. One function, one flag.
 */
export function stationSubtitle(station: RadioStation, opts?: { detailed?: boolean }): string {
  const parts = [qualityLabel(station), tagLabel(station, opts?.detailed ? 2 : 3), station.country];
  const line = opts?.detailed ? parts.filter(Boolean).join(' · ') : parts.find(Boolean);
  return line || 'Live';
}

import type {
  RadioDirectorySource,
  RadioFacet,
  RadioFacetKind,
  RadioList,
  RadioListDetail,
  RadioSearchRequest,
  RadioStation,
} from '@/tauri-bridge';
import { tauriAPI } from '@/tauri-bridge';

export type {
  RadioDirectorySource,
  RadioFacet,
  RadioFacetKind,
  RadioList,
  RadioListDetail,
  RadioStation,
};

/**
 * Every radio query key in one place. Inline arrays were how these were written
 * before, which meant a typo produced a second cache entry instead of an error.
 */
export const radioKeys = {
  all: ['radio'] as const,
  sources: () => ['radio', 'sources'] as const,
  facets: (kind: RadioFacetKind, source: string | null) =>
    ['radio', 'facets', kind, source ?? 'all'] as const,
  /** The whole tag list, held for local prefix matching — not a facet page. */
  tagCorpus: () => ['radio', 'tag-corpus'] as const,
  suggest: (prefix: string) => ['radio', 'suggest', prefix] as const,
  stations: (scope: string, params: unknown) => ['radio', 'stations', scope, params] as const,
  favorites: () => ['radio', 'favorites'] as const,
  recent: () => ['radio', 'recent'] as const,
  lists: () => ['radio', 'lists'] as const,
  list: (id: number) => ['radio', 'list', id] as const,
};

/**
 * How long each kind of radio answer stays fresh.
 *
 * `lib/queryClient.ts` already sets five minutes globally, so only the ones that
 * genuinely differ are named here.
 */
export const RADIO_FRESH = {
  /** Favourites, history and playlists — cheap, local, and edited by the user. */
  list: 60_000,
  /** A home shelf. Its contents barely move and it is fetched six at a time. */
  shelf: 10 * 60_000,
  /** Browse facets. The directories publish these once a day at most. */
  facets: 24 * 60 * 60_000,
} as const;

export const PAGE_SIZE = 60;

/** One page of stations. `sources` is left off when every enabled source counts. */
export function searchStations(req: RadioSearchRequest): Promise<RadioStation[]> {
  return tauriAPI.radio.search({ limit: PAGE_SIZE, ...req });
}

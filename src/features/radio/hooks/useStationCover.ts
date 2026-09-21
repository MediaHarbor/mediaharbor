import { useMemo } from 'react';
import { useCoverUrls, useVisibleCoverUrls } from '@/features/library/useCoverUrls';
import type { RadioStation } from '@/features/radio/api';

/** A resolved-cover lookup, keyed by `coverId`. */
export type StationCovers = Record<string, string | null>;

/**
 * The image to draw for a station: the user's own cover if there is one,
 * otherwise the directory's icon URL.
 *
 * A `coverId` lives in the shared cover cache and has to be resolved to a local
 * URL through the streaming server — the same path album and playlist art take.
 */
export function coverOf(station: RadioStation, resolved: StationCovers): string | null {
  const own = station.coverId ? resolved[station.coverId] : null;
  return own ?? station.favicon ?? null;
}

/**
 * Resolves the covers of a whole collection in one pass.
 *
 * Per-card resolution meant one `useState` + `useEffect` + promise for every
 * mounted tile — fifty or more inside the virtualizer, each restarting at `null`
 * on every scroll remount, and most of them only to resolve `null` because a
 * directory station has no cover of its own.
 */
export function useStationCovers(stations: RadioStation[]): StationCovers {
  const ids = useMemo(() => stations.map((s) => s.coverId), [stations]);
  return useVisibleCoverUrls(ids);
}

/**
 * The single-cover case: the editor, resolving the one image it is showing while
 * the user swaps it about.
 */
export function useStationCoverUrl(
  coverId: string | null | undefined,
  favicon: string | null | undefined
): string | null {
  const ids = useMemo(() => [coverId], [coverId]);
  const resolved = useCoverUrls(ids);
  return (coverId ? resolved[coverId] : null) ?? favicon ?? null;
}

import type { CSSProperties, RefObject } from 'react';

import { useElementWidth } from '@/features/library/hooks/useElementWidth';
import { AUTO_COLUMNS, useRadioStore } from '@/features/radio/stores/useRadioStore';

/** Matches the library grid so the two pages break at the same widths. */
function autoColumnsFor(width: number): number {
  if (width >= 1536) return 7;
  if (width >= 1280) return 6;
  if (width >= 1024) return 5;
  if (width >= 768) return 4;
  if (width >= 640) return 3;
  return 2;
}

/** The user's column count, or the width-derived one when they left it on auto. */
export function columnsFor(width: number, preference: number = AUTO_COLUMNS): number {
  return preference === AUTO_COLUMNS ? autoColumnsFor(width) : preference;
}

export function gridStyle(cols: number): CSSProperties {
  return { gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` };
}

/**
 * How wide a results surface is and how many columns fit in it.
 *
 * Measured where the surface is laid out, so the stepper's bounds, the grid and
 * the virtualizer's row height all read one observation — the count used to
 * round-trip child → effect → parent state → child on every resize.
 */
export function useGridMetrics(ref: RefObject<HTMLElement | null>): {
  width: number;
  cols: number;
} {
  const width = useElementWidth(ref);
  const view = useRadioStore((s) => s.view);
  const preference = useRadioStore((s) => s.columns);
  return { width, cols: view === 'list' ? 1 : columnsFor(width, preference) };
}

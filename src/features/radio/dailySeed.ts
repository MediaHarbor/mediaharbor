import { hashString } from '@/utils/hash';

/**
 * A number that is the same all day and different tomorrow.
 *
 * The Home feed needs to vary without knowing anything about the listener. A
 * date-derived seed gives that for free: the same shelves all day, so the page
 * is stable while you use it, and a new set in the morning. Nothing here learns
 * from listening — a real personalised feed is a separate, later thing.
 */
export function todaySeed(now: Date = new Date()): number {
  return hashString(`${now.getFullYear()}-${now.getMonth() + 1}-${now.getDate()}`);
}

/** xorshift32 — small, deterministic, and enough to shuffle a shortlist. */
function nextRandom(state: number): [number, number] {
  let x = state || 0x9e3779b9;
  x ^= x << 13;
  x >>>= 0;
  x ^= x >>> 17;
  x ^= x << 5;
  x >>>= 0;
  return [x, x / 0x100000000];
}

/**
 * `count` items chosen from the head of `items`, the same way every time for a
 * given seed.
 *
 * Choosing from a shortlist rather than the whole list is deliberate: the tail
 * of a facet corpus is thousands of tags with one station each, and a shelf
 * titled after one of those is a dead end.
 */
export function pickSeeded<T>(items: T[], count: number, seed: number, poolSize = 40): T[] {
  const pool = items.slice(0, Math.max(poolSize, count));
  if (pool.length <= count) return pool.slice(0, count);

  const picked: T[] = [];
  const taken = new Set<number>();
  let state = seed;
  // Bounded rather than while-true: a pathological seed must not spin here.
  for (let attempt = 0; picked.length < count && attempt < pool.length * 8; attempt += 1) {
    const [nextState, fraction] = nextRandom(state);
    state = nextState;
    const index = Math.floor(fraction * pool.length);
    if (taken.has(index)) continue;
    taken.add(index);
    picked.push(pool[index]);
  }
  return picked;
}

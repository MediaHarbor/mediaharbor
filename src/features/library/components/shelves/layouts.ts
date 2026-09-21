import { hashString } from '@/utils/hash';

export type ShelfItem = Record<string, unknown> & { kind: string };

export interface Shelf {
  id: string;
  title: string;
  subtitle: string | null;
  category?: string;
  items: ShelfItem[];
}

export type ShelfLayout =
  'genre-pills' | 'shortcuts' | 'hero' | 'ranked' | 'compact' | 'mix-tiles' | 'standard';

function allItemsAre(shelf: Shelf, kind: string): boolean {
  return shelf.items.length > 0 && shelf.items.every((it) => it.kind === kind);
}

export function layoutFor(shelf: Shelf): ShelfLayout {
  if (shelf.category === 'genre') return 'genre-pills';
  if (allItemsAre(shelf, 'page_link')) return 'genre-pills';

  if (allItemsAre(shelf, 'track')) return 'ranked';

  switch (shelf.category) {
    case 'hero':
      return shelf.items.length > 4 ? 'shortcuts' : 'hero';
    case 'charts':
      return 'ranked';
    case 'recently_played':
      return 'compact';
    case 'daily_mix':
    case 'stations':
      return 'mix-tiles';
    default:
      return 'standard';
  }
}

export function pickCoverId(it: ShelfItem): string | null {
  const single = it.cover_id as string | undefined;
  if (single) return single;
  const arr = it.cover_ids as string[] | undefined;
  if (Array.isArray(arr) && arr.length > 0 && typeof arr[0] === 'string') return arr[0];
  return null;
}

export function pickTitle(it: ShelfItem): string {
  return (
    (it.title as string | undefined) ??
    (it.name as string | undefined) ??
    (it.display as string | undefined) ??
    ''
  );
}

export function pickSubtitle(it: ShelfItem): string | null {
  if (it.kind === 'episode') {
    const show = it.show_name as string | undefined;
    return show ? `Episode · ${show}` : 'Episode';
  }
  if (typeof it.artist === 'string' && it.artist) return it.artist;
  if (typeof it.subtitle === 'string' && it.subtitle) return it.subtitle;
  if (it.kind === 'playlist' && typeof it.owner === 'string' && it.owner) return it.owner;
  if (it.kind === 'playlist' && typeof it.track_count === 'number' && it.track_count > 0) {
    return `${it.track_count} tracks`;
  }
  if (it.kind === 'mix') return 'Mix';
  return null;
}

export function pickReleaseLabel(it: ShelfItem): string | null {
  const year = it.year as string | number | undefined;
  if (typeof year === 'number' && year > 0) return String(year);
  if (typeof year === 'string' && year.trim()) return year.trim().slice(0, 4);
  const date = it.release_date as string | undefined;
  if (typeof date === 'string' && date.length >= 4) return date.slice(0, 4);
  return null;
}

export function gradientFor(seed: string): string {
  const hue = hashString(seed) % 360;
  return `linear-gradient(135deg, hsl(${hue} 62% 32%), hsl(${(hue + 48) % 360} 58% 18%))`;
}

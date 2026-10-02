import { toIpcPlatform, type ServiceKey } from '@/utils/platform-data';
import { tauriAPI } from '@/tauri-bridge';
import type { MediaType, PlayableTrack } from '@/stores/usePlayerStore';
export { errorMessage } from '@/utils/errors';

export interface AlbumDto {
  album_key: string;
  title: string;
  artist: string;
  year?: string | null;
  year_end?: string | null;
  cover_id?: string | null;
  track_count: number;
  artist_id?: string | null;
  disc_total?: number | null;
  track_total?: number | null;
  codec?: string | null;
  bit_depth?: number | null;
  sample_rate?: number | null;
  release_dir?: string | null;
  album_kind?: string | null;
}

export interface TrackDto {
  path: string;
  title?: string | null;
  artist?: string | null;
  album?: string | null;
  album_key?: string | null;
  album_artist?: string | null;
  year?: string | null;
  genre?: string | null;
  duration_secs?: number | null;
  track_no?: number | null;
  disc_no?: number | null;
  size: number;
  mtime_ns: number;
  is_video: boolean;
  cover_id?: string | null;
  primary_artist?: string | null;
  artist_id?: string | null;
  track_total?: number | null;
  disc_total?: number | null;
  date?: string | null;
  original_date?: string | null;
  compilation?: boolean | null;
  isrc?: string | null;
  barcode?: string | null;
  mb_recording_id?: string | null;
  mb_release_id?: string | null;
  composer?: string | null;
  lyricist?: string | null;
  producer?: string | null;
  conductor?: string | null;
  performer?: string | null;
  engineer?: string | null;
  mixer?: string | null;
  label?: string | null;
  copyright?: string | null;
  comment?: string | null;
  grouping?: string | null;
  description?: string | null;
  codec?: string | null;
  bitrate?: number | null;
  sample_rate?: number | null;
  bit_depth?: number | null;
  channels?: number | null;
  bpm?: number | null;
  rg_track_gain?: string | null;
  rg_track_peak?: string | null;
  rg_album_gain?: string | null;
  rg_album_peak?: string | null;
  has_lyrics?: boolean;
  release_dir?: string | null;
  added_at?: number | null;
  play_count?: number | null;
  last_played_at?: number | null;
}

export interface AlbumDetailDto {
  album: AlbumDto;
  tracks: TrackDto[];
}

export interface ActivityFeedItem {
  id: string;
  kind: string;
  title: string;
  subtitle: string;
  cover_id?: string | null;
  service_id?: string | null;
  occurred_at?: string | null;
  seen: boolean;
}

export interface ServiceUserDto {
  key: string;
  display: string;
  album_count: number;
  track_count: number;
  cover_id?: string | null;
}

export const libraryKeys = {
  all: ['library'] as const,
  albums: (sort: string, search: string, source: string = 'local') =>
    ['library', source, 'albums', sort, search] as const,
  tracks: (sort: string, search: string, source: string = 'local') =>
    ['library', source, 'tracks', sort, search] as const,
  videos: (sort: string, search: string, source: string = 'local') =>
    ['library', source, 'videos', sort, search] as const,
  artists: (sort: string, search: string, source: string = 'local') =>
    ['library', source, 'artists', sort, search] as const,
  playlists: (search: string, source: string = 'local') =>
    ['library', source, 'playlists', search] as const,
  recent: (kind: 'albums' | 'videos') => ['library', 'recent', kind] as const,
  album: (album_key: string, source: string = 'local') =>
    ['library', source, 'album', album_key] as const,
  recommendations: (source: string) => ['library', source, 'recommendations'] as const,
  explore: (source: string) => ['library', source, 'explore'] as const,
  explorePage: (source: string, path: string) => ['library', source, 'explorePage', path] as const,
  albumPage: (source: string, albumKey: string) =>
    ['library', source, 'albumPage', albumKey] as const,
  artistPage: (source: string, artistKey: string) =>
    ['library', source, 'artistPage', artistKey] as const,
  activityFeed: (source: string) => ['library', source, 'activityFeed'] as const,
  episodeBookmarks: (source: string) => ['library', source, 'episodeBookmarks'] as const,
  followers: (source: string) => ['library', source, 'followers'] as const,
  following: (source: string) => ['library', source, 'following'] as const,
};

export type ServicePlatform = 'local' | ServiceKey;

export async function queryLibrary<T>(req: {
  kind: 'albums' | 'tracks' | 'videos' | 'artists' | 'playlists';
  offset?: number;
  limit?: number;
  sort?: string;
  search?: string;
  source?: ServicePlatform;
}): Promise<{ items: T[]; total: number | null }> {
  const source: ServicePlatform = req.source ?? 'local';
  if (source === 'local') {
    const r = (await tauriAPI.library.query?.(req)) ?? { items: [], total: 0 };
    return r as { items: T[]; total: number };
  }
  const r = await tauriAPI.serviceLibrary?.query?.({
    platform: source,
    kind: req.kind,
    offset: req.offset,
    limit: req.limit,
  });
  return (r as { items: T[]; total: number | null }) ?? { items: [], total: 0 };
}

export async function queryRecommendations(platform: ServicePlatform) {
  if (platform === 'local') return { shelves: [] };
  return (await tauriAPI.serviceLibrary?.recommendations?.(platform)) ?? { shelves: [] };
}

export async function queryExplore(platform: ServicePlatform) {
  if (platform === 'local') return { shelves: [] };
  return (await tauriAPI.serviceLibrary?.explore?.(platform)) ?? { shelves: [] };
}

export async function queryExplorePage(platform: ServicePlatform, path: string) {
  if (platform === 'local') return { shelves: [] };
  return (await tauriAPI.serviceLibrary?.explorePage?.(platform, path)) ?? { shelves: [] };
}

export interface AlbumPageShelf {
  id: string;
  title: string;
  subtitle: string | null;
  items: Array<Record<string, unknown> & { kind: string }>;
}

export async function queryAlbumPage(
  platform: ServicePlatform,
  albumKey: string
): Promise<{ shelves: AlbumPageShelf[] }> {
  if (platform === 'local') return { shelves: [] };
  const r = await tauriAPI.serviceLibrary?.albumPage?.(platform, albumKey);
  const shelves = (r as { shelves?: AlbumPageShelf[] } | undefined)?.shelves ?? [];
  return { shelves };
}

export async function queryArtistPage(
  platform: ServicePlatform,
  artistKey: string
): Promise<{ shelves: AlbumPageShelf[] }> {
  if (platform === 'local') return { shelves: [] };
  const r = await tauriAPI.serviceLibrary?.artistPage?.(platform, artistKey);
  const shelves = (r as { shelves?: AlbumPageShelf[] } | undefined)?.shelves ?? [];
  return { shelves };
}

export async function queryActivityFeed(platform: ServicePlatform): Promise<ActivityFeedItem[]> {
  if (platform === 'local') return [];
  const r = await tauriAPI.serviceLibrary?.activityFeed?.(platform);
  return (r as { activities?: ActivityFeedItem[] } | undefined)?.activities ?? [];
}

export async function queryEpisodeBookmarks(platform: ServicePlatform): Promise<TrackDto[]> {
  if (platform === 'local') return [];
  const r = await tauriAPI.serviceLibrary?.episodeBookmarks?.(platform);
  return (r as unknown as TrackDto[]) ?? [];
}

export async function queryFollowers(platform: ServicePlatform): Promise<ServiceUserDto[]> {
  if (platform === 'local') return [];
  const r = await tauriAPI.serviceLibrary?.followers?.(platform);
  return (r as unknown as ServiceUserDto[]) ?? [];
}

export async function queryFollowing(platform: ServicePlatform): Promise<ServiceUserDto[]> {
  if (platform === 'local') return [];
  const r = await tauriAPI.serviceLibrary?.following?.(platform);
  return (r as unknown as ServiceUserDto[]) ?? [];
}

/// A string worth showing, or `null`. Backend rows arrive with `""` as readily
/// as with a missing key, and `??` treats `""` as a real value — so every
/// fallback chain reads its input through this instead.
export function text(v: unknown): string | null {
  return typeof v === 'string' && v.trim() ? v : null;
}

export function ipcPlatformOf(source: ServicePlatform): string {
  return toIpcPlatform(source);
}

export function trackIdFromUrl(url: string): string {
  if (!url) return '';
  if (url.includes('?v=')) {
    return url.split('?v=').pop()?.split('&')[0] ?? '';
  }
  return url.split('/').pop()?.split('?')[0] ?? '';
}

const coverCache = new Map<string, string | null>();
const coverPending = new Map<string, Promise<string | null>>();

export async function resolveCoverUrl(cover_id?: string | null): Promise<string | null> {
  if (!cover_id) return null;
  if (coverCache.has(cover_id)) return coverCache.get(cover_id) ?? null;
  const pending = coverPending.get(cover_id);
  if (pending) return pending;
  const p = (tauriAPI.library.coverUrl?.(cover_id) ?? Promise.resolve(null))
    .then((url) => {
      coverCache.set(cover_id, url ?? null);
      coverPending.delete(cover_id);
      return url ?? null;
    })
    .catch(() => {
      coverCache.set(cover_id, null);
      coverPending.delete(cover_id);
      return null;
    });
  coverPending.set(cover_id, p);
  return p;
}

/**
 * A library row as the player wants it. Every caller shared this exact field set
 * and differed only in what it overrode, so overrides win last.
 */
export interface PlayableSource {
  path: string;
  title?: string | null;
  artist?: string | null;
  album?: string | null;
  album_key?: string | null;
  artist_id?: string | null;
  cover_id?: string | null;
  is_video?: boolean | null;
}

export function toPlayable(
  t: PlayableSource,
  platform: string,
  covers?: Record<string, string | null>,
  overrides?: Partial<PlayableTrack>
): PlayableTrack {
  return {
    url: t.path,
    title: text(t.title) ?? '',
    artist: text(t.artist) ?? '',
    album: text(t.album) ?? null,
    thumbnail: t.cover_id ? (covers?.[t.cover_id] ?? undefined) : undefined,
    coverId: t.cover_id ?? undefined,
    albumId: t.album_key ?? null,
    artistId: t.artist_id ?? null,
    mediaType: (t.is_video ? 'video' : 'audio') as MediaType,
    platform,
    ...overrides,
  };
}

export async function startLocalRadio(seed: TrackDto): Promise<PlayableTrack[]> {
  const rows = ((await tauriAPI.library.radio?.(seed.path)) ?? []) as unknown as TrackDto[];
  return [seed, ...rows].map((t) => toPlayable(t, 'local'));
}

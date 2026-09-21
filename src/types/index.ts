export type Platform =
  'spotify' | 'tidal' | 'deezer' | 'qobuz' | 'youtube' | 'youtubemusic' | 'applemusic';

export type OrpheusPlatform =
  'soundcloud' | 'napster' | 'beatport' | 'nugs' | 'kkbox' | 'bugs' | 'idagio' | 'jiosaavn';

export type MediaKind = 'track' | 'album' | 'artist' | 'playlist';

export type SearchType =
  | 'track'
  | 'album'
  | 'playlist'
  | 'artist'
  | 'video'
  | 'channel'
  | 'podcast'
  | 'show'
  | 'episode'
  | 'musicvideo'
  | 'audiobook';

export interface Track {
  id: string;
  title: string;
  artist: string;
  album?: string;
  duration?: number;
  platform: Platform;
  url: string;
  thumbnail?: string;
  releaseDate?: string;
  explicit?: boolean;
  hires?: boolean;
  bitDepth?: number;
  sampleRate?: number;
  mediaTag?: string;
  genre?: string;
  views?: number;
  popularity?: number;
  isrc?: string;
  discNumber?: number;
  trackNumber?: number;
  copyright?: string;
  label?: string;
  previewUrl?: string;
  lyricsAvailable?: boolean;
  genres?: string[];
  artists?: string[];
  albumId?: string;
  artistId?: string;
}

export interface Album {
  id: string;
  title: string;
  artist: string;
  platform: Platform;
  url: string;
  thumbnail?: string;
  trackCount?: number;
  releaseDate?: string;
  tracks?: Track[];
  explicit?: boolean;
  hires?: boolean;
  bitDepth?: number;
  sampleRate?: number;
  mediaTag?: string;
  genre?: string;
  description?: string;
  label?: string;
  copyright?: string;
  popularity?: number;
  totalDuration?: number;
  genres?: string[];
  upc?: string;
  discCount?: number;
  isCompilation?: boolean;
  artistId?: string;
  artists?: string[];
}

export interface Playlist {
  id: string;
  title: string;
  owner: string;
  platform: Platform;
  url: string;
  thumbnail?: string;
  trackCount?: number;
  tracks?: Track[];
  explicit?: boolean;
  description?: string;
  ownerId?: string;
  ownerThumbnail?: string;
  followerCount?: number;
  totalDuration?: number;
  createdAt?: string;
  updatedAt?: string;
  isPublic?: boolean;
  isCollaborative?: boolean;
  isEditable?: boolean;
  genres?: string[];
  moodTags?: string[];
}

export interface Artist {
  id: string;
  name: string;
  platform: Platform;
  url: string;
  thumbnail?: string;
  followerCount?: number;
  genre?: string;
  albums?: Album[];
  biography?: string;
  monthlyListeners?: number;
  popularity?: number;
  genres?: string[];
  topTracks?: Track[];
  similarArtists?: Artist[];
  socialLinks?: Record<string, string>;
  verified?: boolean;
}

export type SearchResult = Track | Album | Playlist | Artist;

export interface QualityOption {
  value: string;
  label: string;
  group?: string;
}

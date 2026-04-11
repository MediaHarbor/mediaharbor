export type Platform =
  | 'spotify'
  | 'tidal'
  | 'deezer'
  | 'qobuz'
  | 'youtube'
  | 'youtubemusic'
  | 'applemusic';

export type OrpheusPlatform =
  | 'soundcloud'
  | 'napster'
  | 'beatport'
  | 'nugs'
  | 'kkbox'
  | 'bugs'
  | 'idagio'
  | 'jiosaavn';

export type SearchType = 'track' | 'album' | 'playlist' | 'artist' | 'video' | 'channel' | 'podcast' | 'show' | 'episode' | 'musicvideo' | 'audiobook';

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
}

export type SearchResult = Track | Album | Playlist | Artist;

export interface QualityOption {
  value: string;
  label: string;
}


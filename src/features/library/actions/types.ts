import type { ReactNode } from 'react';

export type ActionContext = 'search' | 'library' | 'queue' | 'playlist' | 'album' | 'now-playing';

export interface ActionItem {
  id: string;
  label: string;
  icon?: ReactNode;
  action?: () => void;
  disabled?: boolean;
  destructive?: boolean;
  hidden?: boolean;
  checked?: boolean;
  submenu?: ActionItem[];
  separator?: boolean;
}

export interface TrackRef {
  id: string;
  title?: string | null;
  artist?: string | null;
  album?: string | null;
  url?: string | null;
  thumbnail?: string | null;
  uid?: string | null;
  albumId?: string | null;
  artistId?: string | null;
}

export interface AlbumRef {
  id: string;
  title?: string | null;
  artist?: string | null;
  cover_url?: string | null;
  artistId?: string | null;
}

export interface ArtistRef {
  id: string;
  display?: string | null;
  cover_url?: string | null;
}

export interface PlaylistRef {
  id: string;
  name?: string | null;
  cover_url?: string | null;
  owned?: boolean;
}

export interface PodcastRef {
  id: string;
  name?: string | null;
  cover_url?: string | null;
}

export interface EpisodeRef {
  id: string;
  title?: string | null;
  show?: string | null;
  url?: string | null;
  cover_url?: string | null;
}

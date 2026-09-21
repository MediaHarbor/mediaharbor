/// <reference types="vite/client" />

declare module '*.svg' {
  const content: string;
  export default content;
}

interface DownloadRequest {
  url: string;
  outputDir?: string;
  quality?: string | number | null;
  title?: string | null;
  artist?: string | null;
  uploader?: string | null;
  album?: string | null;
  thumbnail?: string | null;
  platform?: string;
  forceRedownload?: boolean;
}

interface DownloadInfoEvent {
  order: number;
  title?: string;
  artist?: string;
  uploader?: string;
  album?: string;
  thumbnail?: string | null;
  platform?: string;
  quality?: string;
}

interface DownloadProgressEvent {
  order: number;
  progress: number;
  title?: string;
  thumbnail?: string | null;
  artist?: string;
  album?: string;
  speed?: string | null;
  eta?: string | null;
  itemIndex?: number | null;
  itemTotal?: number | null;
  currentTrack?: string | null;
  quality?: string | null;
}

interface DownloadCompleteEvent {
  order: number;
  title?: string;
  warnings?: string;
  location?: string;
  fullLog?: string;
}

interface DownloadErrorEvent {
  order: number;
  error: string;
  fullLog: string;
  title?: string;
}

interface ScanProgressEvent {
  progress?: number;
  currentFile?: string;
}

interface SpotifyProfile {
  name?: string;
  id?: string;
  [key: string]: unknown;
}

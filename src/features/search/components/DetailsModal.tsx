import { useState, useEffect, useCallback } from 'react';
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Download, Play, Loader2, Music } from 'lucide-react';
import { formatDuration } from '@/utils/formatters';
import type { Platform } from '@/types';
import { searchService } from '@/services/ipc/search';
import { useNotificationStore } from '@/stores/useNotificationStore';

interface RawTrack {
  id?: string;
  title?: string;
  name?: string;
  trackName?: string;
  artist?: string | { name?: string };
  artists?: Array<{ name?: string }>;
  performer?: { name?: string };
  artistName?: string;
  channel?: string;
  uploader?: string;
  album?: {
    images?: Array<{ url?: string }>;
    cover?: string;
    cover_xl?: string;
    cover_big?: string;
    image?: { large?: string };
  };
  playUrl?: string;
  url?: string;
  uri?: string;
  link?: string;
  webpage_url?: string;
  trackViewUrl?: string;
  external_urls?: { spotify?: string };
  thumbnail?: string;
  thumbnail_url?: string;
  artworkUrl100?: string;
  duration?: number;
  duration_secs?: number;
  duration_ms?: number;
  number?: number;
  quality?: string;
}

interface DetailsResponse {
  tracks?: RawTrack[];
  album?: {
    coverUrl?: string;
    title?: string;
    artist?: string | { name?: string };
    releaseDate?: string;
    genre?: string | { name?: string };
  };
  playlist?: {
    coverUrl?: string;
    title?: string;
    creator?: string;
    artist?: string;
    creationDate?: string;
  };
  thumbnail?: string;
  images?: Array<{ url?: string }>;
  cover_xl?: string;
  artworkUrl100?: string;
  artist?: string | { name?: string };
  url?: string;
  external_urls?: { spotify?: string };
  uri?: string;
  link?: string;
}

export interface TrackInfo {
  url: string;
  title: string;
  artist: string;
  thumbnail?: string;
}

interface DetailsModalProps {
  open: boolean;
  onClose: () => void;
  type: 'album' | 'playlist' | 'podcast';
  id: string;
  platform: Platform;
  title: string;
  onDownload: (info: TrackInfo | string) => void;
  onPlay: (track: TrackInfo) => void;
  onPlayAll?: (tracks: TrackInfo[]) => void;
}

function getTrackInfo(track: RawTrack, platform: Platform, fallbackThumbnail?: string): TrackInfo | null {
  let url: string | undefined;
  let title = track.title || track.name || track.trackName || 'Unknown Track';
  let artist = (typeof track.artist === 'string' ? track.artist : track.artist?.name) || '';
  let thumbnail: string | undefined = fallbackThumbnail;

  switch (platform) {
    case 'spotify':
      url = track.playUrl || track.external_urls?.spotify || track.uri;
      artist = artist || track.artists?.[0]?.name || 'Unknown Artist';
      thumbnail = track.album?.images?.[0]?.url || thumbnail;
      break;

    case 'tidal':
      url = track.playUrl || track.url || (track.id ? `https://tidal.com/browse/track/${track.id}` : undefined);
      artist = artist || track.artists?.[0]?.name || (typeof track.artist !== 'string' ? track.artist?.name : undefined) || 'Unknown Artist';
      if (!artist || artist === 'Unknown Artist') artist = track.artists?.[0]?.name || (typeof track.artist !== 'string' ? track.artist?.name : undefined) || 'Unknown Artist';
      if (track.album?.cover) {
        thumbnail = `https://resources.tidal.com/images/${track.album.cover.replace(/-/g, '/')}/640x640.jpg`;
      }
      break;

    case 'qobuz':
      url = track.playUrl || track.url || (track.id ? `https://play.qobuz.com/track/${track.id}` : undefined);
      artist = artist || track.performer?.name || (typeof track.artist !== 'string' ? track.artist?.name : undefined) || 'Unknown Artist';
      thumbnail = track.album?.image?.large || thumbnail;
      break;

    case 'deezer':
      url = track.playUrl || track.link || (track.id ? `https://www.deezer.com/track/${track.id}` : undefined);
      artist = artist || (typeof track.artist !== 'string' ? track.artist?.name : undefined) || 'Unknown Artist';
      thumbnail = track.album?.cover_xl || track.album?.cover_big || thumbnail;
      break;

    case 'youtube':
    case 'youtubemusic':
      url = track.playUrl || track.url || track.webpage_url || (track.id ? `https://youtube.com/watch?v=${track.id}` : undefined);
      artist = artist || track.channel || track.uploader || 'Unknown';
      thumbnail = track.thumbnail_url || track.thumbnail || thumbnail;
      break;

    case 'applemusic':
      url = track.playUrl || track.trackViewUrl;
      title = track.trackName || title;
      artist = artist || track.artistName || 'Unknown Artist';
      thumbnail = track.artworkUrl100?.replace('100x100', '640x640') || thumbnail;
      break;

    default:
      url = track.playUrl || track.url;
      artist = artist || 'Unknown Artist';
  }

  if (!url) return null;
  return { url, title, artist, thumbnail };
}

export function DetailsModal({
  open,
  onClose,
  type,
  id,
  platform,
  title,
  onDownload,
  onPlay,
  onPlayAll,
}: DetailsModalProps) {
  const [details, setDetails] = useState<DetailsResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const addNotification = useNotificationStore((state) => state.addNotification);

  const loadDetails = useCallback(async () => {
    setLoading(true);
    setDetails(null);
    try {
      if (type === 'album') {
        const data = await searchService.getAlbumDetails({ platform, albumId: id });
        setDetails(data);
      } else {
        const data = await searchService.getPlaylistDetails({ platform, playlistId: id });
        setDetails(data);
      }
    } catch (error) {
      addNotification({
        type: 'error',
        title: 'Error',
        message: error instanceof Error ? error.message : 'Failed to load details',
      });
    } finally {
      setLoading(false);
    }
  }, [type, id, platform, addNotification]);

  useEffect(() => {
    if (open && id) loadDetails();
  }, [open, id, loadDetails]);

  const tracks: RawTrack[] = details?.tracks || [];

  const coverUrl: string | undefined =
    details?.album?.coverUrl ||
    details?.playlist?.coverUrl ||
    details?.thumbnail ||
    details?.images?.[0]?.url ||
    details?.cover_xl ||
    details?.artworkUrl100?.replace('100x100', '640x640');

  const headerTitle: string =
    details?.album?.title || details?.playlist?.title || title;

  const headerArtist: string =
    (typeof details?.album?.artist === 'string' ? details.album.artist : details?.album?.artist?.name) ||
    (typeof details?.artist === 'string' ? details.artist : details?.artist?.name) ||
    details?.playlist?.creator ||
    details?.playlist?.artist ||
    '';

  const releaseDate: string =
    details?.album?.releaseDate ||
    details?.playlist?.creationDate ||
    '';

  const qualityBadge: string =
    (typeof details?.album?.genre === 'string'
      ? details.album.genre
      : details?.album?.genre?.name) ||
    tracks[0]?.quality ||
    '';

  const collectionUrl: string =
    details?.url ||
    details?.external_urls?.spotify ||
    details?.uri ||
    details?.link ||
    '';

  const handlePlayTrack = (track: RawTrack) => {
    const info = getTrackInfo(track, platform, coverUrl);
    if (!info) {
      addNotification({ type: 'error', title: 'Playback Failed', message: 'No URL found for this track' });
      return;
    }
    onPlay(info);
  };

  const handleDownloadTrack = (track: RawTrack) => {
    const info = getTrackInfo(track, platform, coverUrl);
    if (!info) {
      addNotification({ type: 'error', title: 'Download Failed', message: 'No URL found for this track' });
      return;
    }
    onDownload(info);
  };

  const handlePlayAll = () => {
    const allTracks = tracks
      .map((track) => getTrackInfo(track, platform, coverUrl))
      .filter((t): t is TrackInfo => t !== null);

    if (!allTracks.length) {
      addNotification({ type: 'error', title: 'Playback Failed', message: 'No playable tracks found' });
      return;
    }

    if (onPlayAll) {
      onPlayAll(allTracks);
    } else {
      onPlay(allTracks[0]);
    }
  };

  const handleDownloadAll = () => {
    if (!collectionUrl) {
      addNotification({ type: 'error', title: 'Download Failed', message: 'No URL found for this collection' });
      return;
    }
    onDownload(collectionUrl);
  };

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-3xl p-0 overflow-hidden flex flex-col max-h-[85vh] gap-0">
        <DialogTitle className="sr-only">{headerTitle || title}</DialogTitle>
        <DialogDescription className="sr-only">
          {type === 'album' ? 'Album' : type === 'podcast' ? 'Podcast' : 'Playlist'} details
        </DialogDescription>
        <div className="flex items-start gap-4 p-6 pb-4 border-b shrink-0">
          <div className="w-24 h-24 rounded-md overflow-hidden shrink-0 bg-muted flex items-center justify-center">
            {coverUrl ? (
              <img src={coverUrl} alt={headerTitle} className="w-full h-full object-cover" />
            ) : (
              <Music className="w-10 h-10 text-muted-foreground" />
            )}
          </div>

          <div className="flex-1 min-w-0">
            <p className="text-xs text-muted-foreground uppercase tracking-wider mb-1">
              {type === 'album' ? 'Album' : type === 'podcast' ? 'Podcast' : 'Playlist'}
            </p>
            <h2 className="text-xl font-bold truncate leading-tight">{headerTitle}</h2>
            {headerArtist && (
              <p className="text-sm text-muted-foreground mt-0.5 truncate">{headerArtist}</p>
            )}
            <div className="flex items-center gap-2 mt-2 flex-wrap">
              {releaseDate && (
                <span className="text-xs text-muted-foreground">{releaseDate}</span>
              )}
              {releaseDate && tracks.length > 0 && (
                <span className="text-xs text-muted-foreground">·</span>
              )}
              {tracks.length > 0 && (
                <span className="text-xs text-muted-foreground">
                  {tracks.length} {type === 'podcast' ? 'episodes' : 'tracks'}
                </span>
              )}
              {qualityBadge && (
                <>
                  <span className="text-xs text-muted-foreground">·</span>
                  <span className="text-xs bg-secondary text-secondary-foreground rounded px-1.5 py-0.5">{qualityBadge}</span>
                </>
              )}
            </div>

            {details && (
              <div className="flex gap-2 mt-3">
                <Button size="sm" onClick={handlePlayAll} disabled={tracks.length === 0}>
                  <Play className="h-3.5 w-3.5 mr-1.5" />
                  Play All
                </Button>
                <Button size="sm" variant="outline" onClick={handleDownloadAll}>
                  <Download className="h-3.5 w-3.5 mr-1.5" />
                  Download All
                </Button>
              </div>
            )}
          </div>

        </div>

        <div className="overflow-y-auto flex-1 px-2 py-2">
          {loading ? (
            <div className="flex items-center justify-center py-12">
              <Loader2 className="h-7 w-7 animate-spin text-muted-foreground" />
            </div>
          ) : tracks.length === 0 ? (
            <p className="text-muted-foreground text-center py-8">No tracks found</p>
          ) : (
            tracks.map((track: RawTrack, index: number) => {
              const duration = track.duration || track.duration_secs || (track.duration_ms && Math.floor(track.duration_ms / 1000));
              const trackArtist =
                (typeof track.artist !== 'string' ? track.artist?.name : track.artist) ||
                track.artists?.[0]?.name ||
                track.performer?.name ||
                track.artistName ||
                headerArtist ||
                'Unknown Artist';
              return (
                <div
                  key={`${track.id || 'track'}-${index}`}
                  className="group flex items-center gap-3 px-3 py-2 rounded-lg hover:bg-muted/50 transition-colors"
                >
                  <span className="text-muted-foreground text-xs w-6 text-right shrink-0 select-none">
                    {track.number || index + 1}
                  </span>
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium truncate">{track.title || track.name || track.trackName}</p>
                    <p className="text-xs text-muted-foreground truncate">{trackArtist}</p>
                  </div>
                  {track.quality && (
                    <span className="text-[10px] border rounded px-1 py-0 shrink-0 hidden group-hover:inline-flex text-muted-foreground">
                      {track.quality}
                    </span>
                  )}
                  {duration ? (
                    <span className="text-xs text-muted-foreground tabular-nums shrink-0 select-none">
                      {formatDuration(duration)}
                    </span>
                  ) : null}
                  <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity shrink-0">
                    <Button
                      size="icon"
                      variant="ghost"
                      className="h-7 w-7"
                      onClick={() => handlePlayTrack(track)}
                      title="Play"
                    >
                      <Play className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      size="icon"
                      variant="ghost"
                      className="h-7 w-7"
                      onClick={() => handleDownloadTrack(track)}
                      title="Download"
                    >
                      <Download className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </div>
              );
            })
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

export default DetailsModal;

import { useState, useEffect } from 'react';
import { motion } from 'framer-motion';
import { ArrowLeft, Disc3, Clock, Play, Shuffle, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { searchService } from '@/services/ipc/search';
import type { Platform } from '@/types';

function formatDuration(seconds?: number): string {
  if (!seconds) return '';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
  return `${m}:${s.toString().padStart(2, '0')}`;
}

function formatTotalDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h > 0) return `${h} hr ${m} min`;
  return `${m} min`;
}

export interface RemoteTrack {
  title: string;
  artist?: string;
  duration?: number;
  url?: string;
}

function normalizeTrack(track: any, platform: Platform): RemoteTrack {
  let title = track.title || track.name || track.trackName || 'Unknown Track';
  let artist = track.artist || '';
  let duration: number | undefined;
  let url: string | undefined;

  switch (platform) {
    case 'spotify':
      url = track.playUrl || track.external_urls?.spotify || track.uri;
      artist = artist || track.artists?.[0]?.name || '';
      duration = track.duration_ms ? Math.floor(track.duration_ms / 1000) : undefined;
      break;
    case 'tidal':
      url = track.playUrl || track.url || (track.id ? `https://tidal.com/browse/track/${track.id}` : undefined);
      artist = artist || track.artists?.[0]?.name || track.artist?.name || '';
      duration = track.duration;
      break;
    case 'qobuz':
      url = track.playUrl || track.url || (track.id ? `https://play.qobuz.com/track/${track.id}` : undefined);
      artist = artist || track.performer?.name || track.artist?.name || '';
      duration = track.duration;
      break;
    case 'deezer':
      url = track.playUrl || track.link || (track.id ? `https://www.deezer.com/track/${track.id}` : undefined);
      artist = artist || track.artist?.name || '';
      duration = track.duration;
      break;
    case 'applemusic':
      url = track.playUrl || track.trackViewUrl;
      artist = artist || track.artistName || '';
      duration = track.duration ?? (track.trackTimeMillis ? Math.floor(track.trackTimeMillis / 1000) : undefined);
      title = track.trackName || title;
      break;
    case 'youtube':
    case 'youtubemusic':
      url = track.url || track.webpage_url || (track.id ? `https://youtube.com/watch?v=${track.id}` : undefined);
      artist = artist || track.channel || track.uploader || '';
      duration = track.duration;
      break;
  }

  return { title, artist, duration, url };
}

interface SearchAlbumViewProps {
  albumId: string;
  albumTitle: string;
  albumThumbnail?: string;
  albumArtist?: string;
  albumReleaseDate?: string;
  platform: Platform;
  backLabel?: string;
  onBack: () => void;
  onPlay?: (tracks: RemoteTrack[], index?: number) => void;
}

export function SearchAlbumView({
  albumId,
  albumTitle,
  albumThumbnail,
  albumArtist,
  albumReleaseDate,
  platform,
  backLabel = 'Artist',
  onBack,
  onPlay,
}: SearchAlbumViewProps) {
  const [tracks, setTracks] = useState<RemoteTrack[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [coverUrl, setCoverUrl] = useState<string | undefined>(albumThumbnail);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setTracks([]);
    searchService
      .getAlbumDetails({ platform, albumId })
      .then((data) => {
        if (cancelled) return;
        if (data?.thumbnail) setCoverUrl(data.thumbnail);
        const rawTracks: any[] = data?.tracks || data?.items || [];
        setTracks(rawTracks.map((t) => normalizeTrack(t, platform)));
      })
      .catch((e) => {
        if (!cancelled) setError(e?.message || 'Failed to load tracks');
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => { cancelled = true; };
  }, [albumId, platform]);

  const totalDuration = tracks.reduce((acc, t) => acc + (t.duration || 0), 0);

  return (
    <motion.div
      initial={{ opacity: 0, x: 40 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: -40 }}
      transition={{ duration: 0.25 }}
      className="space-y-6"
    >
      <button
        onClick={onBack}
        className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
      >
        <ArrowLeft className="h-4 w-4" />
        {backLabel}
      </button>

      <div className="flex gap-6 items-end">
        <div className="shrink-0">
          {coverUrl ? (
            <img
              src={coverUrl}
              alt={albumTitle}
              className="h-48 w-48 rounded-lg object-cover shadow-xl"
            />
          ) : (
            <div className="h-48 w-48 rounded-lg bg-gradient-to-br from-muted to-muted/60 flex items-center justify-center shadow-xl">
              <Disc3 className="h-16 w-16 text-muted-foreground/20" />
            </div>
          )}
        </div>

        <div className="flex-1 min-w-0 pb-1">
          <p className="text-xs font-medium uppercase tracking-widest text-muted-foreground/60 mb-1">Album</p>
          <h1 className="text-3xl font-bold truncate leading-tight">{albumTitle}</h1>
          {albumArtist && <p className="text-base text-muted-foreground mt-1.5">{albumArtist}</p>}
          <div className="flex items-center gap-2 mt-2 text-sm text-muted-foreground/60">
            {albumReleaseDate && <span>{albumReleaseDate.slice(0, 4)}</span>}
            {albumReleaseDate && tracks.length > 0 && <span>·</span>}
            {tracks.length > 0 && (
              <span>{tracks.length} song{tracks.length !== 1 ? 's' : ''}</span>
            )}
            {totalDuration > 0 && (
              <>
                <span>·</span>
                <span>{formatTotalDuration(totalDuration)}</span>
              </>
            )}
          </div>

          <div className="flex items-center gap-3 mt-5">
            <Button
              size="sm"
              className="rounded-full gap-2 px-6 shadow-md"
              disabled={!onPlay || tracks.length === 0}
              onClick={() => onPlay?.(tracks, 0)}
            >
              <Play className="h-4 w-4 ml-0.5" />
              Play
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="rounded-full gap-2 px-5"
              disabled={!onPlay || tracks.length === 0}
              onClick={() => {
                if (!onPlay || !tracks.length) return;
                onPlay(tracks, Math.floor(Math.random() * tracks.length));
              }}
            >
              <Shuffle className="h-3.5 w-3.5" />
              Shuffle
            </Button>
          </div>
        </div>
      </div>

      {loading ? (
        <div className="flex justify-center py-16">
          <Loader2 className="h-6 w-6 animate-spin text-muted-foreground/40" />
        </div>
      ) : error ? (
        <div className="text-center py-16 text-muted-foreground/50">
          <p className="text-sm">{error}</p>
        </div>
      ) : (
        <div className="rounded-lg overflow-hidden">
          <div className="flex items-center gap-3 px-4 py-2 text-[11px] uppercase tracking-wider text-muted-foreground/40 border-b border-border/20">
            <span className="w-8 text-right">#</span>
            <span className="flex-1">Title</span>
            <Clock className="h-3 w-3 mr-1" />
          </div>
          {tracks.map((track, i) => (
            <div
              key={i}
              className="group flex items-center gap-3 px-4 py-2.5 hover:bg-card/60 transition-colors cursor-pointer rounded-md"
              onClick={() => onPlay?.(tracks, i)}
            >
              <div className="w-8 text-right shrink-0">
                <span className="text-[13px] text-muted-foreground/40 tabular-nums group-hover:hidden">
                  {i + 1}
                </span>
                <Play className="h-3.5 w-3.5 text-foreground hidden group-hover:inline-block ml-auto" />
              </div>
              <div className="flex-1 min-w-0">
                <p className="text-[13px] truncate font-medium">{track.title}</p>
                {track.artist && (
                  <p className="text-[11px] text-muted-foreground/50 truncate">{track.artist}</p>
                )}
              </div>
              {track.duration != null && (
                <span className="text-[12px] text-muted-foreground/40 shrink-0 tabular-nums w-12 text-right">
                  {formatDuration(track.duration)}
                </span>
              )}
            </div>
          ))}
        </div>
      )}
    </motion.div>
  );
}

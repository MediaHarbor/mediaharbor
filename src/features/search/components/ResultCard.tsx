import { useState } from 'react';
import { Play, Download, Music, Disc, ListMusic, User, ChevronRight, Copy, Check, ListEnd } from 'lucide-react';
import { cn } from '@/utils/cn';
import { formatDuration } from '@/utils/formatters';
import type { SearchResult, Track, Album, Playlist, Artist } from '@/types';

interface SearchResultView {
  resultType?: string;
  id?: string;
  title?: string;
  name?: string;
  artist?: string;
  album?: string;
  owner?: string;
  channel?: string;
  platform?: string;
  url?: string;
  thumbnail?: string;
  duration?: number;
  trackCount?: number;
  followerCount?: number;
  genre?: string;
  releaseDate?: string;
  explicit?: boolean;
  hires?: boolean;
  bitDepth?: number;
  sampleRate?: number;
  mediaTag?: string;
  views?: number;
  popularity?: number;
  rank?: number;
  videoId?: string;
  channelId?: string;
  channelTitle?: string;
  viewCount?: number;
  subscriberCount?: number;
}

interface ResultCardProps {
  result: SearchResult;
  onPlay?: () => void;
  onPlayNext?: () => void;
  onDownload?: () => void;
  onClick?: () => void;
}

const TIDAL_TAG_LABELS: Record<string, string> = {
  HIRES_LOSSLESS: 'Master',
  LOSSLESS:       'Lossless',
  MQA:            'MQA',
  DOLBY_ATMOS:    'Atmos',
  SONY_360RA:     '360',
};

function extractYear(dateStr?: string): string | null {
  if (!dateStr) return null;
  const m = dateStr.match(/\d{4}/);
  return m ? m[0] : null;
}

function formatViews(n?: number): string | null {
  if (!n) return null;
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B views`;
  if (n >= 1_000_000)     return `${(n / 1_000_000).toFixed(1)}M views`;
  if (n >= 1_000)         return `${(n / 1_000).toFixed(0)}K views`;
  return `${n} views`;
}

function ExplicitBadge() {
  return (
    <span className="inline-flex items-center justify-center h-4 w-4 rounded-sm bg-muted-foreground/30 text-[9px] font-bold text-muted-foreground leading-none shrink-0">
      E
    </span>
  );
}

function HiResBadge() {
  return (
    <span className="inline-flex items-center rounded-sm border border-yellow-500/60 px-1 py-0 text-[9px] font-semibold text-yellow-500 leading-4 shrink-0">
      Hi-Res
    </span>
  );
}

function TagBadge({ label }: { label: string }) {
  return (
    <span className="inline-flex items-center rounded-sm border border-sky-500/50 px-1 py-0 text-[9px] font-semibold text-sky-400 leading-4 shrink-0">
      {label}
    </span>
  );
}

function QobuzBadge({ bitDepth, sampleRate }: { bitDepth: number; sampleRate: number }) {
  return (
    <span className="inline-flex items-center rounded-sm border border-violet-500/50 px-1 py-0 text-[9px] font-semibold text-violet-400 leading-4 shrink-0 whitespace-nowrap">
      {bitDepth}bit / {sampleRate}kHz
    </span>
  );
}

function YearBadge({ year }: { year: string }) {
  return (
    <span className="inline-flex items-center rounded-sm border border-muted-foreground/25 px-1 py-0 text-[9px] font-medium text-muted-foreground/60 leading-4 shrink-0">
      {year}
    </span>
  );
}

function PopularityBadge({ score }: { score: number }) {
  return (
    <span className="inline-flex items-center rounded-sm border border-green-500/40 px-1 py-0 text-[9px] font-semibold text-green-500/80 leading-4 shrink-0 whitespace-nowrap">
      ★ {score}
    </span>
  );
}

function RankBadge({ rank }: { rank: number }) {
  const label = rank >= 1_000_000
    ? `${(rank / 1_000_000).toFixed(1)}M`
    : rank >= 1_000
    ? `${(rank / 1_000).toFixed(0)}K`
    : String(rank);
  return (
    <span className="inline-flex items-center rounded-sm border border-orange-500/40 px-1 py-0 text-[9px] font-semibold text-orange-400/80 leading-4 shrink-0 whitespace-nowrap">
      # {label}
    </span>
  );
}

export function ResultCard({ result, onPlay, onPlayNext, onDownload, onClick }: ResultCardProps) {
  const [copied, setCopied] = useState(false);

  const r = result as SearchResultView;

  const isTrack      = r.resultType === 'track';
  const isAlbum      = r.resultType === 'album';
  const isPlaylist   = r.resultType === 'playlist';
  const isArtist     = r.resultType === 'artist';
  const isVideo      = r.resultType === 'video';
  const isMusicVideo = r.resultType === 'musicvideo';
  const isChannel    = r.resultType === 'channel';
  const isPodcast    = r.resultType === 'podcast';
  const isShow       = r.resultType === 'show';
  const isEpisode    = r.resultType === 'episode';
  const isAudiobook  = r.resultType === 'audiobook';

  const isPlayable   = isTrack || isVideo || isMusicVideo || isEpisode;
  const isExpandable = isAlbum || isPlaylist || isPodcast || isShow || isAudiobook;

  const Icon = isPlayable ? Music : isAlbum ? Disc : (isPlaylist || isPodcast || isShow || isAudiobook) ? ListMusic : User;

  const title = r.title ?? r.name ?? 'Unknown';
  const url   = r.url ?? '';

  const str = (v: unknown): string | undefined =>
    typeof v === 'string' && v.length > 0 ? v : undefined;

  const getSubtitle = () => {
    if (isTrack) {
      const t = result as Track;
      const parts = [str(t.artist), str(t.album), str(t.genre)].filter(Boolean) as string[];
      return parts.join(' · ');
    }
    if (isVideo || isMusicVideo) {
      return str(r.artist) || str(r.channel) || '';
    }
    if (isEpisode) {
      return str(r.artist) || str(r.owner) || 'Episode';
    }
    if (isShow) {
      return [str(r.owner), r.trackCount ? `${r.trackCount} episodes` : undefined].filter(Boolean).join(' · ');
    }
    if (isAudiobook) {
      return [str(r.owner), r.trackCount ? `${r.trackCount} chapters` : undefined].filter(Boolean).join(' · ');
    }
    if (isChannel) {
      return 'YouTube Channel';
    }
    if (isPodcast) {
      return str(r.artist) || 'Podcast';
    }
    if (isAlbum) {
      const a = result as Album;
      const parts: string[] = [str(a.artist)].filter(Boolean) as string[];
      if (a.trackCount) parts.push(`${a.trackCount} tracks`);
      const year = extractYear(str(a.releaseDate));
      if (year) parts.push(year);
      const genre = str(a.genre);
      if (genre) parts.push(genre);
      return parts.join(' · ');
    }
    if (isPlaylist) {
      const p = result as Playlist;
      return [str(p.owner), p.trackCount ? `${p.trackCount} tracks` : undefined]
        .filter(Boolean).join(' · ');
    }
    if (isArtist) {
      const a = result as Artist;
      const parts: string[] = [];
      if (a.followerCount) parts.push(`${a.followerCount.toLocaleString()} followers`);
      const genre = str(a.genre);
      if (genre) parts.push(genre);
      return parts.join(' · ');
    }
    return '';
  };

  const tidalLabel = r.mediaTag ? (TIDAL_TAG_LABELS[r.mediaTag] ?? r.mediaTag) : null;
  const viewLabel  = formatViews(r.views);

  const handleCopy = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!url) return;
    navigator.clipboard.writeText(url).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  const duration = isPlayable ? (result as Track).duration : undefined;

  return (
    <div
      className={cn(
        'group flex items-center gap-3 px-3 py-2 rounded-lg transition-colors duration-100',
        'hover:bg-muted/60',
        onClick && 'cursor-pointer'
      )}
      onClick={onClick}
    >
      <div className="relative h-10 w-10 shrink-0 overflow-hidden rounded-md bg-muted">
        {result.thumbnail ? (
          <img
            src={result.thumbnail}
            alt={title}
            className="h-full w-full object-cover"
            onError={(e) => { e.currentTarget.style.display = 'none'; }}
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center">
            <Icon className="h-4 w-4 text-muted-foreground/40" />
          </div>
        )}

        {isPlayable && onPlay && (
          <div
            className="absolute inset-0 flex items-center justify-center bg-black/50 opacity-0 group-hover:opacity-100 transition-opacity duration-100"
            onClick={(e) => { e.stopPropagation(); onPlay(); }}
          >
            <Play className="h-4 w-4 text-white fill-white" />
          </div>
        )}
        {isExpandable && onClick && (
          <div className="absolute inset-0 flex items-center justify-center bg-black/50 opacity-0 group-hover:opacity-100 transition-opacity duration-100">
            <ChevronRight className="h-5 w-5 text-white" />
          </div>
        )}
        {isArtist && onClick && (
          <div className="absolute inset-0 flex items-center justify-center bg-black/50 opacity-0 group-hover:opacity-100 transition-opacity duration-100">
            <ChevronRight className="h-5 w-5 text-white" />
          </div>
        )}
      </div>

      <div className="flex-1 min-w-0">
        <p className="text-sm font-medium truncate leading-snug">{title}</p>
        <p className="text-xs text-muted-foreground truncate leading-snug mt-0.5">{getSubtitle()}</p>
      </div>

      <div className="hidden sm:flex items-center gap-1 shrink-0">
        {r.explicit && <ExplicitBadge />}
        {r.hires && <HiResBadge />}
        {r.bitDepth && r.sampleRate && (
          <QobuzBadge bitDepth={r.bitDepth} sampleRate={r.sampleRate} />
        )}
        {tidalLabel && !r.hires && <TagBadge label={tidalLabel} />}
        {viewLabel && (
          <span className="text-[10px] text-muted-foreground/60 whitespace-nowrap">{viewLabel}</span>
        )}
        {typeof r.popularity === 'number' && r.platform === 'spotify' && (
          <PopularityBadge score={r.popularity} />
        )}
        {typeof r.rank === 'number' && r.platform === 'deezer' && (
          <RankBadge rank={r.rank} />
        )}
        {isTrack && r.releaseDate && r.platform !== 'youtube' && r.platform !== 'youtubemusic' && (() => {
          const yr = extractYear(r.releaseDate);
          return yr ? <YearBadge year={yr} /> : null;
        })()}
      </div>

      {duration ? (
        <span className="text-xs text-muted-foreground tabular-nums shrink-0 w-10 text-right">
          {formatDuration(duration)}
        </span>
      ) : (
        <span className="w-10 shrink-0" />
      )}

      <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity duration-100">
        {onPlay && isPlayable && (
          <button
            className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors duration-100"
            onClick={(e) => { e.stopPropagation(); onPlay(); }}
            title="Play"
          >
            <Play className="h-3.5 w-3.5" />
          </button>
        )}
        {onPlayNext && isPlayable && (
          <button
            className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors duration-100"
            onClick={(e) => { e.stopPropagation(); onPlayNext(); }}
            title="Play next"
          >
            <ListEnd className="h-3.5 w-3.5" />
          </button>
        )}
        {onDownload && (
          <button
            className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors duration-100"
            onClick={(e) => { e.stopPropagation(); onDownload(); }}
            title="Download"
          >
            <Download className="h-3.5 w-3.5" />
          </button>
        )}
        {url && (
          <button
            className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors duration-100"
            onClick={handleCopy}
            title="Copy URL"
          >
            {copied
              ? <Check className="h-3.5 w-3.5 text-green-500" />
              : <Copy className="h-3.5 w-3.5" />
            }
          </button>
        )}
      </div>
    </div>
  );
}

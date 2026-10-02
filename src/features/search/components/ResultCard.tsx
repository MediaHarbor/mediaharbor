import { useState, type ReactNode } from 'react';
import {
  Play,
  Download,
  Music,
  Disc,
  ListMusic,
  User,
  ChevronRight,
  Copy,
  Check,
  ListEnd,
} from 'lucide-react';
import { cn } from '@/utils/cn';
import { formatDuration } from '@/utils/formatters';
import { TIDAL_TAG_LABELS } from '@/utils/constants';
import type { SearchResult, Track, Album, Playlist, Artist } from '@/types';
import { SaveToggleHeart } from '@/features/library/components/SaveToggleHeart';
import type { SaveKindStr } from '@/tauri-bridge';
import { copyText } from '@/utils/clipboard';
import { isKnownPlatform, normalizePlatform } from '@/utils/platform-data';
import {
  TrackContextMenu,
  AlbumContextMenu,
  ArtistContextMenu,
  PlaylistContextMenu,
  PodcastContextMenu,
  EpisodeContextMenu,
  TrackKebab,
  AlbumKebab,
  ArtistKebab,
  PlaylistKebab,
} from '@/features/library/actions/RowContextMenu';

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

function extractYear(dateStr?: string): string | null {
  if (!dateStr) return null;
  const m = dateStr.match(/\d{4}/);
  return m ? m[0] : null;
}

function formatViews(n?: number): string | null {
  if (!n) return null;
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B views`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M views`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}K views`;
  return `${n} views`;
}

function ExplicitBadge() {
  return (
    <span className="inline-flex items-center justify-center h-4 w-4 rounded-sm bg-muted-foreground/30 text-[9px] font-bold text-muted-foreground leading-none shrink-0">
      E
    </span>
  );
}

/** Border + text colour per badge kind; everything else is shared. */
const BADGE_TONES = {
  gold: 'border-yellow-500/60 text-yellow-500',
  sky: 'border-sky-500/50 text-sky-400',
  violet: 'border-violet-500/50 text-violet-400',
  muted: 'border-muted-foreground/25 text-muted-foreground/60 font-medium',
  green: 'border-green-500/40 text-green-500/80',
  orange: 'border-orange-500/40 text-orange-400/80',
} as const;

function Badge({ tone, children }: { tone: keyof typeof BADGE_TONES; children: ReactNode }) {
  return (
    <span
      className={cn(
        'inline-flex items-center rounded-sm border px-1 py-0 text-[9px] font-semibold leading-4 shrink-0 whitespace-nowrap',
        BADGE_TONES[tone]
      )}
    >
      {children}
    </span>
  );
}

function HiResBadge({ label = 'Hi-Res' }: { label?: string }) {
  return <Badge tone="gold">{label}</Badge>;
}

function TagBadge({ label }: { label: string }) {
  return <Badge tone="sky">{label}</Badge>;
}

function QobuzBadge({ bitDepth, sampleRate }: { bitDepth: number; sampleRate: number }) {
  return (
    <Badge tone="violet">
      {bitDepth}bit / {sampleRate}kHz
    </Badge>
  );
}

function YearBadge({ year }: { year: string }) {
  return <Badge tone="muted">{year}</Badge>;
}

function PopularityBadge({ score }: { score: number }) {
  return <Badge tone="green">★ {score}</Badge>;
}

function RankBadge({ rank }: { rank: number }) {
  const label =
    rank >= 1_000_000
      ? `${(rank / 1_000_000).toFixed(1)}M`
      : rank >= 1_000
        ? `${(rank / 1_000).toFixed(0)}K`
        : String(rank);
  return <Badge tone="orange"># {label}</Badge>;
}

const btnIcon = 'h-3.5 w-3.5';

function IconBtn(p: { icon: React.ReactNode; title: string; onClick: () => void }) {
  const click = (e: React.MouseEvent) => {
    e.stopPropagation();
    p.onClick();
  };
  return (
    <button
      className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-accent transition-colors duration-100"
      onClick={click}
      title={p.title}
    >
      {p.icon}
    </button>
  );
}

export function ResultCard({ result, onPlay, onPlayNext, onDownload, onClick }: ResultCardProps) {
  const [copied, setCopied] = useState(false);

  const r = result as SearchResultView;

  const isTrack = r.resultType === 'track';
  const isAlbum = r.resultType === 'album';
  const isPlaylist = r.resultType === 'playlist';
  const isArtist = r.resultType === 'artist';
  const isVideo = r.resultType === 'video';
  const isMusicVideo = r.resultType === 'musicvideo';
  const isChannel = r.resultType === 'channel';
  const isPodcast = r.resultType === 'podcast';
  const isShow = r.resultType === 'show';
  const isEpisode = r.resultType === 'episode';
  const isAudiobook = r.resultType === 'audiobook';

  const isPlayable = isTrack || isVideo || isMusicVideo || isEpisode;
  const isExpandable = isAlbum || isPlaylist || isPodcast || isShow || isAudiobook;

  const Icon = isPlayable
    ? Music
    : isAlbum
      ? Disc
      : isPlaylist || isPodcast || isShow || isAudiobook
        ? ListMusic
        : User;

  const title = r.title ?? r.name ?? 'Unknown';
  const url = r.url ?? '';

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
      return [str(r.owner), r.trackCount ? `${r.trackCount} episodes` : undefined]
        .filter(Boolean)
        .join(' · ');
    }
    if (isAudiobook) {
      return [str(r.owner), r.trackCount ? `${r.trackCount} chapters` : undefined]
        .filter(Boolean)
        .join(' · ');
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
        .filter(Boolean)
        .join(' · ');
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
  const viewLabel = formatViews(r.views);

  const copyUrl = () => {
    if (!url) return;
    void copyText(url).then((ok) => {
      if (!ok) return;
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  const duration = isPlayable ? (result as Track).duration : undefined;

  const rowPlatform =
    r.platform && isKnownPlatform(r.platform) ? normalizePlatform(r.platform) : undefined;
  const rowItemIdRaw = r.id ?? (url ? url.split('/').pop() : undefined);
  const rowItemId = rowItemIdRaw != null ? String(rowItemIdRaw) : undefined;

  const kebab =
    rowPlatform && rowItemId ? (
      isTrack ? (
        <TrackKebab
          size="sm"
          platform={rowPlatform}
          context="search"
          track={{
            id: rowItemId,
            title: r.title ?? null,
            artist: r.artist ?? null,
            album: (result as Track).album ?? null,
            url: url || null,
            thumbnail: result.thumbnail ?? null,
            albumId: (result as Track).albumId ?? null,
            artistId: (result as Track).artistId ?? null,
          }}
          onPlay={onPlay}
        />
      ) : isAlbum ? (
        <AlbumKebab
          size="sm"
          platform={rowPlatform}
          context="search"
          album={{
            id: rowItemId,
            title: (result as Album).title ?? null,
            artist: (result as Album).artist ?? null,
            cover_url: result.thumbnail ?? null,
            artistId: (result as Album).artistId ?? null,
          }}
          onPlay={onPlay}
          copyLink={url || undefined}
        />
      ) : isArtist ? (
        <ArtistKebab
          size="sm"
          platform={rowPlatform}
          context="search"
          artist={{
            id: rowItemId,
            display: (result as Artist).name ?? null,
            cover_url: result.thumbnail ?? null,
          }}
          copyLink={url || undefined}
        />
      ) : isPlaylist ? (
        <PlaylistKebab
          size="sm"
          platform={rowPlatform}
          context="search"
          playlist={{
            id: rowItemId,
            name: (result as Playlist).title ?? null,
            cover_url: result.thumbnail ?? null,
            owned: false,
          }}
          onPlay={onPlay}
          copyLink={url || undefined}
        />
      ) : null
    ) : null;

  const inner = (
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
            loading="lazy"
            decoding="async"
            className="h-full w-full object-cover"
            onError={(e) => {
              e.currentTarget.style.display = 'none';
            }}
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center">
            <Icon className="h-4 w-4 text-muted-foreground/40" />
          </div>
        )}

        {isPlayable && onPlay && (
          <div
            className="absolute inset-0 flex items-center justify-center bg-black/50 opacity-0 group-hover:opacity-100 transition-opacity duration-100"
            onClick={(e) => {
              e.stopPropagation();
              onPlay();
            }}
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
        <p className="text-xs text-muted-foreground truncate leading-snug mt-0.5">
          {getSubtitle()}
        </p>
      </div>

      <div className="hidden sm:flex items-center gap-1 shrink-0">
        {r.explicit && <ExplicitBadge />}
        {r.hires && !tidalLabel && <HiResBadge />}
        {r.bitDepth && r.sampleRate && (
          <QobuzBadge bitDepth={r.bitDepth} sampleRate={r.sampleRate} />
        )}
        {tidalLabel &&
          (r.hires ? <HiResBadge label={tidalLabel} /> : <TagBadge label={tidalLabel} />)}
        {viewLabel && (
          <span className="text-[10px] text-muted-foreground/60 whitespace-nowrap">
            {viewLabel}
          </span>
        )}
        {typeof r.popularity === 'number' && r.platform === 'spotify' && (
          <PopularityBadge score={r.popularity} />
        )}
        {typeof r.rank === 'number' && r.platform === 'deezer' && <RankBadge rank={r.rank} />}
        {isTrack &&
          r.releaseDate &&
          r.platform !== 'youtube' &&
          r.platform !== 'youtubemusic' &&
          (() => {
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

      {(() => {
        const kind: SaveKindStr | null = isTrack
          ? 'track'
          : isAlbum
            ? 'album'
            : isArtist
              ? 'artist'
              : isPlaylist
                ? 'playlist'
                : null;
        if (!rowPlatform || !rowItemId || !kind) return null;
        return <SaveToggleHeart platform={rowPlatform} kind={kind} id={rowItemId} />;
      })()}

      <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity duration-100">
        {onPlay && isPlayable && (
          <IconBtn icon={<Play className={btnIcon} />} title="Play" onClick={onPlay} />
        )}
        {onPlayNext && isPlayable && (
          <IconBtn icon={<ListEnd className={btnIcon} />} title="Play next" onClick={onPlayNext} />
        )}
        {onDownload && (
          <IconBtn icon={<Download className={btnIcon} />} title="Download" onClick={onDownload} />
        )}
        {url && (
          <IconBtn
            icon={
              copied ? (
                <Check className={`${btnIcon} text-green-500`} />
              ) : (
                <Copy className={btnIcon} />
              )
            }
            title="Copy URL"
            onClick={copyUrl}
          />
        )}
        {kebab}
      </div>
    </div>
  );

  if (!rowPlatform || !rowItemId) return inner;
  if (isTrack) {
    const t = result as Track;
    return (
      <TrackContextMenu
        platform={rowPlatform}
        track={{
          id: rowItemId,
          title: t.title ?? null,
          artist: t.artist ?? null,
          album: t.album ?? null,
          url: url || null,
          thumbnail: result.thumbnail ?? null,
          albumId: t.albumId ?? null,
          artistId: t.artistId ?? null,
        }}
        context="search"
        onPlay={onPlay}
      >
        {inner}
      </TrackContextMenu>
    );
  }
  if (isAlbum) {
    const a = result as Album;
    return (
      <AlbumContextMenu
        platform={rowPlatform}
        album={{
          id: rowItemId,
          title: a.title ?? null,
          artist: a.artist ?? null,
          cover_url: result.thumbnail ?? null,
          artistId: a.artistId ?? null,
        }}
        context="search"
        onPlay={onPlay}
        copyLink={url}
      >
        {inner}
      </AlbumContextMenu>
    );
  }
  if (isArtist) {
    const ar = result as Artist;
    return (
      <ArtistContextMenu
        platform={rowPlatform}
        artist={{
          id: rowItemId,
          display: ar.name ?? null,
          cover_url: result.thumbnail ?? null,
        }}
        context="search"
        copyLink={url}
      >
        {inner}
      </ArtistContextMenu>
    );
  }
  if (isPlaylist) {
    const p = result as Playlist;
    return (
      <PlaylistContextMenu
        platform={rowPlatform}
        playlist={{
          id: rowItemId,
          name: p.title ?? null,
          cover_url: result.thumbnail ?? null,
          owned: false,
        }}
        context="search"
        onPlay={onPlay}
        copyLink={url}
      >
        {inner}
      </PlaylistContextMenu>
    );
  }
  if (isPodcast || isShow) {
    return (
      <PodcastContextMenu
        platform={rowPlatform}
        podcast={{
          id: rowItemId,
          name: title,
          cover_url: r.thumbnail ?? null,
        }}
        context="search"
        copyLink={url}
      >
        {inner}
      </PodcastContextMenu>
    );
  }
  if (isEpisode) {
    return (
      <EpisodeContextMenu
        platform={rowPlatform}
        episode={{
          id: rowItemId,
          title,
          show: str(r.owner) || str(r.artist) || null,
          url,
          cover_url: r.thumbnail ?? null,
        }}
        context="search"
      >
        {inner}
      </EpisodeContextMenu>
    );
  }
  return inner;
}

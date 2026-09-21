import { Disc3, ListMusic, User, ListChecks, Mic, Radio, Compass, Play } from 'lucide-react';
import { cn } from '@/utils/cn';
import { gradientFor, pickReleaseLabel, pickSubtitle, pickTitle, type ShelfItem } from './layouts';

const KIND_ICONS: Record<string, React.ReactNode> = {
  album: <Disc3 className="h-5 w-5 text-muted-foreground/50" />,
  track: <ListMusic className="h-5 w-5 text-muted-foreground/50" />,
  artist: <User className="h-5 w-5 text-muted-foreground/50" />,
  playlist: <ListChecks className="h-5 w-5 text-muted-foreground/50" />,
  episode: <Mic className="h-5 w-5 text-muted-foreground/50" />,
  mix: <Radio className="h-5 w-5 text-muted-foreground/50" />,
  page_link: <Compass className="h-5 w-5 text-muted-foreground/50" />,
};

function Placeholder({ kind }: { kind: string }) {
  return (
    <div className="flex h-full w-full items-center justify-center">
      {KIND_ICONS[kind] ?? <Disc3 className="h-6 w-6 text-muted-foreground/40" />}
    </div>
  );
}

const FRAME =
  'relative overflow-hidden bg-muted/30 shadow-[0_4px_16px_rgba(0,0,0,0.4)] ' +
  'transition-all duration-200 group-hover:shadow-[0_8px_28px_rgba(0,0,0,0.55)]';

const IMG = 'h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.04]';

const OVERLAY = 'absolute inset-0 hidden place-items-center bg-black/55';

function PlayBadge({ round = false, onPlay }: { round?: boolean; onPlay?: () => void }) {
  const cls = cn(
    'absolute bottom-2 right-2 grid h-10 w-10 place-items-center rounded-full',
    'bg-[color:var(--service-accent)] text-black shadow-xl',
    'translate-y-2 opacity-0 transition-all duration-200',
    'group-hover:translate-y-0 group-hover:opacity-100',
    round && 'bottom-3 right-3'
  );
  const icon = <Play className="h-4 w-4 translate-x-px fill-current" />;
  if (!onPlay) return <div className={cls}>{icon}</div>;
  return (
    <button
      type="button"
      aria-label="Play"
      className={cn(cls, 'hover:scale-105 focus-visible:opacity-100')}
      onClick={(e) => {
        e.stopPropagation();
        onPlay();
      }}
    >
      {icon}
    </button>
  );
}

export interface CardProps {
  item: ShelfItem;
  coverUrl: string | null;
  onOpen: () => void;
  /// Starts playback instead of opening. Absent when the item has nothing to play.
  onPlay?: () => void;
}

export function StandardCard({
  item,
  coverUrl,
  onOpen,
  onPlay,
  compact = false,
  showYear = false,
}: CardProps & { compact?: boolean; showYear?: boolean }) {
  const isArtist = item.kind === 'artist';
  const year = showYear ? pickReleaseLabel(item) : null;
  const subtitle = pickSubtitle(item);
  return (
    <div
      className={cn(
        'group shrink-0 cursor-pointer rounded-lg p-2 transition-colors hover:bg-card/70',
        compact ? 'w-[148px]' : 'w-[184px]'
      )}
      onClick={onOpen}
    >
      <div className={cn(FRAME, 'aspect-square', isArtist ? 'rounded-full' : 'rounded-md')}>
        {coverUrl ? (
          <img src={coverUrl} alt="" className={IMG} loading="lazy" />
        ) : (
          <Placeholder kind={item.kind} />
        )}
        <PlayBadge round={isArtist} onPlay={onPlay} />
      </div>
      <div
        className={cn(
          'mt-3 line-clamp-2 font-semibold leading-snug tracking-tight',
          compact ? 'text-[13px]' : 'text-[15px]'
        )}
      >
        {pickTitle(item)}
      </div>
      {(subtitle || year) && (
        <div className="mt-1 flex items-center gap-1.5 text-[12px] text-muted-foreground/65">
          {year && (
            <span className="shrink-0 rounded-sm border border-border/60 px-1 py-px text-[10px] leading-none">
              {year}
            </span>
          )}
          {subtitle && <span className="line-clamp-1">{subtitle}</span>}
        </div>
      )}
    </div>
  );
}

export function ShortcutTile({ item, coverUrl, onOpen, onPlay }: CardProps) {
  return (
    <div
      className="group flex cursor-pointer items-center gap-3 overflow-hidden rounded-md bg-card/70 pr-3 transition-colors hover:bg-card"
      onClick={onOpen}
    >
      <div className="h-[64px] w-[64px] shrink-0 overflow-hidden bg-muted/40">
        {coverUrl ? (
          <img src={coverUrl} alt="" className="h-full w-full object-cover" loading="lazy" />
        ) : (
          <Placeholder kind={item.kind} />
        )}
      </div>
      <span className="line-clamp-2 min-w-0 flex-1 text-[14px] font-semibold leading-tight">
        {pickTitle(item)}
      </span>
      <button
        type="button"
        aria-label="Play"
        disabled={!onPlay}
        className="grid h-9 w-9 shrink-0 place-items-center rounded-full bg-[color:var(--service-accent)] text-black opacity-0 shadow-lg transition-opacity group-hover:opacity-100"
        onClick={(e) => {
          e.stopPropagation();
          onPlay?.();
        }}
      >
        <Play className="h-4 w-4 translate-x-px fill-current" />
      </button>
    </div>
  );
}

export function HeroCard({ item, coverUrl, onOpen, onPlay }: CardProps) {
  const subtitle = pickSubtitle(item);
  return (
    <div className="group w-[340px] shrink-0 cursor-pointer p-2" onClick={onOpen}>
      <div className={cn(FRAME, 'aspect-[16/9] rounded-lg')}>
        {coverUrl ? (
          <img src={coverUrl} alt="" className={IMG} loading="lazy" />
        ) : (
          <div
            className="h-full w-full"
            style={{ background: gradientFor(pickTitle(item) || item.kind) }}
          />
        )}
        <div className="absolute inset-0 bg-gradient-to-t from-black/85 via-black/25 to-transparent" />
        <div className="absolute inset-x-0 bottom-0 p-3">
          <div className="line-clamp-2 text-[16px] font-bold tracking-tight text-white drop-shadow">
            {pickTitle(item)}
          </div>
          {subtitle && (
            <div className="mt-0.5 line-clamp-1 text-[12px] text-white/70">{subtitle}</div>
          )}
        </div>
        <PlayBadge onPlay={onPlay} />
      </div>
    </div>
  );
}

export function MixTile({ item, coverUrl, onOpen, onPlay }: CardProps) {
  const title = pickTitle(item);
  const subtitle = pickSubtitle(item);
  return (
    <div className="group w-[184px] shrink-0 cursor-pointer p-2" onClick={onOpen}>
      <div className={cn(FRAME, 'aspect-square rounded-2xl')}>
        {coverUrl ? (
          <img src={coverUrl} alt="" className={IMG} loading="lazy" />
        ) : (
          <div
            className="h-full w-full"
            style={{ background: gradientFor(title || (item.id as string) || item.kind) }}
          />
        )}
        <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/10 to-transparent" />
        <div className="absolute inset-x-0 bottom-0 p-3">
          <div className="line-clamp-2 text-[14px] font-bold tracking-tight text-white drop-shadow">
            {title}
          </div>
          {subtitle && (
            <div className="mt-0.5 line-clamp-1 text-[11px] text-white/65">{subtitle}</div>
          )}
        </div>
        <PlayBadge onPlay={onPlay} />
      </div>
    </div>
  );
}

export function RankedRow({
  item,
  coverUrl,
  onOpen,
  onPlay,
  rank,
  showRank = true,
}: CardProps & { rank: number; showRank?: boolean }) {
  const subtitle = pickSubtitle(item);
  return (
    <div
      className="group flex cursor-pointer items-center gap-3 rounded-md px-2 py-1.5 transition-colors hover:bg-card/70"
      onClick={onOpen}
    >
      {showRank && (
        <span className="w-6 shrink-0 text-right font-mono text-[15px] tabular-nums text-muted-foreground/35 transition-colors group-hover:text-[color:var(--service-accent)]">
          {rank}
        </span>
      )}
      <div className={cn(FRAME, 'h-12 w-12 shrink-0 rounded')}>
        {coverUrl ? (
          <img src={coverUrl} alt="" className="h-full w-full object-cover" loading="lazy" />
        ) : (
          <Placeholder kind={item.kind} />
        )}
        {onPlay ? (
          <button
            type="button"
            aria-label="Play"
            className={cn(OVERLAY, 'group-hover:grid')}
            onClick={(e) => {
              e.stopPropagation();
              onPlay();
            }}
          >
            <Play className="h-4 w-4 fill-white text-white" />
          </button>
        ) : (
          <div className={cn(OVERLAY, 'group-hover:grid')}>
            <Play className="h-4 w-4 fill-white text-white" />
          </div>
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[14px] font-medium leading-tight">{pickTitle(item)}</div>
        {subtitle && (
          <div className="mt-0.5 truncate text-[12px] text-muted-foreground/60">{subtitle}</div>
        )}
      </div>
    </div>
  );
}

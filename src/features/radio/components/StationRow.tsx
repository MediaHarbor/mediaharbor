import { memo, type ReactNode } from 'react';
import { Heart, Play, Volume2 } from 'lucide-react';
import { cn } from '@/utils/cn';
import { StationIcon } from '@/features/radio/components/StationIcon';
import { StationMenu } from '@/features/radio/components/StationMenu';
import { useRadioActions } from '@/features/radio/RadioActions';
import { stationSubtitle } from '@/features/radio/format';
import type { StationTileProps } from '@/features/radio/components/StationCard';

interface Props extends StationTileProps {
  /** Rendered at the far left — the drag handle of a playlist row. */
  handle?: ReactNode;
  /** Shown on hover at the far right, beside the remove button. */
  trailing?: ReactNode;
}

/**
 * A station as one line.
 *
 * The same row backs list mode everywhere: browse results, favourites, search
 * and playlists. It was written inside `ListsView` first; sharing it is what
 * keeps a station looking like a station wherever it appears.
 */
export const StationRow = memo(function StationRow({
  station,
  favorite,
  playing,
  cover,
  handle,
  trailing,
  onRemoveFromList,
}: Props) {
  const { onPlay, onToggleFavorite } = useRadioActions();

  return (
    <StationMenu station={station} favorite={favorite} onRemoveFromList={onRemoveFromList}>
      <div
        className={cn(
          'group flex items-center gap-3 rounded-lg border bg-muted/20 p-2 pr-2 transition-colors',
          playing ? 'border-[var(--service-accent)]/60' : 'border-border/40 hover:bg-muted/40'
        )}
      >
        {handle}
        <button
          type="button"
          onClick={() => onPlay(station)}
          className="flex min-w-0 flex-1 items-center gap-3 rounded text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <StationIcon
            src={cover}
            className="h-10 w-10 shrink-0 overflow-hidden rounded bg-muted/50"
            iconClassName="h-4 w-4"
          />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-sm font-medium">{station.name}</span>
            <span className="block truncate text-[10px] uppercase tracking-wide text-muted-foreground/60">
              {stationSubtitle(station, { detailed: true })}
            </span>
          </span>
          {playing ? (
            <Volume2 className="h-4 w-4 shrink-0" style={{ color: 'var(--service-accent)' }} />
          ) : (
            <Play className="h-3.5 w-3.5 shrink-0 fill-current opacity-0 transition-opacity group-hover:opacity-60" />
          )}
        </button>

        <button
          type="button"
          onClick={() => onToggleFavorite(station, !favorite)}
          aria-pressed={favorite}
          title={favorite ? 'Remove from favourites' : 'Save station'}
          className={cn(
            'shrink-0 rounded p-1.5 transition-opacity focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
            favorite ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
          )}
        >
          <Heart
            className={cn(
              'h-3.5 w-3.5',
              favorite ? 'fill-red-500 text-red-500' : 'text-muted-foreground'
            )}
          />
        </button>
        {trailing}
      </div>
    </StationMenu>
  );
});

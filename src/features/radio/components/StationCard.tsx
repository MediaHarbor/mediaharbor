import { memo } from 'react';
import { Heart, Play, Volume2 } from 'lucide-react';
import { cn } from '@/utils/cn';
import { StationIcon } from '@/features/radio/components/StationIcon';
import { StationMenu } from '@/features/radio/components/StationMenu';
import { useRadioActions } from '@/features/radio/RadioActions';
import { type RadioStation } from '@/features/radio/api';
import { stationSubtitle } from '@/features/radio/format';

export interface StationTileProps {
  station: RadioStation;
  favorite: boolean;
  playing: boolean;
  /** Already resolved by the collection — see `useStationCovers`. */
  cover: string | null;
  onRemoveFromList?: (station: RadioStation) => void;
}

interface Props extends StationTileProps {
  /** Rendered inside the card, top-left — the drag handle of a list row. */
  handle?: React.ReactNode;
}

/**
 * A station tile.
 *
 * The favicon falls back to the placeholder rather than being hidden: a station
 * whose icon 404s used to leave an empty square, which reads as a broken card
 * instead of a station without a logo.
 */
export const StationCard = memo(function StationCard({
  station,
  favorite,
  playing,
  cover,
  handle,
  onRemoveFromList,
}: Props) {
  const { onPlay, onToggleFavorite } = useRadioActions();

  return (
    <StationMenu station={station} favorite={favorite} onRemoveFromList={onRemoveFromList}>
      <div
        className={cn(
          'group relative flex flex-col overflow-hidden rounded-lg border bg-muted/20 text-left',
          'transition-colors duration-100 hover:border-border hover:bg-muted/40',
          playing ? 'border-[var(--service-accent)]' : 'border-border/40'
        )}
      >
        <button
          type="button"
          onClick={() => onPlay(station)}
          className="text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring rounded-lg"
          aria-label={`Play ${station.name}`}
        >
          <div className="relative aspect-square w-full bg-muted/40">
            <StationIcon src={cover} className="h-full w-full" iconClassName="h-8 w-8" />
            <div
              className={cn(
                'absolute bottom-2 right-2 flex h-9 w-9 items-center justify-center rounded-full',
                'bg-primary text-primary-foreground shadow-lg transition-opacity',
                playing ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
              )}
            >
              {playing ? (
                <Volume2 className="h-4 w-4" />
              ) : (
                <Play className="h-4 w-4 fill-current" />
              )}
            </div>
          </div>
          <div className="p-3">
            <p className="truncate text-sm font-medium">{station.name}</p>
            <p className="mt-0.5 truncate text-[10px] uppercase tracking-wide text-muted-foreground/60">
              {stationSubtitle(station)}
            </p>
          </div>
        </button>

        {handle && <div className="absolute left-2 top-2 z-10">{handle}</div>}

        <button
          type="button"
          onClick={() => onToggleFavorite(station, !favorite)}
          aria-pressed={favorite}
          title={favorite ? 'Remove from favourites' : 'Save station'}
          className={cn(
            'absolute right-2 top-2 rounded-full bg-background/80 p-1.5 backdrop-blur transition-opacity',
            'focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
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
      </div>
    </StationMenu>
  );
});

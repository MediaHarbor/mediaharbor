import { Slider } from '@/components/ui/slider';
import { usePlayerStore } from '@/stores/usePlayerStore';
import { formatDuration } from '@/utils/formatters';

interface PlaybackSeekBarProps {
  duration: number;
  onSeek: (val: number[]) => void;
  trackColor?: string;
}

/// Subscribes to `position` itself so the ~10 Hz clock tick re-renders this row alone.
/// Held in the player shell it re-rendered the whole ~1000-line component ten times a
/// second to move one slider.
export function PlaybackSeekBar({ duration, onSeek, trackColor }: PlaybackSeekBarProps) {
  const position = usePlayerStore((s) => s.position);
  return (
    <div className="flex items-center gap-2 w-full max-w-[480px]">
      <span className="text-[11px] text-muted-foreground tabular-nums w-8 text-right shrink-0 select-none">
        {formatDuration(position)}
      </span>
      <Slider
        min={0}
        max={duration || 100}
        step={0.5}
        value={[position]}
        onValueChange={onSeek}
        className="flex-1"
        trackColor={trackColor}
      />
      <span className="text-[11px] text-muted-foreground tabular-nums w-8 shrink-0 select-none">
        {formatDuration(duration)}
      </span>
    </div>
  );
}

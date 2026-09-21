import { useCallback, useRef, useState } from 'react';
import { ChevronLeft, GripVertical, Pencil, Play, Radio, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/utils/cn';
import { useRadioList } from '@/features/radio/hooks/useRadioQueries';
import {
  useDeleteList,
  useRemoveFromList,
  useRenameList,
  useReorderList,
} from '@/features/radio/hooks/useRadioMutations';
import { useStationCovers } from '@/features/radio/hooks/useStationCover';
import { RadioEmpty, RadioError } from '@/features/radio/components/RadioStates';
import { StationRow } from '@/features/radio/components/StationRow';
import { NameDialog } from '@/features/radio/components/NameDialog';
import { coverOf } from '@/features/radio/hooks/useStationCover';
import type { StationChrome } from '@/features/radio/RadioActions';
import type { RadioStation } from '@/features/radio/api';

/** Stable identity while the list is loading, so the covers hook does not churn. */
const NO_STATIONS: RadioStation[] = [];

interface Props extends StationChrome {
  id: number;
  onBack: () => void;
  onPlayAll: (stations: RadioStation[]) => void;
}

export function ListDetailView({ id, onBack, onPlayAll, ...chrome }: Props) {
  const detail = useRadioList(id);
  const rename = useRenameList();
  const remove = useDeleteList();
  const removeAt = useRemoveFromList();
  const reorder = useReorderList();
  const [renaming, setRenaming] = useState(false);

  const stations = detail.data?.stations ?? NO_STATIONS;

  const move = useCallback(
    (from: number, to: number) => {
      if (to < 0 || to >= stations.length || from === to) return;
      reorder.mutate({ id, from, to });
    },
    [id, reorder, stations.length]
  );

  if (detail.isError) return <RadioError error={detail.error} what="this list" />;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center gap-2 pb-3">
        <Button size="sm" variant="ghost" onClick={onBack}>
          <ChevronLeft className="mr-1 h-3.5 w-3.5" /> Playlists
        </Button>
        <h2 className="truncate text-sm font-semibold">{detail.data?.name ?? 'List'}</h2>
        <span className="text-[11px] text-muted-foreground/50">
          {stations.length} station{stations.length === 1 ? '' : 's'}
        </span>
        <div className="ml-auto flex items-center gap-1">
          <Button
            size="sm"
            variant="ghost"
            disabled={stations.length === 0}
            onClick={() => onPlayAll(stations)}
          >
            <Play className="mr-1.5 h-3.5 w-3.5 fill-current" /> Play
          </Button>
          <Button
            size="icon"
            variant="ghost"
            aria-label="Rename playlist"
            onClick={() => setRenaming(true)}
          >
            <Pencil className="h-3.5 w-3.5" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            aria-label="Delete playlist"
            className="text-muted-foreground hover:text-destructive"
            onClick={() => {
              remove.mutate(id);
              onBack();
            }}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto pb-8">
        {detail.isPending ? (
          <div className="space-y-1.5">
            {Array.from({ length: 6 }, (_, i) => (
              <div key={i} className="h-14 animate-pulse rounded-lg bg-muted/30" />
            ))}
          </div>
        ) : stations.length === 0 ? (
          <RadioEmpty
            icon={Radio}
            title="This playlist is empty"
            hint="Right-click a station anywhere on this page and add it here."
          />
        ) : (
          <ReorderableRows
            stations={stations}
            chrome={chrome}
            onMove={move}
            onRemove={(index) => removeAt.mutate({ id, position: index })}
          />
        )}
      </div>

      <NameDialog
        open={renaming}
        title="Rename playlist"
        confirm="Rename"
        initial={detail.data?.name ?? ''}
        onClose={() => setRenaming(false)}
        onSubmit={(name) => {
          rename.mutate({ id, name });
          setRenaming(false);
        }}
      />
    </div>
  );
}

/**
 * Drag to reorder, with a keyboard path that does the same thing.
 *
 * Native HTML5 drag-and-drop rather than a library: this is the only reorderable
 * surface in the app, and it is worth neither a dependency nor a new primitive.
 * The handle is a real button, so Alt+Up / Alt+Down reach the same `onMove` a
 * drop does — a drag-only list is unusable without a mouse.
 *
 * The row itself is the shared `StationRow`, so a station in a playlist looks
 * and behaves like a station anywhere else; only the handle and the remove
 * button are added here.
 */
function ReorderableRows({
  stations,
  chrome,
  onMove,
  onRemove,
}: {
  stations: RadioStation[];
  chrome: StationChrome;
  onMove: (from: number, to: number) => void;
  onRemove: (index: number) => void;
}) {
  const dragFrom = useRef<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const covers = useStationCovers(stations);

  return (
    <ul className="space-y-1.5">
      {stations.map((station, index) => (
        <li
          key={station.key}
          draggable
          onDragStart={(e) => {
            dragFrom.current = index;
            e.dataTransfer.effectAllowed = 'move';
            // Firefox refuses to start a drag without payload.
            e.dataTransfer.setData('text/plain', station.key);
          }}
          onDragOver={(e) => {
            e.preventDefault();
            e.dataTransfer.dropEffect = 'move';
            if (over !== index) setOver(index);
          }}
          onDragLeave={() => setOver((o) => (o === index ? null : o))}
          onDrop={(e) => {
            e.preventDefault();
            const from = dragFrom.current;
            dragFrom.current = null;
            setOver(null);
            if (from !== null) onMove(from, index);
          }}
          onDragEnd={() => {
            dragFrom.current = null;
            setOver(null);
          }}
          className={cn('rounded-lg', over === index && 'ring-1 ring-[var(--service-accent)]')}
        >
          <StationRow
            station={station}
            favorite={chrome.favoriteKeys.has(station.key)}
            playing={chrome.nowPlayingKey === station.key}
            cover={coverOf(station, covers)}
            onRemoveFromList={() => onRemove(index)}
            handle={
              <button
                type="button"
                aria-label={`Reorder ${station.name}. Position ${index + 1} of ${stations.length}. Use Alt with the arrow keys to move.`}
                className="shrink-0 cursor-grab rounded p-1 text-muted-foreground/40 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                onKeyDown={(e) => {
                  if (!e.altKey) return;
                  if (e.key === 'ArrowUp') {
                    e.preventDefault();
                    onMove(index, index - 1);
                  } else if (e.key === 'ArrowDown') {
                    e.preventDefault();
                    onMove(index, index + 1);
                  }
                }}
              >
                <GripVertical className="h-4 w-4" />
              </button>
            }
            trailing={
              <button
                type="button"
                onClick={() => onRemove(index)}
                aria-label={`Remove ${station.name} from this playlist`}
                className="shrink-0 rounded p-1.5 text-muted-foreground/40 opacity-0 transition-opacity hover:text-destructive focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring group-hover:opacity-100"
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            }
          />
        </li>
      ))}
    </ul>
  );
}

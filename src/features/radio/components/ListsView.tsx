import { useState } from 'react';
import { ListMusic, Plus } from 'lucide-react';
import { StationIcon } from '@/features/radio/components/StationIcon';
import { NameDialog } from '@/features/radio/components/NameDialog';
import { RadioEmpty, RadioError } from '@/features/radio/components/RadioStates';
import { useCreateList } from '@/features/radio/hooks/useRadioMutations';
import { useRadioLists } from '@/features/radio/hooks/useRadioQueries';
import { useRadioStore } from '@/features/radio/stores/useRadioStore';
import type { RadioList } from '@/features/radio/api';

/** The playlist shelf. Opening one is a store change, not a screen this owns. */
export function ListsView() {
  const lists = useRadioLists();
  const openList = useRadioStore((s) => s.openList);
  const [creating, setCreating] = useState(false);
  const createList = useCreateList();

  if (lists.error) return <RadioError error={lists.error} what="your lists" />;
  const rows = lists.data ?? [];

  return (
    <div className="h-full overflow-y-auto pb-8">
      <div
        className="grid gap-3 pt-1"
        style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(196px, 1fr))' }}
      >
        <button
          type="button"
          onClick={() => setCreating(true)}
          className="flex min-h-[124px] flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-border/60 bg-transparent text-sm text-muted-foreground transition-colors hover:border-[var(--service-accent)] hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <Plus className="h-5 w-5" />
          New playlist
        </button>
        {lists.isPending
          ? Array.from({ length: 3 }, (_, i) => (
              <div key={i} className="h-[124px] animate-pulse rounded-xl bg-muted/30" />
            ))
          : rows.map((list) => (
              <ListCard key={list.id} list={list} onOpen={() => openList(list.id)} />
            ))}
      </div>

      {!lists.isPending && rows.length === 0 && (
        <RadioEmpty
          icon={ListMusic}
          title="No playlists yet"
          hint="A playlist keeps stations in the order you choose — a morning set, a work set. Right-click any station to add it to one."
        />
      )}

      <NameDialog
        open={creating}
        title="New playlist"
        confirm="Create"
        onClose={() => setCreating(false)}
        onSubmit={(name) => {
          createList.mutate(name);
          setCreating(false);
        }}
      />
    </div>
  );
}

function ListCard({ list, onOpen }: { list: RadioList; onOpen: () => void }) {
  return (
    <button
      type="button"
      onClick={onOpen}
      className="group flex flex-col gap-3 rounded-xl border border-border/40 bg-muted/20 p-3 text-left transition-colors hover:border-[var(--service-accent)] hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
    >
      <div className="grid aspect-square w-full grid-cols-2 grid-rows-2 overflow-hidden rounded-lg bg-muted/40">
        {list.favicons.length === 0 ? (
          <div className="col-span-2 row-span-2 flex items-center justify-center">
            <ListMusic className="h-7 w-7 text-muted-foreground/30" />
          </div>
        ) : (
          // Through `StationIcon` like every other station image, so a favicon
          // that 404s falls back instead of leaving a broken-image box.
          Array.from({ length: 4 }, (_, i) => (
            <StationIcon
              key={i}
              src={list.favicons[i % list.favicons.length] ?? null}
              className="h-full w-full"
              iconClassName="h-4 w-4"
            />
          ))
        )}
      </div>
      <div>
        <p className="truncate text-sm font-medium">{list.name}</p>
        <p className="mt-0.5 text-[10px] uppercase tracking-wide text-muted-foreground/60">
          {list.stationCount} station{list.stationCount === 1 ? '' : 's'}
        </p>
      </div>
    </button>
  );
}

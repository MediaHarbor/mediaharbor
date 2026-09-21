import { useCallback, useMemo, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { radioKeys, type RadioStation } from '@/features/radio/api';
import {
  useFavoriteKeys,
  useRadioLists,
  useRadioSources,
  useRadioWarmupRefresh,
  useRecentOnStreamReady,
} from '@/features/radio/hooks/useRadioQueries';
import {
  useAddToList,
  useCreateList,
  useForgetStation,
  useSetRadioSources,
  useToggleFavorite,
} from '@/features/radio/hooks/useRadioMutations';
import {
  useNowPlayingKey,
  usePlayStation,
  usePlayStations,
} from '@/features/radio/hooks/useRadioPlayback';
import { useRadioStore, type RadioFilters } from '@/features/radio/stores/useRadioStore';
import { RadioActionsContext, type RadioActions } from '@/features/radio/RadioActions';
import { RadioTabs } from '@/features/radio/components/RadioTabs';
import { SourceChips } from '@/features/radio/components/SourceChips';
import { SearchBox } from '@/features/radio/components/SearchBox';
import { SearchResults } from '@/features/radio/components/SearchResults';
import { HomeView } from '@/features/radio/components/HomeView';
import { BrowseView } from '@/features/radio/components/BrowseView';
import { FavoritesView } from '@/features/radio/components/FavoritesView';
import { ListsView } from '@/features/radio/components/ListsView';
import { ListDetailView } from '@/features/radio/components/ListDetailView';
import { NameDialog } from '@/features/radio/components/NameDialog';
import { ImportDialog } from '@/features/radio/components/ImportDialog';
import { StationEditor } from '@/features/radio/components/StationEditor';

/**
 * Internet radio.
 *
 * Deliberately not a service platform: `radio` never joins the `Platform` union,
 * carries no `ServiceDescriptor`, and reaches playback as a pass-through
 * pseudo-platform. It is its own page precisely so it needs none of that
 * machinery.
 */
const ACCENT = '#F2A33C';

/** Stable identity while the roster loads, so cards keep their memoized props. */
const NO_LISTS: RadioActions['lists'] = [];

export default function RadioPage() {
  const qc = useQueryClient();
  const tab = useRadioStore((s) => s.tab);
  const setTab = useRadioStore((s) => s.setTab);
  const setFilter = useRadioStore((s) => s.setFilter);
  const clearFilters = useRadioStore((s) => s.clearFilters);
  const openListId = useRadioStore((s) => s.openListId);
  const openList = useRadioStore((s) => s.openList);

  const [search, setSearch] = useState('');
  const [importing, setImporting] = useState(false);
  const [pendingListStation, setPendingListStation] = useState<RadioStation | null>(null);
  const [editing, setEditing] = useState<{ station: RadioStation; unsaved: boolean } | null>(null);

  const { sources, isPending: sourcesLoading, error: sourcesError } = useRadioSources();
  const setSources = useSetRadioSources();
  const lists = useRadioLists();
  const favoriteKeys = useFavoriteKeys();
  const nowPlayingKey = useNowPlayingKey();
  const play = usePlayStation();
  const playAll = usePlayStations();
  const { mutate: toggleFavorite } = useToggleFavorite();
  const { mutate: addToList } = useAddToList();
  const { mutate: createList } = useCreateList();
  const { mutate: forget } = useForgetStation();

  useRadioWarmupRefresh();
  useRecentOnStreamReady();

  const enabledIds = useMemo(
    () => sources.filter((s) => s.enabled && s.toggleable).map((s) => s.id),
    [sources]
  );

  const onToggleSource = useCallback(
    (id: string, enabled: boolean) => {
      setSources.mutate(enabled ? [...enabledIds, id] : enabledIds.filter((s) => s !== id));
    },
    [enabledIds, setSources]
  );

  // `mutate` is a stable reference; the mutation object around it is not, and an
  // unstable value here would defeat the memo on every station card.
  const actions = useMemo<RadioActions>(
    () => ({
      onPlay: play,
      onToggleFavorite: (station, favorite) => toggleFavorite({ station, favorite }),
      onAddToList: (station, list) =>
        addToList({ id: list.id, keys: [station.key], listName: list.name }),
      onCreateListWith: (station) => setPendingListStation(station),
      onEdit: (station) => setEditing({ station, unsaved: false }),
      onForget: (station) => forget(station.key),
      lists: lists.data ?? NO_LISTS,
    }),
    [play, toggleFavorite, addToList, forget, lists.data]
  );

  const chrome = { favoriteKeys, nowPlayingKey };
  const searching = search.trim().length > 0;

  /** A shelf's "Show all" lands in Browse with that shelf's filters applied. */
  const showAll = useCallback(
    (filters: Partial<RadioFilters>) => {
      clearFilters();
      (Object.entries(filters) as [keyof RadioFilters, never][]).forEach(([key, value]) =>
        setFilter(key, value)
      );
      setSearch('');
      setTab('browse');
    },
    [clearFilters, setFilter, setTab]
  );

  return (
    <RadioActionsContext.Provider value={actions}>
      <div
        className="flex h-full min-h-0 flex-col overflow-hidden"
        style={{ ['--service-accent' as 'color']: ACCENT }}
      >
        <div className="mx-auto flex min-h-0 w-full max-w-[1600px] flex-1 flex-col p-6">
          <header className="flex flex-wrap items-center gap-x-4 gap-y-2 pb-3">
            <h1 className="shrink-0 text-2xl font-bold tracking-tight">Radio</h1>
            <SearchBox
              className="min-w-[220px] flex-1"
              value={search}
              onChange={setSearch}
              onPickTag={(tag) => {
                setSearch('');
                showAll({ tag });
              }}
            />
            <div className="flex items-center gap-3">
              <SourceChips
                sources={sources}
                loading={sourcesLoading}
                error={sourcesError}
                pending={setSources.isPending}
                onToggle={onToggleSource}
              />
              <Button size="sm" variant="outline" onClick={() => setImporting(true)}>
                <Plus className="mr-1.5 h-3.5 w-3.5" /> Add station
              </Button>
            </div>
          </header>

          {/* Hidden rather than unmounted while searching, so clearing the box
              returns to the tab you were on with its scroll position intact. */}
          <div className={searching ? 'hidden' : undefined}>
            <RadioTabs value={tab} onChange={setTab} />
          </div>

          <div className="min-h-0 flex-1 pt-4">
            {searching ? (
              <SearchResults text={search} onClear={() => setSearch('')} {...chrome} />
            ) : (
              <>
                {tab === 'home' && (
                  <HomeView sourceCount={enabledIds.length} onShowAll={showAll} {...chrome} />
                )}
                {tab === 'browse' && <BrowseView sources={sources} {...chrome} />}
                {tab === 'favorites' && <FavoritesView {...chrome} />}
                {tab === 'playlists' &&
                  (openListId === null ? (
                    <ListsView />
                  ) : (
                    <ListDetailView
                      id={openListId}
                      onBack={() => openList(null)}
                      onPlayAll={playAll}
                      {...chrome}
                    />
                  ))}
              </>
            )}
          </div>
        </div>

        <ImportDialog
          open={importing}
          onClose={() => setImporting(false)}
          onReview={(station) => {
            setImporting(false);
            setEditing({ station, unsaved: true });
          }}
        />

        {editing && (
          // Keyed by station, so opening a different one remounts the form
          // instead of the editor having to re-seed itself from an effect.
          <StationEditor
            key={editing.station.key}
            station={editing.station}
            unsaved={editing.unsaved}
            onClose={() => setEditing(null)}
            onSaved={() => void qc.invalidateQueries({ queryKey: radioKeys.all })}
          />
        )}

        <NameDialog
          open={pendingListStation !== null}
          title="New playlist"
          confirm="Create and add"
          onClose={() => setPendingListStation(null)}
          onSubmit={(name: string) => {
            const station = pendingListStation;
            setPendingListStation(null);
            if (!station) return;
            createList(name, {
              onSuccess: ({ id }) => addToList({ id, keys: [station.key], listName: name }),
            });
          }}
        />
      </div>
    </RadioActionsContext.Provider>
  );
}

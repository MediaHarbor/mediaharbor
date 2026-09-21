import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { ListChecks, Plus, Upload, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { libraryKeys, errorMessage, type ServicePlatform } from '@/features/library/api';
import { useLibraryInfiniteQuery } from '@/features/library/hooks/useLibraryInfiniteQuery';
import { CreatePlaylistDialog } from '@/features/library/components/CreatePlaylistDialog';
import { RenamePlaylistDialog } from '@/features/library/components/RenamePlaylistDialog';
import { DeletePlaylistConfirm } from '@/features/library/components/DeletePlaylistConfirm';
import { PlaylistContextMenu, PlaylistKebab } from '@/features/library/actions/RowContextMenu';
import { useOwnedPlaylists } from '@/features/library/hooks/useLibraryMutations';
import { useVisibleCoverUrls } from '@/features/library/useCoverUrls';
import { normalizePlatform } from '@/utils/platform-data';
import { tauriAPI } from '@/tauri-bridge';

export interface LibraryPlaylist {
  id: number;
  name: string;
  created_at: number;
  updated_at: number;
  track_count: number;
  cover_ids: string[];
  service_id?: string | null;
  description?: string | null;
  owner?: string | null;
}

interface PlaylistsViewProps {
  search: string;
  source?: ServicePlatform;
  onSelectPlaylist: (id: number, serviceId?: string | null) => void;
}

export function PlaylistsView({ search, source = 'local', onSelectPlaylist }: PlaylistsViewProps) {
  const [newName, setNewName] = useState('');
  const [creating, setCreating] = useState(false);
  const sentinelRef = useRef<HTMLDivElement>(null);

  const [createOpen, setCreateOpen] = useState(false);
  const [renameTarget, setRenameTarget] = useState<{
    id: string;
    name: string;
    description?: string | null;
  } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null);

  const isLocal = source === 'local';
  const platformId = normalizePlatform(source);

  const ownedQ = useOwnedPlaylists(isLocal ? null : platformId);

  const qc = useQueryClient();
  const { query, items: playlists } = useLibraryInfiniteQuery<LibraryPlaylist>('playlists', {
    search: '',
    source,
  });
  const loadError = query.isError ? errorMessage(query.error) : null;

  const refresh = useCallback(() => {
    void qc.invalidateQueries({ queryKey: libraryKeys.playlists('', source) });
  }, [qc, source]);

  useEffect(() => {
    const el = sentinelRef.current;
    if (!el || !query.hasNextPage) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((en) => en.isIntersecting) && !query.isFetchingNextPage) {
          query.fetchNextPage();
        }
      },
      { rootMargin: '400px' }
    );
    io.observe(el);
    return () => io.disconnect();
  }, [query.hasNextPage, query.isFetchingNextPage, query]);

  const covers = useVisibleCoverUrls(playlists.flatMap((p) => p.cover_ids));

  const merged = useMemo(() => {
    if (isLocal) return playlists;
    const owned = ownedQ.data ?? [];
    const fromLibByServiceId = new Map<string, LibraryPlaylist>();
    for (const p of playlists) {
      if (p.service_id) fromLibByServiceId.set(p.service_id, p);
    }
    const out: LibraryPlaylist[] = [];
    for (const o of owned) {
      const hit = fromLibByServiceId.get(o.serviceId);
      out.push(
        hit ?? {
          id: -Math.abs(o.serviceId.split('').reduce((a, c) => a + c.charCodeAt(0), 0)),
          name: o.name,
          created_at: 0,
          updated_at: o.updatedAt,
          track_count: o.trackCount,
          cover_ids: o.coverId ? [o.coverId] : [],
          service_id: o.serviceId,
        }
      );
      fromLibByServiceId.delete(o.serviceId);
    }
    for (const p of playlists) {
      if (p.service_id && !out.find((q) => q.service_id === p.service_id)) {
        out.push(p);
      }
    }
    return out;
  }, [isLocal, playlists, ownedQ.data]);

  const filtered = search.trim()
    ? merged.filter((p) => p.name.toLowerCase().includes(search.trim().toLowerCase()))
    : merged;

  const onCreateLocal = async () => {
    const name = newName.trim();
    if (!name) return;
    await tauriAPI.library.playlists?.create(name);
    setNewName('');
    setCreating(false);
    refresh();
  };

  const onImport = async () => {
    const src = await tauriAPI.settings.openFile();
    if (!src) return;
    await tauriAPI.library.playlists?.importM3u(src);
    refresh();
  };

  const onDeleteLocal = async (id: number, name: string) => {
    if (!confirm(`Delete playlist "${name}"?`)) return;
    await tauriAPI.library.playlists?.delete(id);
    refresh();
  };

  if (loadError) {
    return (
      <div className="py-16 text-center text-sm text-red-400/80 whitespace-pre-wrap">
        {`Failed to load playlists from ${source}:\n${loadError}`}
      </div>
    );
  }

  const ownedSet = new Set(isLocal ? [] : (ownedQ.data ?? []).map((o) => o.serviceId));

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        {isLocal ? (
          creating ? (
            <>
              <Input
                autoFocus
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') onCreateLocal();
                  if (e.key === 'Escape') setCreating(false);
                }}
                placeholder="Playlist name"
                className="max-w-xs h-8"
              />
              <Button size="sm" onClick={onCreateLocal}>
                Create
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setCreating(false)}>
                Cancel
              </Button>
            </>
          ) : (
            <Button size="sm" onClick={() => setCreating(true)} className="gap-2">
              <Plus className="h-4 w-4" />
              New playlist
            </Button>
          )
        ) : (
          <Button size="sm" onClick={() => setCreateOpen(true)} className="gap-2">
            <Plus className="h-4 w-4" />
            New {source} playlist
          </Button>
        )}
        {isLocal && (
          <Button size="sm" variant="outline" onClick={onImport} className="gap-2">
            <Upload className="h-4 w-4" />
            Import .m3u
          </Button>
        )}
      </div>

      {filtered.length === 0 ? (
        <div className="text-center py-16 text-muted-foreground/50 text-sm">
          {search
            ? 'No playlists match your search.'
            : isLocal
              ? 'No playlists yet. Create one or import an .m3u file.'
              : `No playlists on ${source}. Create one above to get started.`}
        </div>
      ) : (
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
          {filtered.map((p) => {
            const card = (
              <div
                className="group rounded-lg p-3 hover:bg-card/60 transition-colors cursor-pointer relative"
                onClick={() => onSelectPlaylist(p.id, p.service_id ?? null)}
              >
                {isLocal && !p.service_id?.startsWith('dir:') && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onDeleteLocal(p.id, p.name);
                    }}
                    className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded hover:bg-destructive/20 text-muted-foreground/60 hover:text-destructive z-10"
                    title="Delete playlist"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                )}
                {!isLocal && p.service_id && (
                  <div
                    className="absolute top-2 right-2 z-10 opacity-0 group-hover:opacity-100 transition-opacity rounded-md bg-background/70 backdrop-blur-sm"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <PlaylistKebab
                      size="sm"
                      platform={platformId}
                      context="library"
                      playlist={{
                        id: p.service_id,
                        name: p.name,
                        cover_url: null,
                        owned: ownedSet.has(p.service_id),
                      }}
                      onOpen={() => onSelectPlaylist(p.id, p.service_id ?? null)}
                      onRename={
                        ownedSet.has(p.service_id)
                          ? () =>
                              setRenameTarget({
                                id: p.service_id!,
                                name: p.name,
                                description: p.description,
                              })
                          : undefined
                      }
                      onDelete={
                        ownedSet.has(p.service_id)
                          ? () => setDeleteTarget({ id: p.service_id!, name: p.name })
                          : undefined
                      }
                    />
                  </div>
                )}
                {(() => {
                  const urls = p.cover_ids
                    .map((id) => (id ? covers[id] : null))
                    .filter((u): u is string => !!u);
                  if (urls.length === 0) {
                    return (
                      <div className="aspect-square rounded-md overflow-hidden bg-muted flex items-center justify-center mb-2">
                        <ListChecks className="h-10 w-10 text-muted-foreground/30" />
                      </div>
                    );
                  }
                  if (urls.length < 4) {
                    return (
                      <div className="aspect-square rounded-md overflow-hidden bg-muted mb-2">
                        <img
                          src={urls[0]}
                          alt=""
                          loading="lazy"
                          className="w-full h-full object-cover"
                        />
                      </div>
                    );
                  }
                  return (
                    <div className="aspect-square rounded-md overflow-hidden bg-muted grid grid-cols-2 grid-rows-2 gap-px mb-2">
                      {urls.slice(0, 4).map((url, i) => (
                        <div key={i} className="bg-muted/40">
                          <img
                            src={url}
                            alt=""
                            loading="lazy"
                            className="w-full h-full object-cover"
                          />
                        </div>
                      ))}
                    </div>
                  );
                })()}
                <p className="text-sm font-medium truncate">{p.name}</p>
                {p.owner && (
                  <p className="text-[11px] text-muted-foreground/70 truncate">by {p.owner}</p>
                )}
                <p className="text-[11px] text-muted-foreground/50">
                  {p.track_count} track{p.track_count !== 1 ? 's' : ''}
                </p>
                {p.description && (
                  <p className="text-[11px] text-muted-foreground/40 truncate">{p.description}</p>
                )}
              </div>
            );

            if (!isLocal && p.service_id) {
              const owned = ownedSet.has(p.service_id);
              return (
                <PlaylistContextMenu
                  key={p.id}
                  platform={platformId}
                  playlist={{
                    id: p.service_id,
                    name: p.name,
                    cover_url: null,
                    owned,
                  }}
                  context="library"
                  onOpen={() => onSelectPlaylist(p.id, p.service_id ?? null)}
                  onRename={
                    owned
                      ? () =>
                          setRenameTarget({
                            id: p.service_id!,
                            name: p.name,
                            description: p.description,
                          })
                      : undefined
                  }
                  onDelete={
                    owned ? () => setDeleteTarget({ id: p.service_id!, name: p.name }) : undefined
                  }
                >
                  {card}
                </PlaylistContextMenu>
              );
            }
            return (
              <div key={p.id} className="contents">
                {card}
              </div>
            );
          })}
        </div>
      )}

      <div ref={sentinelRef} aria-hidden className="h-px w-full" />
      {query.isFetchingNextPage && (
        <div className="py-6 text-center text-xs text-muted-foreground/40">Loading more…</div>
      )}

      {!isLocal && (
        <CreatePlaylistDialog
          open={createOpen}
          onOpenChange={setCreateOpen}
          platform={platformId}
          onCreated={() => refresh()}
        />
      )}

      {!isLocal && renameTarget && (
        <RenamePlaylistDialog
          open
          onOpenChange={(v) => {
            if (!v) setRenameTarget(null);
          }}
          platform={platformId}
          playlistId={renameTarget.id}
          currentName={renameTarget.name}
          currentDescription={renameTarget.description ?? undefined}
        />
      )}
      {!isLocal && deleteTarget && (
        <DeletePlaylistConfirm
          open
          onOpenChange={(v) => {
            if (!v) setDeleteTarget(null);
          }}
          platform={platformId}
          playlistId={deleteTarget.id}
          playlistName={deleteTarget.name}
        />
      )}
    </div>
  );
}

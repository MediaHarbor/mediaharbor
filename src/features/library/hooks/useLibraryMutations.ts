import { useEffect } from 'react';
import { errorMessage } from '@/utils/errors';
import {
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
  type InfiniteData,
} from '@tanstack/react-query';
import { useSavedStateStore } from '@/stores/useSavedStateStore';
import type {
  OwnedPlaylistRow,
  PlaylistMutateResult,
  RadioResult,
  SaveKindStr,
} from '@/tauri-bridge';
import { libraryKeys } from '@/features/library/api';
import { logWarning } from '@/utils/logger';
import { tauriAPI } from '@/tauri-bridge';

type LibraryPage = { items: Array<{ service_id?: string | null }>; total: number | null };
type PlaylistsInfinite = InfiniteData<LibraryPage>;

interface OwnedSnapshot {
  owned: OwnedPlaylistRow[] | undefined;
  library: PlaylistsInfinite | undefined;
}

function snapshotOwned(qc: QueryClient, platform: string): OwnedSnapshot {
  return {
    owned: qc.getQueryData<OwnedPlaylistRow[]>(ownedKey(platform)),
    library: qc.getQueryData<PlaylistsInfinite>(libraryKeys.playlists('', platform)),
  };
}

function restoreOwned(qc: QueryClient, platform: string, snap: OwnedSnapshot) {
  qc.setQueryData(ownedKey(platform), snap.owned);
  qc.setQueryData(libraryKeys.playlists('', platform), snap.library);
}

function removeOwnedPlaylist(qc: QueryClient, platform: string, serviceId: string) {
  qc.setQueryData<OwnedPlaylistRow[]>(ownedKey(platform), (rows) =>
    rows?.filter((r) => r.serviceId !== serviceId)
  );
  qc.setQueryData<PlaylistsInfinite>(libraryKeys.playlists('', platform), (data) =>
    data
      ? {
          ...data,
          pages: data.pages.map((p) => {
            const items = p.items.filter((it) => it.service_id !== serviceId);
            const removed = p.items.length - items.length;
            return {
              ...p,
              items,
              total: p.total === null ? null : Math.max(0, p.total - removed),
            };
          }),
        }
      : data
  );
}

function patchOwnedPlaylist(
  qc: QueryClient,
  platform: string,
  serviceId: string,
  patch: Partial<OwnedPlaylistRow>
) {
  qc.setQueryData<OwnedPlaylistRow[]>(ownedKey(platform), (rows) =>
    rows?.map((r) => (r.serviceId === serviceId ? { ...r, ...patch } : r))
  );
}

function addOwnedPlaylist(qc: QueryClient, platform: string, row: OwnedPlaylistRow) {
  qc.setQueryData<OwnedPlaylistRow[]>(ownedKey(platform), (rows) =>
    rows ? [row, ...rows] : [row]
  );
}

const bridge = () => tauriAPI.serviceLibrary;

const ensureBridge = () => {
  const b = bridge();
  if (!b) throw new Error('Tauri bridge not available');
  return b;
};

const savedKey = (platform: string, kind: SaveKindStr) => ['saved-state', platform, kind] as const;

const ownedKey = (platform: string) => ['service-library', 'owned-playlists', platform] as const;

export function useSavedStateHydrate(platform: string, kind: SaveKindStr) {
  const hasHydrated = useSavedStateStore((s) => s.hasHydrated);
  const hydrate = useSavedStateStore((s) => s.hydrate);
  const getInflight = useSavedStateStore((s) => s.getInflight);
  const setInflight = useSavedStateStore((s) => s.setInflight);

  useEffect(() => {
    if (!platform || platform === 'local') return;
    if (hasHydrated(platform, kind)) return;
    if (getInflight(platform, kind)) return;
    const p = (async () => {
      try {
        const ids = await ensureBridge().savedStateFor(platform, kind);
        hydrate(platform, kind, ids);
      } catch (e) {
        logWarning('app', 'Saved-state hydrate failed', `${platform}/${kind}: ${errorMessage(e)}`);
      } finally {
        setInflight(platform, kind, null);
      }
    })();
    setInflight(platform, kind, p);
  }, [platform, kind, hasHydrated, getInflight, hydrate, setInflight]);
}

export function useOwnedPlaylists(platform: string | null | undefined) {
  return useQuery({
    queryKey: ownedKey(platform ?? ''),
    enabled: !!platform && platform !== 'local',
    queryFn: () => ensureBridge().ownedPlaylists(platform!) as Promise<OwnedPlaylistRow[]>,
    staleTime: 30 * 1000,
  });
}

interface ToggleArg {
  id: string;
  save: boolean;
}

function createToggleMutation(kind: SaveKindStr) {
  return function useToggleHook(platform: string) {
    const localToggle = useSavedStateStore((s) => s.localToggle);
    return useMutation({
      mutationFn: async ({ id, save }: ToggleArg) => {
        await ensureBridge().setSaved(platform, kind, save, [id]);
      },
      onMutate: ({ id, save }) => {
        localToggle(platform, kind, id, save);
        return { id, save };
      },
      onError: (err, _vars, ctx) => {
        logWarning('app', 'Save toggle failed', `${platform}/${kind}: ${errorMessage(err)}`);
        if (ctx) localToggle(platform, kind, ctx.id, !ctx.save);
      },
    });
  };
}

export const useToggleSavedTrack = createToggleMutation('track');
export const useToggleSavedAlbum = createToggleMutation('album');
export const useToggleFollowedArtist = createToggleMutation('artist');
export const useToggleFollowedPlaylist = createToggleMutation('playlist');

interface CreateArg {
  platform: string;
  name: string;
  description?: string;
  isPublic?: boolean;
  isCollaborative?: boolean;
  initialTrackIds?: string[];
}
export function useCreateServicePlaylist() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (req: CreateArg) =>
      ensureBridge().playlists.create(req) as Promise<PlaylistMutateResult>,
    onMutate: async (vars: CreateArg) => {
      await qc.cancelQueries({ queryKey: ownedKey(vars.platform) });
      const snap = snapshotOwned(qc, vars.platform);
      addOwnedPlaylist(qc, vars.platform, {
        platform: vars.platform,
        serviceId: `optimistic:${vars.name}`,
        name: vars.name,
        coverId: null,
        trackCount: vars.initialTrackIds?.length ?? 0,
        updatedAt: Date.now(),
      });
      return snap;
    },
    onError: (_err, vars, snap) => {
      if (snap) restoreOwned(qc, vars.platform, snap);
    },
    onSettled: (_d, _e, vars) => {
      qc.invalidateQueries({ queryKey: ownedKey(vars.platform) });
      qc.invalidateQueries({ queryKey: libraryKeys.playlists('', vars.platform) });
    },
  });
}

interface RenameArg {
  platform: string;
  id: string;
  name: string;
  description?: string;
  isPublic?: boolean;
  isCollaborative?: boolean;
}
export function useRenameServicePlaylist() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (req: RenameArg) => ensureBridge().playlists.rename(req),
    onMutate: async (vars: RenameArg) => {
      await qc.cancelQueries({ queryKey: ownedKey(vars.platform) });
      const snap = snapshotOwned(qc, vars.platform);
      patchOwnedPlaylist(qc, vars.platform, vars.id, { name: vars.name });
      return snap;
    },
    onError: (_err, vars, snap) => {
      if (snap) restoreOwned(qc, vars.platform, snap);
    },
    onSettled: (_d, _e, vars) => {
      qc.invalidateQueries({ queryKey: ownedKey(vars.platform) });
      qc.invalidateQueries({ queryKey: libraryKeys.playlists('', vars.platform) });
    },
  });
}

interface DeleteArg {
  platform: string;
  id: string;
}
export function useDeleteServicePlaylist() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (req: DeleteArg) => ensureBridge().playlists.delete(req),
    onMutate: async (vars: DeleteArg) => {
      await qc.cancelQueries({ queryKey: ownedKey(vars.platform) });
      await qc.cancelQueries({ queryKey: libraryKeys.playlists('', vars.platform) });
      const snap = snapshotOwned(qc, vars.platform);
      removeOwnedPlaylist(qc, vars.platform, vars.id);
      return snap;
    },
    onError: (_err, vars, snap) => {
      if (snap) restoreOwned(qc, vars.platform, snap);
    },
    onSettled: (_d, _e, vars) => {
      qc.invalidateQueries({ queryKey: ownedKey(vars.platform) });
      qc.invalidateQueries({ queryKey: libraryKeys.playlists('', vars.platform) });
    },
  });
}

interface AddTracksArg {
  platform: string;
  id: string;
  trackIds: string[];
}
export function useAddTracksToServicePlaylist() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (req: AddTracksArg) =>
      ensureBridge().playlists.addTracks(req) as Promise<PlaylistMutateResult>,
    onSuccess: (_, vars) => {
      qc.invalidateQueries({ queryKey: ownedKey(vars.platform) });
    },
  });
}

interface RemoveTracksArg {
  platform: string;
  id: string;
  trackIds: string[];
  positions?: number[];
}
export function useRemoveTracksFromServicePlaylist() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (req: RemoveTracksArg) =>
      ensureBridge().playlists.removeTracks(req) as Promise<PlaylistMutateResult>,
    onSuccess: (_, vars) => {
      qc.invalidateQueries({ queryKey: ownedKey(vars.platform) });
    },
  });
}

interface RadioArg {
  platform: string;
  seedKind: 'track' | 'album' | 'artist' | 'playlist';
  seedId: string;
}
export function useStartRadio() {
  return useMutation({
    mutationFn: (req: RadioArg) => ensureBridge().radioFor(req) as Promise<RadioResult>,
  });
}

export { savedKey, ownedKey };

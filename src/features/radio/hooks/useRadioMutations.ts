import { useCallback } from 'react';
import {
  useMutation,
  useQueryClient,
  type QueryClient,
  type QueryKey,
} from '@tanstack/react-query';
import { radioKeys } from '@/features/radio/api';
import type {
  RadioDirectorySource,
  RadioList,
  RadioListDetail,
  RadioStation,
} from '@/features/radio/api';
import { useNotificationStore } from '@/stores/useNotificationStore';
import { errorMessage } from '@/utils/errors';
import { invalidateSettingsDerived } from '@/lib/queryClient';
import { tauriAPI } from '@/tauri-bridge';

function useFailureToast(title: string) {
  const notify = useNotificationStore((s) => s.addNotification);
  return useCallback(
    (e: unknown) => notify({ type: 'error', title, message: errorMessage(e) }),
    [notify, title]
  );
}

/**
 * A radio write that shows its effect before the backend confirms it.
 *
 * Every optimistic mutation here does the same five things — cancel the query it
 * is about to touch, snapshot it, patch it, put the snapshot back if the write
 * fails, and refetch either way. Written out per mutation that was six copies of
 * the same block and a rollback fix in six places.
 *
 * `key` is the query being patched; `also` names anything else the write
 * invalidates once it settles.
 */
function useOptimisticRadio<Vars, Data>(options: {
  title: string;
  key: (vars: Vars) => QueryKey;
  mutationFn: (vars: Vars) => Promise<unknown>;
  patch: (qc: QueryClient, vars: Vars) => void;
  also?: (vars: Vars) => QueryKey[];
  /** For an effect wider than a query key — see `useSetRadioSources`. */
  afterSettled?: () => void;
}) {
  const qc = useQueryClient();
  const onFailure = useFailureToast(options.title);
  const { key, mutationFn, patch, also, afterSettled } = options;

  return useMutation({
    mutationFn,
    onMutate: async (vars: Vars) => {
      const target = key(vars);
      await qc.cancelQueries({ queryKey: target });
      const snapshot = qc.getQueryData<Data>(target);
      patch(qc, vars);
      return snapshot;
    },
    onError: (err, vars, snapshot) => {
      qc.setQueryData(key(vars), snapshot);
      onFailure(err);
    },
    onSettled: (_d, _e, vars) => {
      for (const target of [key(vars), ...(also?.(vars) ?? [])]) {
        void qc.invalidateQueries({ queryKey: target });
      }
      afterSettled?.();
    },
  });
}

/** A write with nothing worth showing early — just report failure and refetch. */
function usePlainRadio<Vars, Data>(options: {
  title: string;
  mutationFn: (vars: Vars) => Promise<Data>;
  invalidates: (vars: Vars) => QueryKey[];
  onSuccess?: (data: Data, vars: Vars) => void;
}) {
  const qc = useQueryClient();
  const onFailure = useFailureToast(options.title);
  const { mutationFn, invalidates, onSuccess } = options;

  return useMutation({
    mutationFn,
    onError: onFailure,
    onSuccess,
    onSettled: (_d, _e, vars) => {
      for (const target of invalidates(vars)) {
        void qc.invalidateQueries({ queryKey: target });
      }
    },
  });
}

function patchListStations(
  qc: QueryClient,
  id: number,
  update: (stations: RadioStation[]) => RadioStation[]
) {
  qc.setQueryData<RadioListDetail | null>(radioKeys.list(id), (detail) => {
    if (!detail) return detail;
    const stations = update(detail.stations);
    return { ...detail, stations, stationCount: stations.length };
  });
}

/**
 * Optimistic favourite toggle: the heart flips on click and rolls back only if
 * the write fails. Waiting for a network round trip made a one-bit change feel
 * like a request.
 */
export function useToggleFavorite() {
  return useOptimisticRadio<{ station: RadioStation; favorite: boolean }, RadioStation[]>({
    title: 'Could not save station',
    key: radioKeys.favorites,
    mutationFn: ({ station, favorite }) => tauriAPI.radio.setFavorite(station.key, favorite),
    patch: (qc, { station, favorite }) =>
      qc.setQueryData<RadioStation[]>(radioKeys.favorites(), (rows) => {
        const without = (rows ?? []).filter((s) => s.key !== station.key);
        return favorite ? [station, ...without] : without;
      }),
  });
}

export function useSetRadioSources() {
  return useOptimisticRadio<string[], RadioDirectorySource[]>({
    title: 'Could not change sources',
    key: radioKeys.sources,
    mutationFn: (sources) => tauriAPI.radio.setSources(sources),
    patch: (qc, sources) =>
      qc.setQueryData<RadioDirectorySource[]>(radioKeys.sources(), (rows) =>
        rows?.map((r) => (r.toggleable ? { ...r, enabled: sources.includes(r.id) } : r))
      ),
    // The source list is a setting, so everything derived from settings —
    // including the Settings page's own copy — has to hear about it.
    afterSettled: invalidateSettingsDerived,
  });
}

export function useRenameList() {
  return useOptimisticRadio<{ id: number; name: string }, RadioList[]>({
    title: 'Could not rename list',
    key: radioKeys.lists,
    mutationFn: ({ id, name }) => tauriAPI.radio.listRename(id, name),
    patch: (qc, { id, name }) =>
      qc.setQueryData<RadioList[]>(radioKeys.lists(), (rows) =>
        rows?.map((r) => (r.id === id ? { ...r, name } : r))
      ),
    also: ({ id }) => [radioKeys.list(id)],
  });
}

export function useDeleteList() {
  return useOptimisticRadio<number, RadioList[]>({
    title: 'Could not delete list',
    key: radioKeys.lists,
    mutationFn: (id) => tauriAPI.radio.listDelete(id),
    patch: (qc, id) =>
      qc.setQueryData<RadioList[]>(radioKeys.lists(), (rows) => rows?.filter((r) => r.id !== id)),
  });
}

export function useRemoveFromList() {
  return useOptimisticRadio<{ id: number; position: number }, RadioListDetail | null>({
    title: 'Could not remove from list',
    key: ({ id }) => radioKeys.list(id),
    mutationFn: ({ id, position }) => tauriAPI.radio.listRemove(id, position),
    patch: (qc, { id, position }) =>
      patchListStations(qc, id, (rows) => rows.filter((_, i) => i !== position)),
    also: () => [radioKeys.lists()],
  });
}

/**
 * Applies the move locally first — a drag that snapped back until the write
 * returned would be unusable.
 */
export function useReorderList() {
  return useOptimisticRadio<{ id: number; from: number; to: number }, RadioListDetail | null>({
    title: 'Could not reorder list',
    key: ({ id }) => radioKeys.list(id),
    mutationFn: ({ id, from, to }) => tauriAPI.radio.listReorder(id, from, to),
    patch: (qc, { id, from, to }) =>
      patchListStations(qc, id, (rows) => {
        const next = [...rows];
        const [moved] = next.splice(from, 1);
        if (moved) next.splice(to, 0, moved);
        return next;
      }),
  });
}

/** Drops one of the user's own stations, and with it every list entry for it. */
export function useForgetStation() {
  return usePlainRadio<string, void>({
    title: 'Could not remove station',
    mutationFn: (key) => tauriAPI.radio.forget(key),
    invalidates: () => [radioKeys.all],
  });
}

export function useCreateList() {
  return usePlainRadio({
    title: 'Could not create list',
    mutationFn: (name: string) => tauriAPI.radio.listCreate(name),
    invalidates: () => [radioKeys.lists()],
  });
}

export function useAddToList() {
  const notify = useNotificationStore((s) => s.addNotification);
  return usePlainRadio({
    title: 'Could not add to list',
    mutationFn: ({ id, keys }: { id: number; keys: string[]; listName?: string }) =>
      tauriAPI.radio.listAdd(id, keys),
    invalidates: ({ id }) => [radioKeys.lists(), radioKeys.list(id)],
    onSuccess: (_d, vars) => {
      if (!vars.listName) return;
      notify({
        type: 'success',
        title: 'Added to list',
        message: `${vars.keys.length} station${vars.keys.length === 1 ? '' : 's'} → ${vars.listName}`,
      });
    },
  });
}

export function useSaveStations() {
  const notify = useNotificationStore((s) => s.addNotification);
  return usePlainRadio({
    title: 'Could not save stations',
    mutationFn: (stations: RadioStation[]) => tauriAPI.radio.saveStations(stations),
    invalidates: () => [radioKeys.all],
    onSuccess: (_d, stations) => {
      notify({
        type: 'success',
        title: stations.length === 1 ? 'Station added' : `${stations.length} stations added`,
        message: 'On Home under “Your stations”, or filter Browse by source.',
      });
    },
  });
}

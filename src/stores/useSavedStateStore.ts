import { create } from 'zustand';
import type { SaveKindStr, SavedStateChangedPayload } from '@/tauri-bridge';

type Key = `${string}:${SaveKindStr}`;

const keyOf = (platform: string, kind: SaveKindStr): Key => `${platform}:${kind}` as Key;

interface SavedStateState {
  saved: Map<Key, Set<string>>;
  hydrated: Map<Key, boolean>;
  inflight: Map<Key, Promise<void>>;

  isSaved: (platform: string, kind: SaveKindStr, id: string) => boolean;
  hasHydrated: (platform: string, kind: SaveKindStr) => boolean;
  hydrate: (platform: string, kind: SaveKindStr, ids: string[]) => void;
  localToggle: (platform: string, kind: SaveKindStr, id: string, save: boolean) => void;
  applyServerEvent: (e: SavedStateChangedPayload) => void;
  setInflight: (platform: string, kind: SaveKindStr, p: Promise<void> | null) => void;
  getInflight: (platform: string, kind: SaveKindStr) => Promise<void> | undefined;
  reset: () => void;
}

export const useSavedStateStore = create<SavedStateState>((set, get) => ({
  saved: new Map(),
  hydrated: new Map(),
  inflight: new Map(),

  isSaved: (platform, kind, id) => {
    const set_ = get().saved.get(keyOf(platform, kind));
    return set_ ? set_.has(id) : false;
  },

  hasHydrated: (platform, kind) => !!get().hydrated.get(keyOf(platform, kind)),

  hydrate: (platform, kind, ids) =>
    set((s) => {
      const k = keyOf(platform, kind);
      const saved = new Map(s.saved);
      saved.set(k, new Set(ids));
      const hydrated = new Map(s.hydrated);
      hydrated.set(k, true);
      return { saved, hydrated };
    }),

  localToggle: (platform, kind, id, save) =>
    set((s) => {
      const k = keyOf(platform, kind);
      const cur = s.saved.get(k) ?? new Set<string>();
      const next = new Set(cur);
      if (save) next.add(id);
      else next.delete(id);
      const saved = new Map(s.saved);
      saved.set(k, next);
      return { saved };
    }),

  applyServerEvent: (e) =>
    set((s) => {
      const k = keyOf(e.platform, e.kind);
      const cur = s.saved.get(k) ?? new Set<string>();
      const next = new Set(cur);
      for (const id of e.added) next.add(id);
      for (const id of e.removed) next.delete(id);
      const saved = new Map(s.saved);
      saved.set(k, next);
      const hydrated = new Map(s.hydrated);
      hydrated.set(k, true);
      return { saved, hydrated };
    }),

  setInflight: (platform, kind, p) =>
    set((s) => {
      const k = keyOf(platform, kind);
      const inflight = new Map(s.inflight);
      if (p) inflight.set(k, p);
      else inflight.delete(k);
      return { inflight };
    }),

  getInflight: (platform, kind) => get().inflight.get(keyOf(platform, kind)),

  reset: () => set({ saved: new Map(), hydrated: new Map(), inflight: new Map() }),
}));

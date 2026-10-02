import { useQuery } from '@tanstack/react-query';
import { settingsKeys } from '@/lib/queryClient';
import type { Settings } from '@/types/settings';
import { tauriAPI } from '@/tauri-bridge';

export function useAppSettings() {
  return useQuery({
    queryKey: settingsKeys.all,
    queryFn: async () => ((await tauriAPI.settings.get()) ?? null) as Settings | null,
    staleTime: 30 * 1000,
    gcTime: 60 * 1000,
  });
}

/**
 * The settings that decide which download qualities exist at all — the wrapper
 * toggle is what makes ALAC appear or disappear. Reading these through the query
 * cache is what lets a change in Settings reach an already-mounted page: routes
 * are kept alive, so a one-shot fetch on mount would only refresh on reload.
 */
export function useQualityContext() {
  const { data } = useAppSettings();
  return {
    spotifyBackend: data?.spotify_downloader_backend || 'native',
    appleBackend: data?.apple_downloader_backend || 'native',
    appleWrapper: !!data?.apple_use_wrapper,
  };
}

/** Stable identity, so a settings payload without the field does not hand every
 *  consumer a fresh array on each render. */
const NO_SERVICES: string[] = [];

/** `null` while still loading. */
export function useEnabledServices(): string[] | null {
  const { data, isPending } = useAppSettings();
  if (isPending) return null;
  return data?.enabledServices ?? NO_SERVICES;
}

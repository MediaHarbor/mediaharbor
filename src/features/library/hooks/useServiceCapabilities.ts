import { useQuery } from '@tanstack/react-query';
import type { MutationCapabilities } from '@/tauri-bridge';
import { tauriAPI } from '@/tauri-bridge';

export interface ServiceCapabilitiesShape {
  albums: boolean;
  tracks: boolean;
  artists: boolean;
  playlists: boolean;
  videos: boolean;
  recommendations: boolean;
  followers: boolean;
  activity_feed: boolean;
  episode_bookmarks: boolean;
  mutations: MutationCapabilities;
}

const EMPTY: ServiceCapabilitiesShape = {
  albums: false,
  tracks: false,
  artists: false,
  playlists: false,
  videos: false,
  recommendations: false,
  followers: false,
  activity_feed: false,
  episode_bookmarks: false,
  mutations: {
    save_tracks: false,
    save_albums: false,
    follow_artists: false,
    follow_playlists: false,
    create_playlists: false,
    edit_playlists: false,
    reorder_playlists: false,
    radio: false,
  },
};

export function useServiceCapabilities(
  platform: string | null | undefined
): ServiceCapabilitiesShape {
  const enabled = !!platform && platform !== 'local';
  const q = useQuery({
    queryKey: ['service-library', 'capabilities', platform ?? ''],
    enabled,
    staleTime: 30 * 1000,
    queryFn: async () => {
      const bridge = tauriAPI.serviceLibrary;
      if (!bridge) return EMPTY;
      try {
        return (await bridge.capabilities(platform!)) as ServiceCapabilitiesShape;
      } catch {
        return EMPTY;
      }
    },
  });
  return q.data ?? EMPTY;
}

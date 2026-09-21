import { useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useSavedStateStore } from '@/stores/useSavedStateStore';
import { libraryKeys } from '@/features/library/api';
import { tauriAPI } from '@/tauri-bridge';

export function useSavedStateSyncBootstrap() {
  const applyServerEvent = useSavedStateStore((s) => s.applyServerEvent);
  const qc = useQueryClient();

  useEffect(() => {
    const bridge = tauriAPI.serviceLibrary;
    if (!bridge) return;

    const offSaved = bridge.onSavedStateChanged?.(
      (e: {
        platform: string;
        kind: 'track' | 'album' | 'artist' | 'playlist';
        added: string[];
        removed: string[];
      }) => {
        applyServerEvent(e);
        const kindKey =
          e.kind === 'album'
            ? 'albums'
            : e.kind === 'artist'
              ? 'artists'
              : e.kind === 'playlist'
                ? 'playlists'
                : 'tracks';
        qc.invalidateQueries({ queryKey: ['library', e.platform, kindKey] });
      }
    );

    const offPlaylist = bridge.onServicePlaylistChanged?.(
      (e: { platform: string; playlistId: string; change: string }) => {
        qc.invalidateQueries({
          queryKey: ['service-library', 'owned-playlists', e.platform],
        });
        qc.invalidateQueries({ queryKey: libraryKeys.playlists('', e.platform) });
        qc.invalidateQueries({
          queryKey: ['service-library', 'playlist', e.platform, e.playlistId],
        });
      }
    );

    return () => {
      try {
        offSaved?.();
      } catch {
        void 0;
      }
      try {
        offPlaylist?.();
      } catch {
        void 0;
      }
    };
  }, [applyServerEvent, qc]);
}

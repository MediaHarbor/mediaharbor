import { useMemo } from 'react';
import { hashString } from '@/utils/hash';
import { useLocation, useNavigate } from 'react-router-dom';
import { useLibraryStore, type LibraryView } from '@/stores/useLibraryStore';
import { usePlayerStore } from '@/stores/usePlayerStore';
import { serviceSourceOf } from '@/utils/platform-data';
import type { ServicePlatform } from '@/features/library/api';

export function toServiceSource(platform: string): ServicePlatform | null {
  return serviceSourceOf(platform) as ServicePlatform | null;
}

export interface MediaNavigation {
  goToAlbum: (platform: string, albumId: string) => void;
  goToArtist: (platform: string, artistId: string) => void;
  goToPlaylist: (
    platform: string,
    opts: { serviceId?: string | null; numericId?: number | null }
  ) => void;
}

export function useMediaNavigation(): MediaNavigation {
  const navigate = useNavigate();
  const location = useLocation();
  const setActiveSource = useLibraryStore((s) => s.setActiveSource);
  const pushView = useLibraryStore((s) => s.pushView);
  const clearViews = useLibraryStore((s) => s.clearViews);
  const setReturnPath = useLibraryStore((s) => s.setReturnPath);

  return useMemo<MediaNavigation>(() => {
    const go = (platform: string, view: LibraryView) => {
      const source = toServiceSource(platform);
      const fromLibrary = location.pathname === '/library';
      usePlayerStore.getState().closeNowPlaying();
      if (!fromLibrary) {
        clearViews();
        setReturnPath(location.pathname);
      }
      if (source) setActiveSource(source);
      pushView(view);
      if (!fromLibrary) navigate('/library');
    };
    return {
      goToAlbum: (platform, albumId) => {
        if (!albumId) return;
        go(platform, { kind: 'album', albumKey: albumId });
      },
      goToArtist: (platform, artistId) => {
        if (!artistId) return;
        go(platform, { kind: 'artist', artistKey: artistId });
      },
      goToPlaylist: (platform, { serviceId = null, numericId = null }) => {
        if (!serviceId && numericId == null) return;
        go(platform, {
          kind: 'playlist',
          id: numericId ?? (serviceId ? hashString(serviceId) : 0),
          serviceId,
        });
      },
    };
  }, [navigate, location.pathname, setActiveSource, pushView, clearViews, setReturnPath]);
}

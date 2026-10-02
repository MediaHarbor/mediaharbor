import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { errorMessage } from '@/utils/errors';
import { useNotificationStore } from '@/stores/useNotificationStore';
import {
  useLibraryStore,
  LIBRARY_TABS,
  SERVICE_LIBRARY_TABS,
  type LibraryTab,
} from '@/stores/useLibraryStore';
import {
  LibraryHeader,
  RecentlyAddedSection,
  ScanProgressBar,
  LibraryEmptyState,
  AlbumDetailView,
  AlbumsView,
  TracksView,
  ArtistsView,
  ArtistDetailView,
  PlaylistsView,
  PlaylistDetailView,
  ServiceHomeView,
  VideosView,
  FollowingView,
  ActivityFeedView,
  EpisodeBookmarksView,
  type SortType,
  type ViewType,
} from '@/features/library/components';
import {
  libraryKeys,
  ipcPlatformOf,
  type ServicePlatform,
  toPlayable,
} from '@/features/library/api';
import { useLibraryBack } from '@/features/library/hooks/useLibraryBack';
import { useServiceCapabilities } from '@/features/library/hooks/useServiceCapabilities';
import { useAppSettings } from '@/hooks/useAppSettings';
import { accentFor } from '@/features/library/serviceAccent';
import { isGatedService, normalizePlatform } from '@/utils/platform-data';
import { usePlayerStore } from '@/stores/usePlayerStore';
import type { MediaType, PlayableTrack } from '@/stores/usePlayerStore';
import { resolveCoverUrl } from '@/features/library/api';
import { tauriAPI } from '@/tauri-bridge';

const SORT_OPTIONS: Record<LibraryTab, { value: string; label: string }[]> = {
  home: [],
  tracks: [
    { value: 'recent', label: 'Recent' },
    { value: 'title', label: 'Title' },
    { value: 'artist', label: 'Artist' },
    { value: 'album', label: 'Album' },
    { value: 'duration', label: 'Duration' },
    { value: 'bitrate', label: 'Quality' },
    { value: 'year', label: 'Year' },
    { value: 'plays', label: 'Most played' },
  ],
  albums: [
    { value: 'recent', label: 'Recent' },
    { value: 'title', label: 'Title' },
    { value: 'artist', label: 'Artist' },
    { value: 'year', label: 'Year' },
  ],
  artists: [
    { value: 'name', label: 'Name' },
    { value: 'albums', label: 'Albums' },
    { value: 'tracks', label: 'Tracks' },
  ],
  playlists: [],
  videos: [
    { value: 'recent', label: 'Recent' },
    { value: 'name', label: 'Name' },
    { value: 'size', label: 'Size' },
    { value: 'duration', label: 'Duration' },
  ],
  following: [],
  activity: [],
  bookmarks: [],
};

const VIEW_TOGGLE_TABS: LibraryTab[] = ['albums', 'videos'];
const SORT_TABS: LibraryTab[] = ['tracks', 'albums', 'artists', 'videos'];

export default function LibraryPage() {
  const isScanning = useLibraryStore((s) => s.isScanning);
  const isRefreshing = useLibraryStore((s) => s.isRefreshing);
  const scanProgress = useLibraryStore((s) => s.scanProgress);
  const scanFile = useLibraryStore((s) => s.scanFile);
  const downloadDir = useLibraryStore((s) => s.downloadDir);
  const tab = useLibraryStore((s) => s.tab);
  const perTabSort = useLibraryStore((s) => s.perTabSort);
  const activeSource = useLibraryStore((s) => s.activeSource);
  const viewStack = useLibraryStore((s) => s.viewStack);
  const setIsScanning = useLibraryStore((s) => s.setIsScanning);
  const setIsRefreshing = useLibraryStore((s) => s.setIsRefreshing);
  const setScanProgress = useLibraryStore((s) => s.setScanProgress);
  const setScanFile = useLibraryStore((s) => s.setScanFile);
  const setLastScanned = useLibraryStore((s) => s.setLastScanned);
  const setDownloadDir = useLibraryStore((s) => s.setDownloadDir);
  const setTab = useLibraryStore((s) => s.setTab);
  const setSort = useLibraryStore((s) => s.setSort);
  const setActiveSource = useLibraryStore((s) => s.setActiveSource);
  const pushView = useLibraryStore((s) => s.pushView);
  const clearViews = useLibraryStore((s) => s.clearViews);

  const queryClient = useQueryClient();
  const setQueue = usePlayerStore((s) => s.setQueue);

  const currentView = viewStack[viewStack.length - 1] ?? null;

  const openAlbum = useCallback(
    (albumKey: string) => pushView({ kind: 'album', albumKey }),
    [pushView]
  );
  const openArtist = useCallback(
    (artistKey: string) => pushView({ kind: 'artist', artistKey }),
    [pushView]
  );
  const openPlaylist = useCallback(
    (id: number, serviceId?: string | null) =>
      serviceId?.startsWith('dir:')
        ? pushView({ kind: 'album', albumKey: serviceId })
        : pushView({ kind: 'playlist', id, serviceId: serviceId ?? null }),
    [pushView]
  );

  const { goBack } = useLibraryBack();
  const { data: settings } = useAppSettings();

  const [search, setSearch] = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [view, setView] = useState<ViewType>('grid');
  const isLocal = activeSource === 'local';
  const capabilities = useServiceCapabilities(isLocal ? null : activeSource);
  const serviceTabs = useMemo(
    () =>
      SERVICE_LIBRARY_TABS.filter((t) => {
        if (t === 'following') return capabilities.followers;
        if (t === 'activity') return capabilities.activity_feed;
        if (t === 'bookmarks') return capabilities.episode_bookmarks;
        return true;
      }),
    [capabilities]
  );

  const scanCleanupRef = useRef<(() => void) | null>(null);
  const fileChangeRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    const t = setTimeout(() => setDebouncedSearch(search), 200);
    return () => clearTimeout(t);
  }, [search]);

  const invalidateLibrary = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: libraryKeys.all });
  }, [queryClient]);

  const runScan = useCallback(
    async (force: boolean) => {
      if (!downloadDir) return;
      if (isScanning) return;
      setIsScanning(true);
      setScanProgress(0);
      setScanFile('');

      const cleanup = tauriAPI.library.onScanProgress((data) => {
        setScanProgress(data.progress ?? 0);
        setScanFile(data.currentFile ?? '');
      });
      scanCleanupRef.current = cleanup;

      try {
        await tauriAPI.library.scanIncremental?.(downloadDir, force);
        setLastScanned(Date.now());
        invalidateLibrary();
      } catch (err) {
        useNotificationStore
          .getState()
          .addNotification({ type: 'error', title: 'Scan Failed', message: errorMessage(err) });
      } finally {
        cleanup();
        scanCleanupRef.current = null;
        setIsScanning(false);
        setIsRefreshing(false);
        setScanProgress(100);
      }
    },
    [
      downloadDir,
      isScanning,
      invalidateLibrary,
      setIsScanning,
      setIsRefreshing,
      setScanProgress,
      setScanFile,
      setLastScanned,
    ]
  );

  useEffect(() => {
    if (!settings) return;
    const next = settings.downloadLocation ?? '';
    if (next && next !== useLibraryStore.getState().downloadDir) {
      setDownloadDir(next);
    }
    const enabled = (settings.enabledServices ?? []).map((p) => normalizePlatform(p));
    const src = useLibraryStore.getState().activeSource;
    if (isGatedService(src) && !enabled.includes(normalizePlatform(src))) {
      clearViews();
      setActiveSource('local');
      if (useLibraryStore.getState().tab === 'home') setTab('albums');
    }
  }, [settings, setDownloadDir, setActiveSource, clearViews, setTab]);

  useEffect(() => {
    if (isLocal && !LIBRARY_TABS.includes(tab)) setTab('albums');
  }, [isLocal, tab, setTab]);

  useEffect(() => {
    if (!downloadDir) return;
    tauriAPI.library.setWatch?.([downloadDir]).catch(() => {});
  }, [downloadDir]);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const cleanup = tauriAPI.library.onFilesChanged((e) => {
      const dir = (e as { directory?: string }).directory;
      if (dir === 'playlists') {
        invalidateLibrary();
        return;
      }
      if (timer) return;
      timer = setTimeout(() => {
        timer = null;
        invalidateLibrary();
      }, 2_000);
    });
    fileChangeRef.current = cleanup;
    return () => {
      cleanup();
      fileChangeRef.current = null;
    };
  }, [invalidateLibrary]);

  useEffect(() => {
    return () => {
      scanCleanupRef.current?.();
      fileChangeRef.current?.();
    };
  }, []);

  const openInFolder = (filePath: string) => {
    tauriAPI.library.showItemInFolder(filePath);
  };

  const playAlbumByKey = useCallback(
    async (album_key: string, source: ServicePlatform = 'local', startIndex = 0) => {
      const detail =
        source === 'local'
          ? await tauriAPI.library.getAlbum?.(album_key)
          : await tauriAPI.serviceLibrary?.album?.(source, album_key);
      if (!detail) return;
      const tracks =
        (detail.tracks as Array<{
          path: string;
          title?: string | null;
          cover_id?: string | null;
          artist_id?: string | null;
          is_video?: boolean;
        }>) ?? [];
      const albumMeta = detail.album as {
        cover_id?: string | null;
        artist?: string;
        title?: string | null;
        artist_id?: string | null;
      };
      const albumThumb = albumMeta.cover_id ? await resolveCoverUrl(albumMeta.cover_id) : null;
      const artist = albumMeta.artist ?? '';
      const platform = source === 'local' ? 'local' : ipcPlatformOf(source);
      const playable: PlayableTrack[] = tracks.map((t) =>
        toPlayable(t, platform, undefined, {
          artist,
          album: albumMeta.title ?? null,
          thumbnail: albumThumb ?? undefined,
          albumId: album_key,
          artistId: t.artist_id ?? albumMeta.artist_id ?? null,
        })
      );
      if (playable.length === 0) return;
      setQueue(playable, Math.min(startIndex, playable.length - 1));
    },
    [setQueue]
  );

  if (currentView?.kind === 'album') {
    return (
      <div
        className="h-full min-h-0 overflow-y-auto overflow-x-hidden"
        style={{ ['--service-accent' as 'color']: accentFor(activeSource) }}
      >
        <div className="p-6 max-w-[1200px] mx-auto">
          <AlbumDetailView
            key={currentView.albumKey}
            albumKey={currentView.albumKey}
            source={activeSource}
            onBack={goBack}
            onOpen={openInFolder}
          />
        </div>
      </div>
    );
  }

  if (currentView?.kind === 'artist') {
    return (
      <div
        className="h-full min-h-0 overflow-y-auto overflow-x-hidden"
        style={{ ['--service-accent' as 'color']: accentFor(activeSource) }}
      >
        <div className="p-6 max-w-[1200px] mx-auto">
          <ArtistDetailView
            key={currentView.artistKey}
            artistKey={currentView.artistKey}
            source={activeSource}
            onBack={goBack}
            onSelectAlbum={openAlbum}
            onSelectArtist={openArtist}
            onSelectPlaylist={openPlaylist}
          />
        </div>
      </div>
    );
  }

  if (currentView?.kind === 'playlist') {
    return (
      <div
        className="h-full min-h-0 flex flex-col overflow-hidden"
        style={{ ['--service-accent' as 'color']: accentFor(activeSource) }}
      >
        <div className="flex-1 min-h-0 w-full max-w-[1200px] mx-auto flex flex-col p-6">
          <PlaylistDetailView
            key={`${currentView.id}:${currentView.serviceId ?? ''}`}
            playlistId={currentView.id}
            source={activeSource}
            serviceId={currentView.serviceId}
            onBack={goBack}
          />
        </div>
      </div>
    );
  }

  const showViewToggle = VIEW_TOGGLE_TABS.includes(tab);
  const showSort = SORT_TABS.includes(tab);
  const sort: SortType = perTabSort[tab] ?? 'recent';

  return (
    <div
      className="h-full min-h-0 flex flex-col overflow-hidden"
      style={{ ['--service-accent' as 'color']: accentFor(activeSource) }}
    >
      <div className="flex-1 min-h-0 min-w-0 flex flex-col p-6 gap-4 max-w-[1600px] w-full mx-auto">
        <LibraryHeader
          search={search}
          onSearchChange={setSearch}
          sort={sort}
          onSortChange={(v) => setSort(tab, v)}
          sortOptions={SORT_OPTIONS[tab]}
          view={view}
          onViewChange={setView}
          showViewToggle={showViewToggle}
          showSort={showSort}
          albumCount={0}
          trackCount={0}
          videoCount={0}
          isScanning={isScanning}
          isRefreshing={isRefreshing}
          canScan={!!downloadDir}
          onRescan={() => runScan(false)}
          activeSource={activeSource}
          onSourceChange={(v) => {
            const next = v as ServicePlatform;
            clearViews();
            setActiveSource(next);
            if (next === 'local' && tab === 'home') setTab('albums');
            else if (next !== 'local' && tab !== 'home') setTab('home');
          }}
          tab={tab}
          serviceMode={!isLocal}
          tabs={isLocal ? undefined : serviceTabs}
          onTabChange={(t) => {
            clearViews();
            setTab(t);
          }}
        />

        <div className="flex-1 min-h-0 min-w-0 flex flex-col">
          {!isLocal && tab === 'home' && (
            <div className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden">
              <ServiceHomeView
                source={activeSource as Exclude<typeof activeSource, 'local'>}
                onSelectAlbum={openAlbum}
                onSelectArtist={openArtist}
                onSelectPlaylist={(id, serviceId) => {
                  openPlaylist(id, serviceId);
                }}
              />
            </div>
          )}

          {isLocal && isScanning && (
            <ScanProgressBar progress={scanProgress} currentFile={scanFile} />
          )}

          {isLocal && !downloadDir && <LibraryEmptyState variant="no-folder" />}

          {(isLocal ? downloadDir : true) && tab === 'albums' && (
            <div className="flex-1 min-h-0 flex flex-col gap-6">
              {isLocal && !debouncedSearch && (
                <RecentlyAddedSection
                  kind="albums"
                  onSelectAlbum={openAlbum}
                  onPlayAlbum={(k) => playAlbumByKey(k)}
                />
              )}
              <div className="flex-1 min-h-0">
                <AlbumsView
                  view={view}
                  search={debouncedSearch}
                  sort={sort}
                  source={activeSource}
                  onSelectAlbum={openAlbum}
                  onPlayAlbum={(k) => playAlbumByKey(k, activeSource)}
                />
              </div>
            </div>
          )}

          {(isLocal ? downloadDir : true) && tab === 'tracks' && (
            <div className="flex-1 min-h-0">
              <TracksView search={debouncedSearch} sort={sort} source={activeSource} />
            </div>
          )}

          {(isLocal ? downloadDir : true) && tab === 'artists' && (
            <div className="flex-1 min-h-0">
              <ArtistsView
                search={debouncedSearch}
                sort={sort}
                source={activeSource}
                onSelectArtist={openArtist}
              />
            </div>
          )}

          {tab === 'playlists' && (
            <div className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden">
              <PlaylistsView
                search={debouncedSearch}
                source={activeSource}
                onSelectPlaylist={(id, serviceId) => {
                  openPlaylist(id, serviceId);
                }}
              />
            </div>
          )}

          {!isLocal && tab === 'following' && (
            <div className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden">
              <FollowingView source={activeSource} />
            </div>
          )}

          {!isLocal && tab === 'activity' && (
            <div className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden">
              <ActivityFeedView source={activeSource} onSelectAlbum={openAlbum} />
            </div>
          )}

          {!isLocal && tab === 'bookmarks' && (
            <div className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden">
              <EpisodeBookmarksView source={activeSource} />
            </div>
          )}

          {(isLocal ? downloadDir : true) && tab === 'videos' && (
            <div className="flex-1 min-h-0 flex flex-col gap-6">
              {isLocal && !debouncedSearch && (
                <RecentlyAddedSection
                  kind="videos"
                  onPlayVideo={(t) =>
                    setQueue(
                      [
                        {
                          url: t.path,
                          title: t.title ?? '',
                          artist: '',
                          mediaType: 'video' as MediaType,
                          platform: 'local',
                        },
                      ],
                      0
                    )
                  }
                />
              )}
              <div className="flex-1 min-h-0">
                <VideosView
                  view={view}
                  search={debouncedSearch}
                  sort={sort}
                  source={activeSource}
                />
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

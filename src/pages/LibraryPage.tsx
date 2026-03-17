import { useEffect, useCallback, useRef, useState, useMemo } from 'react';
import { useNotificationStore } from '@/stores/useNotificationStore';
import { usePlayerStore } from '@/components/Player';
import type { MediaType, PlayableTrack } from '@/components/Player';
import {
  useLibraryStore,
  type AlbumItem,
  type VideoItem,
  type MediaItem,
} from '@/stores/useLibraryStore';
import {
  LibraryHeader,
  RecentlyAddedSection,
  ScanProgressBar,
  LibraryEmptyState,
  PlatformPlaceholder,
  AlbumDetailView,
  ArtistGroupView,
  VideoCard,
  type FilterType,
  type SortType,
  type ViewType,
} from '@/features/library/components';

function thumbnailSrc(thumb?: { data: string; format: string }): string | null {
  if (!thumb) return null;
  return `data:${thumb.format};base64,${thumb.data}`;
}

export default function LibraryPage() {
  const items           = useLibraryStore((s) => s.items);
  const isScanning      = useLibraryStore((s) => s.isScanning);
  const isRefreshing    = useLibraryStore((s) => s.isRefreshing);
  const scanProgress    = useLibraryStore((s) => s.scanProgress);
  const scanFile        = useLibraryStore((s) => s.scanFile);
  const lastScanned     = useLibraryStore((s) => s.lastScanned);
  const downloadDir     = useLibraryStore((s) => s.downloadDir);
  const setItems        = useLibraryStore((s) => s.setItems);
  const addItems        = useLibraryStore((s) => s.addItems);
  const removeItemsByPaths = useLibraryStore((s) => s.removeItemsByPaths);
  const setIsScanning   = useLibraryStore((s) => s.setIsScanning);
  const setIsRefreshing = useLibraryStore((s) => s.setIsRefreshing);
  const setScanProgress = useLibraryStore((s) => s.setScanProgress);
  const setScanFile     = useLibraryStore((s) => s.setScanFile);
  const setLastScanned  = useLibraryStore((s) => s.setLastScanned);
  const setDownloadDir  = useLibraryStore((s) => s.setDownloadDir);

  const [filter, setFilter]       = useState<FilterType>('music');
  const [sort, setSort]           = useState<SortType>('name');
  const [search, setSearch]       = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [view, setView]           = useState<ViewType>('grid');
  const [activeSource, setActiveSource] = useState('local');
  const [selectedAlbum, setSelectedAlbum] = useState<AlbumItem | null>(null);

  const cleanupRef     = useRef<(() => void) | null>(null);
  const fileChangeRef  = useRef<(() => void) | null>(null);
  const setTrack       = usePlayerStore((s) => s.setTrack);
  const setQueue       = usePlayerStore((s) => s.setQueue);

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 150);
    return () => clearTimeout(timer);
  }, [search]);

  const BACKGROUND_SCAN_INTERVAL = 10 * 60 * 1000; // 10 minutes

  const runScan = useCallback(
    async (force: boolean, background = false) => {
      if (!downloadDir || !window.electron) return;
      if (isScanning) return;

      if (background) {
        setIsRefreshing(true);
      } else {
        setIsScanning(true);
        setScanProgress(0);
        setScanFile('');
      }

      const cleanup = window.electron.library.onScanProgress((data) => {
        if (!background) {
          setScanProgress(data.progress ?? 0);
          setScanFile(data.currentFile ?? '');
        }
      });
      cleanupRef.current = cleanup;

      try {
        const result = await window.electron.library.scan(downloadDir, force);
        if (Array.isArray(result)) {
          setItems(result as MediaItem[]);
          setLastScanned(Date.now());
        }
      } catch (err) {
        useNotificationStore.getState().addNotification({ type: 'error', title: 'Scan Failed', message: String(err) });
      } finally {
        cleanup();
        cleanupRef.current = null;
        setIsScanning(false);
        setIsRefreshing(false);
        setScanProgress(100);
      }
    },
    [downloadDir, isScanning, setIsScanning, setIsRefreshing, setScanProgress, setScanFile, setItems, setLastScanned]
  );

  useEffect(() => {
    if (downloadDir) return;
    window.electron?.settings.get().then((s) => {
      if (s?.downloadLocation) setDownloadDir(s.downloadLocation);
    });
  }, [downloadDir, setDownloadDir]);

  const didInitialScan = useRef(false);
  useEffect(() => {
    if (!downloadDir || didInitialScan.current) return;
    didInitialScan.current = true;
    if (items.length > 0) {
      runScan(false, true);
    } else {
      runScan(false).then(() => {
        setTimeout(() => runScan(true, true), 2000);
      });
    }
  }, [downloadDir]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!downloadDir) return;
    const interval = setInterval(() => {
      if (!isScanning) runScan(false, true);
    }, BACKGROUND_SCAN_INTERVAL);
    return () => clearInterval(interval);
  }, [downloadDir]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const onFocus= () => {
      if (!downloadDir || isScanning) return;
      const stale = !lastScanned || Date.now() - lastScanned > 5 * 60_000;
      if (stale) runScan(false, true);
    };
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [downloadDir]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!window.electron?.library.onFilesChanged) return;

    const cleanup = window.electron.library.onFilesChanged((data) => {
      if (data.added?.length) addItems(data.added as MediaItem[]);
      if (data.removedPaths?.length) removeItemsByPaths(data.removedPaths);
    });
    fileChangeRef.current = cleanup;

    return () => {
      cleanup();
      fileChangeRef.current = null;
    };
  }, [addItems, removeItemsByPaths]);

  useEffect(() => {
    return () => {
      cleanupRef.current?.();
      fileChangeRef.current?.();
    };
  }, []);

  const openInFolder = (filePath: string) => {
    window.electron?.library.showItemInFolder(filePath);
  };

  const handlePlay = (item: AlbumItem | VideoItem, trackIndex = 0) => {
    if (item.type === 'video') {
      setTrack({
        streamUrl: item.path,
        title: item.title,
        artist: '',
        thumbnail: thumbnailSrc(item.thumbnail) ?? undefined,
        mediaType: 'video' as MediaType,
      });
    } else {
      const album = item as AlbumItem;
      if (!album.tracks.length) return;
      const playable: PlayableTrack[] = album.tracks.map((t) => ({
        url: t.path,
        title: t.title,
        artist: album.artist,
        thumbnail: thumbnailSrc(album.thumbnail) ?? undefined,
        mediaType: 'audio' as MediaType,
      }));
      setQueue(playable, Math.min(trackIndex, playable.length - 1));
    }
  };

  const handleSelectAlbum = (album: AlbumItem) => {
    setSelectedAlbum(album);
  };

  const musicItems = useMemo(
    () => items.filter((i) => i.type === 'music') as AlbumItem[],
    [items]
  );

  const filteredVideos = useMemo(() => {
    let videos = items.filter((i) => i.type === 'video') as VideoItem[];

    if (debouncedSearch.trim()) {
      const q = debouncedSearch.toLowerCase();
      videos = videos.filter((v) => v.title.toLowerCase().includes(q));
    }

    return videos.sort((a, b) => {
      if (sort === 'name') return a.title.localeCompare(b.title);
      if (sort === 'date') return (b.date || '').localeCompare(a.date || '');
      if (sort === 'size') return parseFloat(b.size) - parseFloat(a.size);
      return 0;
    });
  }, [items, debouncedSearch, sort]);

  const recentMusic = useMemo(() => {
    return [...musicItems].sort((a, b) => {
      const dateA = a.tracks[0]?.date ?? '';
      const dateB = b.tracks[0]?.date ?? '';
      return dateB.localeCompare(dateA);
    });
  }, [musicItems]);

  const recentVideos = useMemo(() => {
    return [...(items.filter((i) => i.type === 'video') as VideoItem[])]
      .sort((a, b) => (b.date || '').localeCompare(a.date || ''));
  }, [items]);

  const albumCount = musicItems.length;
  const trackCount = useMemo(() => musicItems.reduce((acc, a) => acc + a.tracks.length, 0), [musicItems]);
  const videoCount = useMemo(() => items.filter((i) => i.type === 'video').length, [items]);
  const hasContent = !isScanning && downloadDir && items.length > 0;
  const isSearching = debouncedSearch.trim().length > 0;

  const searchedMusicCount = useMemo(() => {
    if (!isSearching) return musicItems.length;
    const q = debouncedSearch.toLowerCase();
    return musicItems.filter(
      (a) => a.artist.toLowerCase().includes(q) || a.album.toLowerCase().includes(q)
    ).length;
  }, [musicItems, debouncedSearch, isSearching]);

  if (selectedAlbum) {
    return (
      <div className="flex-1 overflow-y-auto">
        <div className="p-6 max-w-[1200px] mx-auto">
          <AlbumDetailView
            album={selectedAlbum}
            onBack={() => setSelectedAlbum(null)}
            onPlay={handlePlay}
            onOpen={openInFolder}
          />
        </div>
      </div>
    );
  }

  const showBothSections = isSearching && searchedMusicCount > 0 && filteredVideos.length > 0;

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="p-6 space-y-4 max-w-[1600px] mx-auto">
        <LibraryHeader
          search={search}
          onSearchChange={setSearch}
          filter={filter}
          onFilterChange={setFilter}
          sort={sort}
          onSortChange={setSort}
          view={view}
          onViewChange={setView}
          albumCount={albumCount}
          trackCount={trackCount}
          videoCount={videoCount}
          isScanning={isScanning}
          isRefreshing={isRefreshing}
          canScan={!!downloadDir}
          onRescan={() => runScan(true)}
          activeSource={activeSource}
          onSourceChange={setActiveSource}
        />

        {activeSource !== 'local' ? (
          <PlatformPlaceholder platformId={activeSource} />
        ) : (
          <>
            {isScanning && (
              <ScanProgressBar progress={scanProgress} currentFile={scanFile} />
            )}

            {!downloadDir && <LibraryEmptyState variant="no-folder" />}

            {!isScanning && downloadDir && items.length === 0 && (
              <LibraryEmptyState variant="empty" />
            )}

            {hasContent && isSearching && (
              <>
                {searchedMusicCount === 0 && filteredVideos.length === 0 && (
                  <LibraryEmptyState variant="no-results" />
                )}

                {searchedMusicCount > 0 && (
                  <div>
                    {showBothSections && (
                      <h3 className="text-sm font-medium text-muted-foreground/50 mb-2">Music</h3>
                    )}
                    <ArtistGroupView
                      albums={musicItems}
                      view={view}
                      onSelectAlbum={handleSelectAlbum}
                      onPlay={handlePlay}
                      onOpen={openInFolder}
                      search={debouncedSearch}
                    />
                  </div>
                )}

                {filteredVideos.length > 0 && (
                  <div>
                    {showBothSections && (
                      <h3 className="text-sm font-medium text-muted-foreground/50 mb-2">Videos</h3>
                    )}
                    {view === 'grid' ? (
                      <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-1">
                        {filteredVideos.map((item, i) => (
                          <VideoCard
                            key={`video-${item.path}`}
                            item={item}
                            view="grid"
                            onPlay={handlePlay}
                            onOpen={openInFolder}
                            index={i}
                          />
                        ))}
                      </div>
                    ) : (
                      <div className="flex flex-col gap-0.5">
                        {filteredVideos.map((item, i) => (
                          <VideoCard
                            key={`video-${item.path}`}
                            item={item}
                            view="list"
                            onPlay={handlePlay}
                            onOpen={openInFolder}
                            index={i}
                          />
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </>
            )}

            {hasContent && !isSearching && filter === 'music' && (
              <>
                {recentMusic.length > 0 && (
                  <RecentlyAddedSection
                    items={recentMusic}
                    onPlay={handlePlay}
                    onSelect={handleSelectAlbum}
                  />
                )}

                {musicItems.length === 0 ? (
                  <LibraryEmptyState variant="no-results" />
                ) : (
                  <ArtistGroupView
                    albums={musicItems}
                    view={view}
                    onSelectAlbum={handleSelectAlbum}
                    onPlay={handlePlay}
                    onOpen={openInFolder}
                    search=""
                  />
                )}
              </>
            )}

            {hasContent && !isSearching && filter === 'video' && (
              <>
                {recentVideos.length > 0 && (
                  <RecentlyAddedSection
                    items={recentVideos}
                    onPlay={handlePlay}
                  />
                )}

                {filteredVideos.length === 0 ? (
                  <LibraryEmptyState variant="no-results" />
                ) : view === 'grid' ? (
                  <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-1">
                    {filteredVideos.map((item, i) => (
                      <VideoCard
                        key={`video-${item.path}`}
                        item={item}
                        view="grid"
                        onPlay={handlePlay}
                        onOpen={openInFolder}
                        index={i}
                      />
                    ))}
                  </div>
                ) : (
                  <div className="flex flex-col gap-0.5">
                    {filteredVideos.map((item, i) => (
                      <VideoCard
                        key={`video-${item.path}`}
                        item={item}
                        view="list"
                        onPlay={handlePlay}
                        onOpen={openInFolder}
                        index={i}
                      />
                    ))}
                  </div>
                )}
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}

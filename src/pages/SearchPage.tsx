import { useState, useRef, useCallback, lazy, Suspense } from 'react';
import { Search, Mic2, Disc3, User, ListEnd, Loader2 } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { ResultsGrid } from '@/features/search/components/ResultsGrid';
import type { TrackInfo } from '@/features/search/components/DetailsModal';
const DetailsModal = lazy(() => import('@/features/search/components/DetailsModal'));
import { SearchArtistView, type AlbumInfo } from '@/features/search/components/SearchArtistView';
import { SearchAlbumView, type RemoteTrack } from '@/features/search/components/SearchAlbumView';
import { QualitySelector } from '@/components/QualitySelector';
import { useSearch } from '@/features/search/hooks/useSearch';
import { useSearchStore } from '@/features/search/stores/searchStore';
import { useNotificationStore } from '@/stores/useNotificationStore';
import { downloadService } from '@/services/ipc/downloads';
import { usePlayerStore, type PlayableTrack } from '@/stores/usePlayerStore';
import { useMusicSuggestions } from '@/hooks/useMusicSuggestions';
import { searchService } from '@/services/ipc/search';
import { logInfo, logError } from '@/utils/logger';
import { cn } from '@/utils/cn';
import { PlatformIcon } from '@/utils/platforms';
import { PLATFORM_COLORS, PLATFORM_LIST } from '@/utils/platform-data';
import { SEARCH_TYPE_LABELS, PLATFORM_IPC_NAME } from '@/utils/constants';
import type { Platform, SearchType, SearchResult } from '@/types';

interface ViewingArtist {
  id: string;
  name: string;
  thumbnail?: string;
  followerCount?: number;
  genre?: string;
  albums: AlbumInfo[];
}

interface ViewingAlbum {
  id: string;
  title: string;
  thumbnail?: string;
  artist?: string;
  releaseDate?: string;
  fromArtist: boolean;
}

interface PlayableSource {
  url?: string;
  external_urls?: { spotify?: string };
  uri?: string;
  title?: string;
  name?: string;
  artist?: string;
  thumbnail?: string;
  resultType?: string;
}

interface DownloadableSource {
  url?: string;
  external_urls?: { spotify?: string };
  uri?: string;
  title?: string;
  name?: string;
  trackName?: string;
  artist?: string | { name?: string };
  artists?: Array<{ name: string }>;
  artistName?: string;
  uploader?: string;
  channel?: string;
  album?: { title?: string; name?: string; images?: Array<{ url: string }> };
  collectionName?: string;
  thumbnail?: string;
  artworkUrl100?: string;
}

interface SearchResultLike {
  id: string;
  url: string;
  title?: string;
  name?: string;
  thumbnail?: string;
  artist?: string;
  followerCount?: number;
  genre?: string;
}

export default function SearchPage() {
  const [query, setQuery] = useState('');
  const [searchQuery, setSearchQuery] = useState('');
  const [showSuggestions, setShowSuggestions] = useState(false);
  const [activeSuggestion, setActiveSuggestion] = useState(-1);
  const searchWrapperRef = useRef<HTMLDivElement>(null);
  const [detailsModal, setDetailsModal] = useState<{
    open: boolean;
    type: 'album' | 'playlist' | 'podcast';
    id: string;
    title: string;
  } | null>(null);
  const [qualityModal, setQualityModal] = useState<{
    open: boolean;
    url: string;
    title: string;
    artist?: string;
    album?: string;
    thumbnail?: string | null;
  } | null>(null);
  const [isLoadingArtist, setIsLoadingArtist] = useState(false);
  const [viewingArtist, setViewingArtist] = useState<ViewingArtist | null>(null);
  const [viewingAlbum, setViewingAlbum] = useState<ViewingAlbum | null>(null);

  const selectedPlatform = useSearchStore((state) => state.selectedPlatform);
  const setSelectedPlatform = useSearchStore((state) => state.setSelectedPlatform);
  const searchType = useSearchStore((state) => state.searchType);
  const setSearchType = useSearchStore((state) => state.setSearchType);
  const getAvailableTypes = useSearchStore((state) => state.getAvailableTypes);
  const addNotification = useNotificationStore((state) => state.addNotification);
  const setTrack = usePlayerStore((state) => state.setTrack);
  const setQueue = usePlayerStore((state) => state.setQueue);
  const insertNext = usePlayerStore((state) => state.insertNext);
  const appendToQueue = usePlayerStore((state) => state.appendToQueue);

  const { data: results, isLoading, error } = useSearch(searchQuery, searchQuery.length > 0);

  const toPlayableTrack = (r: SearchResult | TrackInfo): PlayableTrack => ({
    url: (r as PlayableSource).url || (r as PlayableSource).external_urls?.spotify || (r as PlayableSource).uri || '',
    title: (r as PlayableSource).title || (r as PlayableSource).name || '',
    artist: (r as PlayableSource).artist || '',
    thumbnail: (r as PlayableSource).thumbnail,
    platform: selectedPlatform,
    mediaType: ((r as PlayableSource).resultType === 'video' || (r as PlayableSource).resultType === 'musicvideo') ? 'video' as const : 'audio' as const,
  });

  const suggestions = useMusicSuggestions(showSuggestions ? query : '');

  const handleWrapperBlur = useCallback((e: React.FocusEvent) => {
    if (!searchWrapperRef.current?.contains(e.relatedTarget as Node)) {
      setShowSuggestions(false);
      setActiveSuggestion(-1);
    }
  }, []);

  const availableTypes = getAvailableTypes();
  const activePlatformColor = PLATFORM_COLORS[selectedPlatform];

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    if (query.trim()) {
      logInfo('search', 'Search submitted', `"${query.trim()}" on ${selectedPlatform} (${searchType})`);
      setSearchQuery(query.trim());
      setShowSuggestions(false);
      setActiveSuggestion(-1);
    }
  };

  const commitSuggestion = (text: string) => {
    setQuery(text);
    setSearchQuery(text);
    setShowSuggestions(false);
    setActiveSuggestion(-1);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!showSuggestions || !suggestions.length) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActiveSuggestion((i) => Math.min(i + 1, suggestions.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActiveSuggestion((i) => Math.max(i - 1, -1));
    } else if (e.key === 'Enter' && activeSuggestion >= 0) {
      e.preventDefault();
      commitSuggestion(suggestions[activeSuggestion].text);
    } else if (e.key === 'Escape') {
      setShowSuggestions(false);
      setActiveSuggestion(-1);
    }
  };

  const handleDownload = async (result: SearchResult) => {
    try {
      if (!window.electron) {
        addNotification({ type: 'error', title: 'Error', message: 'Electron API not available' });
        return;
      }

      const r = result as DownloadableSource;
      const url = r.url || r.external_urls?.spotify || r.uri;
      const title = (r.title || r.name || r.trackName) as string;
      const artist = (typeof r.artist === 'string' ? r.artist : r.artist?.name)
        || r.artists?.[0]?.name || r.artistName || r.uploader || r.channel;
      const album = r.album?.title || r.album?.name || r.collectionName;
      const thumbnail = r.thumbnail || r.album?.images?.[0]?.url || r.artworkUrl100?.replace('100x100', '640x640') || null;

      if (!url) {
        addNotification({ type: 'error', title: 'Error', message: 'No URL found for download' });
        return;
      }

      logInfo('download', 'Download requested', `"${title}" on ${selectedPlatform}`);
      setQualityModal({ open: true, url, title, artist, album, thumbnail });
    } catch (err) {
      addNotification({
        type: 'error',
        title: 'Download Failed',
        message: err instanceof Error ? err.message : 'Unknown error',
      });
    }
  };

  const handleQualityConfirm = async (quality: string) => {
    if (!qualityModal) return;

    try {
      logInfo('download', 'Download started', `"${qualityModal.title}" quality=${quality}`);
      await downloadService.startDownload({
        platform: selectedPlatform,
        url: qualityModal.url,
        quality,
        title: qualityModal.title,
        artist: qualityModal.artist,
        album: qualityModal.album,
        thumbnail: qualityModal.thumbnail,
      });

      addNotification({ type: 'success', title: 'Download Started', message: `Downloading ${qualityModal.title}...` });
    } catch (err) {
      addNotification({
        type: 'error',
        title: 'Download Failed',
        message: err instanceof Error ? err.message : 'Unknown error',
      });
    }
  };

  const handlePlay = async (result: SearchResult) => {
    if (searchType === 'album' || searchType === 'playlist' || searchType === 'podcast' || searchType === 'show' || searchType === 'audiobook') {
      openDetails(result);
      return;
    }

    try {
      if (!window.electron) {
        addNotification({ type: 'error', title: 'Error', message: 'Electron API not available' });
        return;
      }

      const currentResults = results || [];
      const clickedIndex = currentResults.findIndex((r) => r.id === result.id);
      const startIndex = clickedIndex >= 0 ? clickedIndex : 0;
      const playable: PlayableTrack[] = currentResults.map(toPlayableTrack).filter((t) => t.url);

      if (!playable.length) {
        addNotification({ type: 'error', title: 'Error', message: 'No URL found for playback' });
        return;
      }

      const title = (result as SearchResultLike).title || (result as SearchResultLike).name;
      logInfo('playback', 'Playing track', `"${title}" from ${selectedPlatform}`);
      setQueue(playable, startIndex);
      addNotification({ type: 'success', title: 'Now Playing', message: title });
    } catch (err) {
      logError('playback', 'Playback failed', err instanceof Error ? (err.stack || err.message) : String(err));
      addNotification({
        type: 'error',
        title: 'Playback Failed',
        message: err instanceof Error ? err.message : String(err),
      });
    }
  };

  const handlePlayNext = (result: SearchResult) => {
    const track = toPlayableTrack(result);
    if (!track.url) return;
    insertNext(track);
    addNotification({ type: 'success', title: 'Play next', message: (result as SearchResultLike).title || (result as SearchResultLike).name });
  };

  const handleAddAllToQueue = () => {
    const currentResults = results || [];
    const tracks: PlayableTrack[] = currentResults.map(toPlayableTrack).filter((t) => t.url);
    if (!tracks.length) return;
    appendToQueue(tracks);
    addNotification({ type: 'success', title: 'Added to queue', message: `${tracks.length} tracks added` });
  };

  const handlePlayAll = (tracks: TrackInfo[]) => {
    if (!tracks.length) return;
    const playable: PlayableTrack[] = tracks.map(toPlayableTrack);
    setQueue(playable, 0);
    addNotification({ type: 'success', title: 'Queue started', message: `${tracks.length} tracks` });
  };

  const openDetails = (result: SearchResult) => {
    const id = String(result.id ?? '');
    const title = ((result as SearchResultLike).title || (result as SearchResultLike).name) as string;
    if (searchType === 'album') setDetailsModal({ open: true, type: 'album', id, title });
    else if (searchType === 'audiobook') setDetailsModal({ open: true, type: 'album', id: `audiobook::${id}`, title });
    else if (searchType === 'playlist') setDetailsModal({ open: true, type: 'playlist', id, title });
    else if (searchType === 'show') setDetailsModal({ open: true, type: 'playlist', id: `show::${id}`, title });
    else if (searchType === 'podcast') setDetailsModal({ open: true, type: 'podcast', id: `podcast::${id}`, title });
    else if (searchType === 'channel') openArtistDetails(result);
  };

  const openArtistDetails = async (result: SearchResult) => {
    const r = result as SearchResultLike;
    const id = r.id;
    const name = (r.title || r.name) as string;

    logInfo('search', 'Fetching artist', `${name} on ${selectedPlatform}`);
    setIsLoadingArtist(true);
    try {
      const data = await searchService.getArtistDetails({ platform: selectedPlatform, artistId: id });
      setViewingArtist({
        id,
        name,
        thumbnail: r.thumbnail,
        followerCount: r.followerCount,
        genre: r.genre,
        albums: data.albums || [],
      });
    } catch (e) {
      logError('search', 'Failed to load artist', e instanceof Error ? (e.stack || e.message) : String(e));
      addNotification({
        type: 'error',
        title: 'Error',
        message: e instanceof Error ? e.message : 'Failed to load artist',
      });
    } finally {
      setIsLoadingArtist(false);
    }
  };

  const handleAlbumSelect = (album: AlbumInfo) => {
    setViewingAlbum({
      id: album.id,
      title: album.title,
      thumbnail: album.thumbnail,
      artist: viewingArtist?.name,
      releaseDate: album.releaseDate,
      fromArtist: viewingArtist !== null,
    });
  };

  const handleAlbumDownload = (albumUrl: string) => {
    setQualityModal({
      open: true,
      url: albumUrl,
      title: viewingAlbum?.title || '',
      artist: viewingAlbum?.artist,
      thumbnail: viewingAlbum?.thumbnail ?? null,
    });
  };

  const handleAlbumTracksPlay = (tracks: RemoteTrack[], index = 0) => {
    const playable: PlayableTrack[] = tracks
      .map((t) => ({
        url: t.url || '',
        title: t.title,
        artist: t.artist || '',
        thumbnail: viewingAlbum?.thumbnail,
        platform: selectedPlatform,
        mediaType: 'audio' as const,
      }))
      .filter((t) => t.url);

    if (!playable.length) {
      addNotification({ type: 'error', title: 'Error', message: 'No playable URLs found' });
      return;
    }

    logInfo('playback', 'Playing album', `"${viewingAlbum?.title || 'Unknown'}" (${playable.length} tracks)`);
    setQueue(playable, index);
    addNotification({ type: 'success', title: 'Now Playing', message: viewingAlbum?.title || '' });
  };

  if (viewingAlbum) {
    return (
      <div className="flex-1 overflow-y-auto px-8 py-6">
        <SearchAlbumView
          albumId={viewingAlbum.id}
          albumTitle={viewingAlbum.title}
          albumThumbnail={viewingAlbum.thumbnail}
          albumArtist={viewingAlbum.artist}
          albumReleaseDate={viewingAlbum.releaseDate}
          platform={selectedPlatform}
          backLabel={viewingAlbum.fromArtist ? viewingArtist?.name || 'Artist' : 'Search Results'}
          onBack={() => setViewingAlbum(null)}
          onPlay={handleAlbumTracksPlay}
          onDownload={handleAlbumDownload}
        />
      </div>
    );
  }

  if (viewingArtist) {
    return (
      <div className="flex-1 overflow-y-auto px-8 py-6">
        <SearchArtistView
          artistName={viewingArtist.name}
          artistThumbnail={viewingArtist.thumbnail}
          followerCount={viewingArtist.followerCount}
          genre={viewingArtist.genre}
          albums={viewingArtist.albums}
          onBack={() => setViewingArtist(null)}
          onAlbumSelect={handleAlbumSelect}
        />
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div
        className="px-8 pt-8 pb-6 space-y-4 border-b border-border transition-colors duration-300"
        style={activePlatformColor ? { borderBottomColor: `${activePlatformColor}66` } : undefined}
      >
        <div ref={searchWrapperRef} className="relative" onBlur={handleWrapperBlur}>
          <form onSubmit={handleSearch} className="flex gap-2">
            <Input
              type="text"
              placeholder="Search for music, videos, albums, playlists..."
              value={query}
              onChange={(e) => { setQuery(e.target.value); setShowSuggestions(true); setActiveSuggestion(-1); }}
              onFocus={() => { if (query.trim().length >= 2) setShowSuggestions(true); }}
              onKeyDown={handleKeyDown}
              autoComplete="off"
              className="flex-1 h-10 bg-muted/40 border-0 focus-visible:ring-1 focus-visible:ring-ring placeholder:text-muted-foreground/60"
            />
            <Button
              type="submit"
              disabled={!query.trim()}
              className="h-10 px-5 transition-colors duration-300"
              style={activePlatformColor ? { backgroundColor: activePlatformColor, color: '#fff', borderColor: activePlatformColor } : undefined}
            >
              Search
            </Button>
          </form>

          {showSuggestions && suggestions.length > 0 && (
            <div className="absolute z-50 left-0 right-16 mt-1 bg-card border border-border rounded-lg shadow-xl overflow-hidden">
              {suggestions.map((s, i) => {
                const Icon = s.type === 'artist' ? User : s.type === 'album' ? Disc3 : Mic2;
                return (
                  <button
                    key={`${s.type}:${s.text}`}
                    type="button"
                    tabIndex={0}
                    onMouseDown={(e) => { e.preventDefault(); commitSuggestion(s.text); }}
                    className={cn(
                      'flex w-full items-center gap-2.5 px-3 py-2 text-sm text-left transition-colors duration-75',
                      i === activeSuggestion
                        ? 'bg-accent text-accent-foreground'
                        : 'hover:bg-muted/60 text-foreground'
                    )}
                  >
                    <Icon className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                    <span className="truncate">{s.text}</span>
                    <span className="ml-auto text-[10px] text-muted-foreground capitalize shrink-0">{s.type}</span>
                  </button>
                );
              })}
            </div>
          )}
        </div>

        <div className="flex items-center gap-5">
          <div className="flex gap-1.5 flex-wrap">
            {PLATFORM_LIST.map((p) => {
              const isActive = selectedPlatform === p.value;
              const color = PLATFORM_COLORS[p.value];
              return (
                <button
                  key={p.value}
                  type="button"
                  onClick={() => { logInfo('search', 'Platform changed', p.value); setSelectedPlatform(p.value as Platform); }}
                  style={isActive ? { backgroundColor: color, color: '#fff' } : undefined}
                  className={cn(
                    'flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-medium transition-all duration-150',
                    isActive
                      ? 'shadow-sm'
                      : 'bg-muted text-muted-foreground hover:text-foreground hover:bg-muted/80'
                  )}
                >
                  <PlatformIcon platform={p.value} size={11} />
                  {p.label}
                </button>
              );
            })}
          </div>

          <div className="w-px h-4 bg-border shrink-0" />
          <div className="flex gap-1">
            {availableTypes.map((t) => (
              <button
                key={t}
                type="button"
                onClick={() => { logInfo('search', 'Search type changed', t); setSearchType(t as SearchType); }}
                className={cn(
                  'px-3 py-1 rounded-md text-xs font-medium transition-colors duration-150',
                  searchType === t
                    ? 'bg-accent text-accent-foreground'
                    : 'text-muted-foreground hover:text-foreground'
                )}
              >
                {SEARCH_TYPE_LABELS[t]}
              </button>
            ))}
          </div>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-8 py-6">
        {isLoadingArtist ? (
          <div className="flex justify-center py-16">
            <Loader2 className="h-6 w-6 animate-spin text-muted-foreground/40" />
          </div>
        ) : searchQuery ? (
          <>
            {!isLoading && !error && (results?.length ?? 0) > 0 && (searchType === 'track' || searchType === 'video') && (
              <div className="flex items-center justify-end mb-3">
                <button
                  type="button"
                  onClick={handleAddAllToQueue}
                  className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs text-muted-foreground hover:text-foreground hover:bg-muted/60 transition-colors duration-100"
                  title="Append all results to the end of the current queue"
                >
                  <ListEnd className="h-3.5 w-3.5" />
                  Add all to queue
                </button>
              </div>
            )}
            <ResultsGrid
              results={results || []}
              isLoading={isLoading}
              error={error}
              onDownload={handleDownload}
              onPlayTrack={handlePlay}
              onPlayNext={(searchType === 'track' || searchType === 'video') ? handlePlayNext : undefined}
              onResultClick={
                (searchType === 'album' || searchType === 'playlist' || searchType === 'podcast' || searchType === 'show' || searchType === 'audiobook') ? openDetails :
                (searchType === 'artist' || searchType === 'channel') ? openArtistDetails :
                undefined
              }
            />
          </>
        ) : (
          <div className="flex flex-col items-center justify-center h-full gap-3 text-muted-foreground select-none">
            <Search className="h-10 w-10 opacity-20" />
            <p className="text-sm">Search across 7 platforms</p>
          </div>
        )}
      </div>

      {qualityModal && (
        <QualitySelector
          open={qualityModal.open}
          onClose={() => setQualityModal(null)}
          platform={selectedPlatform}
          title={qualityModal.title}
          onConfirm={handleQualityConfirm}
        />
      )}
      {detailsModal && (
        <Suspense fallback={null}>
          <DetailsModal
            open={detailsModal.open}
            onClose={() => setDetailsModal(null)}
            type={detailsModal.type}
            id={detailsModal.id}
            platform={selectedPlatform}
            title={detailsModal.title}
            onDownload={(info) => {
              const url = typeof info === 'string' ? info : info.url;
              const title = typeof info === 'string' ? detailsModal.title : (info.title || detailsModal.title);
              const artist = typeof info === 'string' ? undefined : info.artist;
              const thumbnail = typeof info === 'string' ? undefined : info.thumbnail;
              setQualityModal({ open: true, url, title, artist, thumbnail });
            }}
            onPlay={(track: TrackInfo) => {
              setTrack({ streamUrl: '', title: track.title, artist: track.artist, thumbnail: track.thumbnail, mediaType: 'audio', platform: selectedPlatform });
              window.electron?.player.playMedia({ url: track.url, platform: PLATFORM_IPC_NAME[selectedPlatform] ?? selectedPlatform })
                .catch((err: Error) => addNotification({ type: 'error', title: 'Playback Failed', message: err?.message ?? 'Unknown error' }));
              addNotification({ type: 'success', title: 'Now Playing', message: track.title });
            }}
            onPlayAll={handlePlayAll}
          />
        </Suspense>
      )}
    </div>
  );
}
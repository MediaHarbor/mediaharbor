import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { errorDetail, errorMessage } from '@/utils/errors';
import { Search, Mic, Disc3, User, ListEnd, ArrowLeft } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { ResultsGrid } from '@/features/search/components/ResultsGrid';
import { SearchBrowse } from '@/features/search/components/SearchBrowse';
import { ServiceHomeView } from '@/features/library/components/ServiceHomeView';
import { QualitySelector } from '@/components/QualitySelector';
import { useSearch } from '@/features/search/hooks/useSearch';
import { useSearchStore } from '@/features/search/stores/searchStore';
import { useNotificationStore } from '@/stores/useNotificationStore';
import { downloadService } from '@/services/ipc/downloads';
import { usePlayerStore, type PlayableTrack } from '@/stores/usePlayerStore';
import { useMusicSuggestions } from '@/hooks/useMusicSuggestions';
import { useMediaNavigation } from '@/features/library/actions/useMediaNavigation';
import { logInfo, logError } from '@/utils/logger';
import { cn } from '@/utils/cn';
import { PlatformIcon } from '@/utils/platforms';
import {
  PLATFORM_COLORS,
  normalizePlatform,
  searchPlatformsFor,
  toClientPlatform,
  contextUriFor,
} from '@/utils/platform-data';
import { useEnabledServices } from '@/hooks/useAppSettings';
import { SEARCH_TYPE_LABELS } from '@/utils/constants';
import type { Platform, SearchType, SearchResult } from '@/types';
import { isBackendAvailable } from '@/tauri-bridge';

interface PlayableSource {
  url?: string;
  external_urls?: { spotify?: string };
  uri?: string;
  title?: string;
  name?: string;
  artist?: string;
  thumbnail?: string;
  resultType?: string;
  albumId?: string;
  artistId?: string;
  trackNumber?: number;
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
  const [qualityModal, setQualityModal] = useState<{
    open: boolean;
    url: string;
    title: string;
    artist?: string;
    album?: string;
    thumbnail?: string | null;
  } | null>(null);

  const enabledServices = useEnabledServices();
  const [browseCategory, setBrowseCategory] = useState<{ path: string; title: string } | null>(
    null
  );

  const nav = useMediaNavigation();
  const selectedPlatform = useSearchStore((state) => state.selectedPlatform);
  const setSelectedPlatform = useSearchStore((state) => state.setSelectedPlatform);
  const searchType = useSearchStore((state) => state.searchType);
  const setSearchType = useSearchStore((state) => state.setSearchType);
  const getAvailableTypes = useSearchStore((state) => state.getAvailableTypes);
  const addNotification = useNotificationStore((state) => state.addNotification);
  const setQueue = usePlayerStore((state) => state.setQueue);
  const insertNext = usePlayerStore((state) => state.insertNext);
  const appendToQueue = usePlayerStore((state) => state.appendToQueue);

  const platforms = useMemo(
    () => (enabledServices === null ? [] : searchPlatformsFor(enabledServices)),
    [enabledServices]
  );

  useEffect(() => {
    if (enabledServices === null || platforms.length === 0) return;
    if (!platforms.some((p) => p.value === selectedPlatform)) {
      setSelectedPlatform(platforms[0].value as Platform);
    }
  }, [enabledServices, platforms, selectedPlatform, setSelectedPlatform]);

  useEffect(() => {
    setBrowseCategory(null);
  }, [selectedPlatform]);

  const {
    data: results,
    isLoading,
    error,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useSearch(searchQuery, searchQuery.length > 0);

  const toPlayableTrack = (r: SearchResult): PlayableTrack => {
    const albumId = (r as PlayableSource).albumId;
    const contextAlbumId = toClientPlatform(selectedPlatform) === 'spotify' ? albumId : undefined;
    const trackNumber = (r as PlayableSource).trackNumber;
    return {
      url:
        (r as PlayableSource).url ||
        (r as PlayableSource).external_urls?.spotify ||
        (r as PlayableSource).uri ||
        '',
      title: (r as PlayableSource).title || (r as PlayableSource).name || '',
      artist: (r as PlayableSource).artist || '',
      thumbnail: (r as PlayableSource).thumbnail,
      platform: selectedPlatform,
      albumId: albumId ?? null,
      artistId: (r as PlayableSource).artistId ?? null,
      mediaType:
        (r as PlayableSource).resultType === 'video' ||
        (r as PlayableSource).resultType === 'musicvideo'
          ? ('video' as const)
          : ('audio' as const),
      contextUri: contextUriFor(selectedPlatform, 'album', contextAlbumId),
      trackIndex: contextAlbumId && trackNumber ? Math.max(0, trackNumber - 1) : null,
    };
  };

  const pendingQuery = useSearchStore((state) => state.pendingQuery);
  const consumePendingQuery = useSearchStore((state) => state.consumePendingQuery);
  useEffect(() => {
    if (pendingQuery) {
      const pending = consumePendingQuery();
      if (pending) {
        setQuery(pending);
        setSearchQuery(pending);
      }
    }
  }, [pendingQuery, consumePendingQuery]);

  const suggestions = useMusicSuggestions(showSuggestions ? query : '', selectedPlatform);

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
      logInfo(
        'search',
        'Search submitted',
        `"${query.trim()}" on ${selectedPlatform} (${searchType})`
      );
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
      if (!isBackendAvailable()) {
        addNotification({ type: 'error', title: 'Error', message: 'Backend not available' });
        return;
      }

      const r = result as DownloadableSource;
      const url = r.url || r.external_urls?.spotify || r.uri;
      const title = (r.title || r.name || r.trackName) as string;
      const artist =
        (typeof r.artist === 'string' ? r.artist : r.artist?.name) ||
        r.artists?.[0]?.name ||
        r.artistName ||
        r.uploader ||
        r.channel;
      const album = r.album?.title || r.album?.name || r.collectionName;
      const thumbnail =
        r.thumbnail ||
        r.album?.images?.[0]?.url ||
        r.artworkUrl100?.replace('100x100', '640x640') ||
        null;

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
        message: errorMessage(err),
      });
    }
  };

  const handleQualityConfirm = async (quality: string, forceRedownload: boolean) => {
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
        forceRedownload,
      });

      addNotification({
        type: 'success',
        title: 'Download Started',
        message: `Downloading ${qualityModal.title}...`,
      });
    } catch (err) {
      addNotification({
        type: 'error',
        title: 'Download Failed',
        message: errorMessage(err),
      });
    }
  };

  const handlePlay = async (result: SearchResult) => {
    if (
      searchType === 'album' ||
      searchType === 'playlist' ||
      searchType === 'podcast' ||
      searchType === 'show' ||
      searchType === 'audiobook'
    ) {
      openDetails(result);
      return;
    }

    try {
      if (!isBackendAvailable()) {
        addNotification({ type: 'error', title: 'Error', message: 'Backend not available' });
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
      logError('playback', 'Playback failed', errorDetail(err));
      addNotification({
        type: 'error',
        title: 'Playback Failed',
        message: errorMessage(err),
      });
    }
  };

  const handlePlayNext = (result: SearchResult) => {
    const track = toPlayableTrack(result);
    if (!track.url) return;
    insertNext(track);
    addNotification({
      type: 'success',
      title: 'Play next',
      message: (result as SearchResultLike).title || (result as SearchResultLike).name,
    });
  };

  const handleAddAllToQueue = () => {
    const currentResults = results || [];
    const tracks: PlayableTrack[] = currentResults.map(toPlayableTrack).filter((t) => t.url);
    if (!tracks.length) return;
    appendToQueue(tracks);
    addNotification({
      type: 'success',
      title: 'Added to queue',
      message: `${tracks.length} tracks added`,
    });
  };

  const openDetails = (result: SearchResult) => {
    const id = String(result.id ?? '');
    if (!id) return;
    const name = ((result as SearchResultLike).title ||
      (result as SearchResultLike).name) as string;
    logInfo('search', 'Opening detail', `${searchType} "${name}" on ${selectedPlatform}`);
    switch (searchType) {
      case 'album':
        nav.goToAlbum(selectedPlatform, id);
        break;
      case 'audiobook':
        nav.goToAlbum(selectedPlatform, `audiobook::${id}`);
        break;
      case 'playlist':
        nav.goToPlaylist(selectedPlatform, { serviceId: id });
        break;
      case 'show':
        nav.goToPlaylist(selectedPlatform, { serviceId: `show::${id}` });
        break;
      case 'podcast':
        nav.goToPlaylist(selectedPlatform, { serviceId: `podcast::${id}` });
        break;
      case 'channel':
        openArtistDetails(result);
        break;
    }
  };

  const openArtistDetails = (result: SearchResult) => {
    const id = String((result as SearchResultLike).id ?? '');
    if (!id) return;
    const name = ((result as SearchResultLike).title ||
      (result as SearchResultLike).name) as string;
    logInfo('search', 'Opening artist', `${name} on ${selectedPlatform}`);
    nav.goToArtist(selectedPlatform, id);
  };

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
              onChange={(e) => {
                setQuery(e.target.value);
                setShowSuggestions(true);
                setActiveSuggestion(-1);
              }}
              onFocus={() => {
                if (query.trim().length >= 2) setShowSuggestions(true);
              }}
              onKeyDown={handleKeyDown}
              autoComplete="off"
              className="flex-1 h-10 bg-muted/40 border-0 focus-visible:ring-1 focus-visible:ring-ring placeholder:text-muted-foreground/60"
            />
            <Button
              type="submit"
              disabled={!query.trim()}
              className="h-10 px-5 transition-colors duration-300"
              style={
                activePlatformColor
                  ? {
                      backgroundColor: activePlatformColor,
                      color: '#fff',
                      borderColor: activePlatformColor,
                    }
                  : undefined
              }
            >
              Search
            </Button>
          </form>

          {showSuggestions && suggestions.length > 0 && (
            <div className="absolute z-50 left-0 right-16 mt-1 bg-card border border-border rounded-lg shadow-xl overflow-hidden">
              {suggestions.map((s, i) => {
                const Icon = s.type === 'artist' ? User : s.type === 'album' ? Disc3 : Mic;
                return (
                  <button
                    key={`${s.type}:${s.text}`}
                    type="button"
                    tabIndex={0}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      commitSuggestion(s.text);
                    }}
                    className={cn(
                      'flex w-full items-center gap-2.5 px-3 py-2 text-sm text-left transition-colors duration-75',
                      i === activeSuggestion
                        ? 'bg-accent text-accent-foreground'
                        : 'hover:bg-muted/60 text-foreground'
                    )}
                  >
                    <Icon className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                    <span className="truncate">{s.text}</span>
                    <span className="ml-auto text-[10px] text-muted-foreground capitalize shrink-0">
                      {s.type}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
        </div>

        <div className="flex items-center gap-5">
          <div className="flex gap-1.5 flex-wrap">
            {platforms.map((p) => {
              const isActive = selectedPlatform === p.value;
              const color = PLATFORM_COLORS[p.value];
              return (
                <button
                  key={p.value}
                  type="button"
                  onClick={() => {
                    logInfo('search', 'Platform changed', p.value);
                    setSelectedPlatform(p.value as Platform);
                  }}
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
                onClick={() => {
                  logInfo('search', 'Search type changed', t);
                  setSearchType(t as SearchType);
                }}
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
        {searchQuery ? (
          <>
            {!isLoading &&
              !error &&
              (results?.length ?? 0) > 0 &&
              (searchType === 'track' || searchType === 'video') && (
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
              onPlayNext={
                searchType === 'track' || searchType === 'video' ? handlePlayNext : undefined
              }
              onResultClick={
                searchType === 'album' ||
                searchType === 'playlist' ||
                searchType === 'podcast' ||
                searchType === 'show' ||
                searchType === 'audiobook'
                  ? openDetails
                  : searchType === 'artist' || searchType === 'channel'
                    ? openArtistDetails
                    : undefined
              }
            />
            {hasNextPage && !isLoading && (results?.length ?? 0) > 0 && (
              <div className="flex justify-center py-4">
                <button
                  type="button"
                  onClick={() => void fetchNextPage()}
                  disabled={isFetchingNextPage}
                  className="px-4 py-2 rounded-md text-sm text-muted-foreground hover:text-foreground hover:bg-muted/60 transition-colors duration-100 disabled:opacity-50"
                >
                  {isFetchingNextPage ? 'Loading…' : 'Load more'}
                </button>
              </div>
            )}
          </>
        ) : browseCategory ? (
          <div className="flex flex-col h-full min-h-0">
            <div className="flex items-center gap-2 mb-4 shrink-0">
              <button
                type="button"
                onClick={() => setBrowseCategory(null)}
                className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-sm text-muted-foreground hover:text-foreground hover:bg-muted/60 transition-colors"
              >
                <ArrowLeft className="h-4 w-4" />
                Browse
              </button>
              <span className="text-sm font-semibold tracking-tight">{browseCategory.title}</span>
            </div>
            <div className="flex-1 min-h-0 overflow-y-auto">
              <ServiceHomeView
                source={
                  normalizePlatform(selectedPlatform) as Exclude<
                    import('@/features/library/api').ServicePlatform,
                    'local'
                  >
                }
                explorePath={browseCategory.path}
                onOpenCategory={(apiPath, title) => setBrowseCategory({ path: apiPath, title })}
                onSelectAlbum={(albumKey) => nav.goToAlbum(selectedPlatform, albumKey)}
                onSelectArtist={(artistKey) => nav.goToArtist(selectedPlatform, artistKey)}
                onSelectPlaylist={(_id, serviceId) =>
                  nav.goToPlaylist(selectedPlatform, { serviceId: serviceId ?? undefined })
                }
              />
            </div>
          </div>
        ) : (
          <SearchBrowse
            platform={selectedPlatform}
            onOpenCategory={(apiPath, title) => setBrowseCategory({ path: apiPath, title })}
            fallback={
              <div className="flex flex-col items-center justify-center h-full gap-3 text-muted-foreground select-none">
                <Search className="h-10 w-10 opacity-20" />
                <p className="text-sm">Search across 7 platforms</p>
              </div>
            }
          />
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
    </div>
  );
}

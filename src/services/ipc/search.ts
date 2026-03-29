import type { Platform, SearchType, SearchResult } from '@/types';
import { logInfo, logError } from '@/utils/logger';

class SearchService {
  private mapSearchType(platform: Platform, type: SearchType): string {
    const typeMap: Record<Platform, Partial<Record<SearchType, string>>> = {
      youtube: {
        video: 'video',
        playlist: 'playlist',
        channel: 'channel',
      },
      youtubemusic: {
        track: 'song',
        album: 'album',
        playlist: 'playlist',
        artist: 'artist',
        podcast: 'podcast',
      },
      spotify:    { track: 'track', album: 'album', playlist: 'playlist', artist: 'artist', show: 'show', episode: 'episode', audiobook: 'audiobook' },
      tidal:      { track: 'track', album: 'album', playlist: 'playlist', artist: 'artist', video: 'video' },
      deezer:     { track: 'track', album: 'album', playlist: 'playlist', artist: 'artist', podcast: 'podcast', episode: 'episode' },
      qobuz:      { track: 'track', album: 'album', playlist: 'playlist', artist: 'artist' },
      applemusic: { track: 'track', album: 'album', playlist: 'playlist', artist: 'artist', musicvideo: 'musicvideo' },
    };

    return typeMap[platform]?.[type] ?? type;
  }

  async performSearch(params: {
    platform: Platform;
    query: string;
    type: SearchType;
  }): Promise<SearchResult[]> {
    if (!window.electron) {
      throw new Error('Electron API not available. Please run in Electron mode, not browser mode.');
    }

    try {
      const platformType = this.mapSearchType(params.platform, params.type);

      const response = await window.electron.search.perform({
        platform: params.platform,
        query: params.query,
        type: platformType,
      });

      if (!response?.results) {
        return [];
      }

      const data = response.results;
      let results = this.normalizeResults(data, params.platform, params.type);

      results = results
        .filter((item: any) => item != null)
        .map((item: any) => this.normalizeResultItem(item, params.platform, params.type));

      return results;
    } catch (error) {
      throw new Error(`Search failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  async getAlbumDetails(params: {
    platform: Platform;
    albumId: string;
  }): Promise<any> {
    if (!window.electron) {
      throw new Error('Electron API not available');
    }

    logInfo('search', 'Fetching album details', `${params.platform} album ${params.albumId}`);
    try {
      const response = await window.electron.search.getAlbumDetails(params.platform, params.albumId);

      if (!response.success) {
        throw new Error(response.error || 'Failed to get album details');
      }

      const trackCount = response.data?.tracks?.length ?? response.data?.trackCount;
      logInfo('search', 'Album details loaded', trackCount != null ? `${trackCount} tracks` : params.albumId);
      return response.data;
    } catch (error) {
      logError('search', 'Failed to fetch album details', error instanceof Error ? (error.stack || error.message) : String(error));
      throw error;
    }
  }

  async getPlaylistDetails(params: {
    platform: Platform;
    playlistId: string;
  }): Promise<any> {
    if (!window.electron) {
      throw new Error('Electron API not available');
    }

    logInfo('search', 'Fetching playlist details', `${params.platform} playlist ${params.playlistId}`);
    try {
      const response = await window.electron.search.getPlaylistDetails(params.platform, params.playlistId);

      if (!response.success) {
        throw new Error(response.error || 'Failed to get playlist details');
      }

      const trackCount = response.data?.tracks?.length ?? response.data?.trackCount;
      logInfo('search', 'Playlist details loaded', trackCount != null ? `${trackCount} tracks` : params.playlistId);
      return response.data;
    } catch (error) {
      logError('search', 'Failed to fetch playlist details', error instanceof Error ? (error.stack || error.message) : String(error));
      throw error;
    }
  }

  async getArtistDetails(params: {
    platform: Platform;
    artistId: string;
  }): Promise<any> {
    if (!window.electron) {
      throw new Error('Electron API not available');
    }

    logInfo('search', 'Fetching artist details', `${params.platform} artist ${params.artistId}`);
    try {
      const response = await window.electron.search.getArtistDetails(params.platform, params.artistId);

      if (!response.success) {
        throw new Error(response.error || 'Failed to get artist details');
      }

      const albumCount = response.data?.albums?.length;
      logInfo('search', 'Artist details loaded', albumCount != null ? `${albumCount} albums` : params.artistId);
      return response.data;
    } catch (error) {
      logError('search', 'Failed to fetch artist details', error instanceof Error ? (error.stack || error.message) : String(error));
      throw error;
    }
  }

  private normalizeResultItem(item: any, platform: Platform, type: SearchType): any {
    const normalized: any = {
      ...item,
      platform,
      resultType: type,
      album: undefined,
    };

    switch (platform) {
      case 'spotify':
        normalized.title = item.name || item.title;
        normalized.name = item.name || item.title;

        if (type === 'track') {
          normalized.artist = item.artists?.[0]?.name || 'Unknown Artist';
          normalized.album = item.album?.name;
          normalized.thumbnail = item.album?.images?.[0]?.url || item.images?.[0]?.url;
          normalized.duration = item.duration_ms ? Math.floor(item.duration_ms / 1000) : undefined;
          normalized.url = item.external_urls?.spotify || item.uri;
          normalized.explicit = item.explicit ?? false;
          normalized.releaseDate = item.album?.release_date;
          normalized.popularity = item.popularity;
        } else if (type === 'album') {
          normalized.artist = item.artists?.[0]?.name || 'Unknown Artist';
          normalized.thumbnail = item.images?.[0]?.url;
          normalized.trackCount = item.total_tracks;
          normalized.url = item.external_urls?.spotify || item.uri;
          normalized.explicit = item.explicit ?? false;
          normalized.releaseDate = item.release_date;
        } else if (type === 'playlist') {
          normalized.owner = item.owner?.display_name || 'Unknown';
          normalized.thumbnail = item.images?.[0]?.url;
          normalized.trackCount = item.tracks?.total;
          normalized.url = item.external_urls?.spotify || item.uri;
        } else if (type === 'artist') {
          normalized.thumbnail = item.images?.[0]?.url;
          normalized.followerCount = item.followers?.total;
          normalized.genre = item.genres?.[0];
          normalized.url = item.external_urls?.spotify || item.uri;
        } else if (type === 'show' || type === 'podcast') {
          normalized.thumbnail = item.images?.[0]?.url;
          normalized.owner = item.publisher || 'Unknown';
          normalized.trackCount = item.total_episodes;
          normalized.url = item.external_urls?.spotify || item.uri;
          normalized.genre = item.media_type;
        } else if (type === 'episode') {
          normalized.thumbnail = item.images?.[0]?.url;
          normalized.duration = item.duration_ms ? Math.floor(item.duration_ms / 1000) : undefined;
          normalized.url = item.external_urls?.spotify || item.uri;
          normalized.releaseDate = item.release_date;
          normalized.explicit = item.explicit ?? false;
        } else if (type === 'audiobook') {
          normalized.thumbnail = item.images?.[0]?.url;
          normalized.owner = item.authors?.[0]?.name || item.narrators?.[0]?.name || 'Unknown';
          normalized.trackCount = item.total_chapters;
          normalized.url = item.external_urls?.spotify || item.uri;
        }
        break;

      case 'tidal': {
        const attr = item.attributes ?? item;
        normalized.title = attr.title || item.title || item.name;
        normalized.name = normalized.title;

        const tidalTags: string[] =
          attr.mediaMetadata?.tags ?? item.mediaTags ?? item.mediaMetadata?.tags ?? [];

        const tidalImg = (imgArr: any, uuidStr?: string): string | undefined => {
          if (Array.isArray(imgArr)) {
            const best = imgArr.find((i: any) => i.width >= 640) ?? imgArr[imgArr.length - 1];
            return best?.href ?? undefined;
          }
          if (typeof uuidStr === 'string' && uuidStr)
            return `https://resources.tidal.com/images/${uuidStr.replace(/-/g, '/')}/640x640.jpg`;
          return undefined;
        };

        if (type === 'track') {
          normalized.artist = item.artists?.[0]?.name || item.artist?.name || 'Unknown Artist';
          normalized.album = item.album?.title ?? attr.album?.title;
          normalized.thumbnail = tidalImg(item.album?.imageCover, item.album?.cover);
          normalized.duration = attr.duration ?? item.duration;
          normalized.url = attr.url || item.url || `https://tidal.com/browse/track/${item.id}`;
          normalized.explicit = attr.explicit ?? item.explicit ?? false;
          normalized.mediaTag = tidalTags[0];
          normalized.hires = tidalTags.includes('HIRES_LOSSLESS');
          normalized.releaseDate = attr.streamStartDate || item.streamStartDate || item.album?.releaseDate;
        } else if (type === 'album') {
          normalized.artist = item.artist?.name || item.artists?.[0]?.name || 'Unknown Artist';
          normalized.thumbnail = tidalImg(attr.imageCover, item.cover);
          normalized.trackCount = attr.numberOfItems ?? attr.numberOfTracks ?? item.numberOfTracks;
          normalized.url = attr.url || item.url || `https://tidal.com/browse/album/${item.id}`;
          normalized.explicit = attr.explicit ?? item.explicit ?? false;
          normalized.mediaTag = tidalTags[0];
          normalized.hires = tidalTags.includes('HIRES_LOSSLESS');
          normalized.releaseDate = attr.releaseDate ?? item.releaseDate;
        } else if (type === 'playlist') {
          normalized.owner = attr.creator?.name || item.creator?.name || 'Unknown';
          normalized.thumbnail = tidalImg(attr.imageCover || attr.squareImage, item.image);
          normalized.trackCount = attr.numberOfItems ?? attr.numberOfTracks ?? item.numberOfTracks;
          normalized.url = attr.url || item.url || `https://tidal.com/browse/playlist/${item.uuid || item.id}`;
          if (item.uuid) normalized.id = item.uuid;
        } else if (type === 'artist') {
          normalized.thumbnail = tidalImg(attr.picture, item.picture);
          normalized.url = attr.url || item.url || `https://tidal.com/browse/artist/${item.id}`;
        } else if (type === 'video') {
          normalized.artist = item.artists?.[0]?.name || attr.artist?.name || 'Unknown Artist';
          normalized.thumbnail = tidalImg(attr.imageCover || attr.imageLinks, item.imageId);
          normalized.duration = attr.duration ?? item.duration;
          normalized.url = attr.url || item.url || `https://tidal.com/browse/video/${item.id}`;
          normalized.explicit = attr.explicit ?? item.explicit ?? false;
          normalized.releaseDate = attr.releaseDate ?? item.releaseDate;
        }
        break;
      }

      case 'qobuz': {
        normalized.title = item.title || item.name;
        normalized.name = item.title || item.name;
        const qGenre = typeof item.genre === 'string' ? item.genre : item.genre?.name;

        if (type === 'track') {
          normalized.artist = item.performer?.name || item.artist?.name || 'Unknown Artist';
          normalized.album = item.album?.title;
          normalized.thumbnail = item.album?.image?.large || item.image?.large;
          normalized.duration = item.duration;
          normalized.url = `https://play.qobuz.com/track/${item.id}`;
          normalized.explicit = item.parental_warning ?? false;
          normalized.hires = item.hires_streamable ?? item.album?.hires_streamable ?? false;
          normalized.bitDepth = item.maximum_bit_depth || item.album?.maximum_bit_depth;
          normalized.sampleRate = item.maximum_sampling_rate || item.album?.maximum_sampling_rate;
          normalized.genre = qGenre || (typeof item.album?.genre === 'string' ? item.album.genre : item.album?.genre?.name);
          normalized.releaseDate = item.album?.released_at
            ? new Date(item.album.released_at * 1000).toISOString().slice(0, 10)
            : undefined;
        } else if (type === 'album') {
          normalized.artist = item.artist?.name || 'Unknown Artist';
          normalized.thumbnail = item.image?.large;
          normalized.trackCount = item.tracks_count;
          normalized.url = `https://play.qobuz.com/album/${item.id}`;
          normalized.explicit = item.parental_warning ?? false;
          normalized.hires = item.hires_streamable ?? false;
          normalized.bitDepth = item.maximum_bit_depth;
          normalized.sampleRate = item.maximum_sampling_rate;
          normalized.genre = qGenre;
          normalized.releaseDate = item.released_at
            ? new Date(item.released_at * 1000).toISOString().slice(0, 10)
            : item.release_date_original;
        } else if (type === 'playlist') {
          normalized.owner = item.owner?.name || 'Unknown';
          normalized.thumbnail = item.images?.[0] || item.image?.large;
          normalized.trackCount = item.tracks_count;
          normalized.url = `https://play.qobuz.com/playlist/${item.id}`;
        } else if (type === 'artist') {
          normalized.thumbnail = item.image?.large;
          normalized.url = `https://play.qobuz.com/artist/${item.id}`;
        }
        break;
      }

      case 'deezer': {
        normalized.title = item.title || item.name;
        normalized.name = item.title || item.name;
        const dzGenre = typeof item.genre === 'string' ? item.genre : item.genre?.name;

        if (type === 'track') {
          normalized.artist = item.artist?.name || 'Unknown Artist';
          normalized.album = item.album?.title;
          normalized.thumbnail = item.album?.cover_xl || item.album?.cover_big || item.picture_xl || item.picture_big;
          normalized.duration = item.duration;
          normalized.url = item.link || `https://www.deezer.com/track/${item.id}`;
          normalized.explicit = item.explicit_lyrics === 1 || item.explicit_content_lyrics === 1;
          normalized.genre = dzGenre;
          normalized.rank = item.rank;
        } else if (type === 'album') {
          normalized.artist = item.artist?.name || 'Unknown Artist';
          normalized.thumbnail = item.cover_xl || item.cover_big;
          normalized.trackCount = item.nb_tracks;
          normalized.url = item.link || `https://www.deezer.com/album/${item.id}`;
          normalized.explicit = item.explicit_lyrics === 1;
          normalized.releaseDate = item.release_date;
          normalized.genre = dzGenre;
        } else if (type === 'playlist') {
          normalized.owner = item.creator?.name || item.user?.name || 'Unknown';
          normalized.thumbnail = item.picture_xl || item.picture_big;
          normalized.trackCount = item.nb_tracks;
          normalized.url = item.link || `https://www.deezer.com/playlist/${item.id}`;
        } else if (type === 'artist') {
          normalized.thumbnail = item.picture_xl || item.picture_big;
          normalized.followerCount = item.nb_fan;
          normalized.url = item.link || `https://www.deezer.com/artist/${item.id}`;
        } else if (type === 'podcast') {
          normalized.title = item.title || item.name;
          normalized.name = normalized.title;
          normalized.thumbnail = item.picture_xl || item.picture_big;
          normalized.url = item.link || `https://www.deezer.com/show/${item.id}`;
        } else if (type === 'episode') {
          normalized.title = item.title || item.name;
          normalized.name = normalized.title;
          normalized.thumbnail = item.picture_xl || item.picture_big;
          normalized.duration = item.duration;
          normalized.url = item.link || `https://www.deezer.com/episode/${item.id}`;
          normalized.releaseDate = item.release_date;
        }
        break;
      }

      case 'applemusic':
        normalized.title = item.trackName || item.collectionName || item.artistName || item.name;
        normalized.name = normalized.title;

        if (type === 'track') {
          normalized.id = String(item.trackId);
          normalized.artist = item.artistName || 'Unknown Artist';
          normalized.album = item.collectionName;
          normalized.thumbnail = item.artworkUrl100?.replace('100x100', '640x640');
          normalized.duration = item.trackTimeMillis ? Math.floor(item.trackTimeMillis / 1000) : undefined;
          normalized.url = item.trackViewUrl;
          normalized.explicit = item.trackExplicitness === 'explicit';
          normalized.genre = item.primaryGenreName;
          normalized.releaseDate = item.releaseDate;
        } else if (type === 'album') {
          normalized.id = String(item.collectionId);
          normalized.artist = item.artistName || 'Unknown Artist';
          normalized.thumbnail = item.artworkUrl100?.replace('100x100', '640x640');
          normalized.trackCount = item.trackCount;
          normalized.url = item.collectionViewUrl;
          normalized.explicit = item.collectionExplicitness === 'explicit';
          normalized.genre = item.primaryGenreName;
          normalized.releaseDate = item.releaseDate;
        } else if (type === 'artist') {
          normalized.id = String(item.artistId);
          normalized.thumbnail = item.artworkUrl100?.replace('100x100', '640x640');
          normalized.url = item.artistLinkUrl;
          normalized.genre = item.primaryGenreName;
        } else if (type === 'musicvideo') {
          normalized.id = String(item.trackId);
          normalized.artist = item.artistName || 'Unknown Artist';
          normalized.thumbnail = item.artworkUrl100?.replace('100x100', '640x640');
          normalized.duration = item.trackTimeMillis ? Math.floor(item.trackTimeMillis / 1000) : undefined;
          normalized.url = item.trackViewUrl;
          normalized.releaseDate = item.releaseDate;
        }
        break;

      case 'youtube':
        normalized.title = item.title || item.channel || item.name;
        normalized.name = normalized.title;
        normalized.artist = item.artist || item.channel || item.uploader || 'Unknown';
        normalized.thumbnail = item.thumbnail || item.thumbnails?.[0]?.url;
        normalized.duration = item.duration;
        break;
      case 'youtubemusic': {
        normalized.title = item.title || item.channel || item.name;
        normalized.name = normalized.title;
        normalized.artist = item.artist || item.channel || item.uploader || 'Unknown';
        normalized.thumbnail = item.thumbnail_url || item.thumbnail || item.thumbnails?.[0]?.url;
        normalized.duration = item.duration_secs || item.duration;
        normalized.explicit = item.isExplicit ?? item.explicit ?? false;
        normalized.views = item.view_count;
        const rt = item.result_type;
        const browseId = item.browse_id || item.id;
        if (rt === 'album') {
          normalized.url = `https://music.youtube.com/browse/${browseId}`;
        } else if (rt === 'playlist') {
          const listId = item.id?.startsWith?.('VL') ? item.id.slice(2) : item.id;
          normalized.url = `https://music.youtube.com/playlist?list=${listId}`;
        } else if (rt === 'artist') {
          normalized.url = `https://music.youtube.com/channel/${item.id}`;
        } else if (rt === 'podcast') {
          normalized.url = `https://music.youtube.com/browse/${item.id}`;
        } else {
          normalized.url = item.url || item.webpage_url || (item.id ? `https://youtube.com/watch?v=${item.id}` : undefined);
        }
        break;
      }
    }

    return normalized;
  }

  private normalizeResults(results: any, platform: Platform, type: SearchType): any[] {
    switch (platform) {
      case 'spotify':
        switch (type) {
          case 'track':
            return results.tracks?.items || [];
          case 'album':
            return results.albums?.items || [];
          case 'artist':
            return results.artists?.items || [];
          case 'playlist':
            return results.playlists?.items || [];
          case 'show':
          case 'podcast':
            return results.shows?.items || [];
          case 'episode':
            return results.episodes?.items || [];
          case 'audiobook':
            return results.audiobooks?.items || [];
          default:
            return [];
        }

      case 'qobuz':
        switch (type) {
          case 'track':
            return results.tracks?.items || [];
          case 'album':
            return results.albums?.items || [];
          case 'artist':
            return results.artists?.items || [];
          case 'playlist':
            return results.playlists?.items || [];
          default:
            return [];
        }

      case 'tidal':
        switch (type) {
          case 'track':
            return results.tracks?.map((track: any) => track.resource) || [];
          case 'album':
            return results.albums?.map((album: any) => album.resource) || [];
          case 'artist':
            return results.artists?.map((artist: any) => artist.resource) || [];
          case 'playlist':
            return results.playlists?.map((playlist: any) => playlist.resource) || [];
          case 'video':
            return results.videos?.map((v: any) => v.resource) || [];
          default:
            return [];
        }

      case 'deezer':
        return Array.isArray(results) ? results : (results.data || []);

      case 'applemusic':
        if (!Array.isArray(results)) return [];
        switch (type) {
          case 'track':
            return results.filter((item: any) => item.wrapperType === 'track' && item.kind === 'song');
          case 'album':
            return results.filter((item: any) => item.wrapperType === 'collection' && item.collectionType === 'Album');
          case 'artist':
            return results.filter((item: any) => item.wrapperType === 'artist');
          case 'playlist':
            return results.filter((item: any) => item.wrapperType === 'collection' && item.collectionType === 'Compilation');
          case 'musicvideo':
            return results.filter((item: any) => item.wrapperType === 'track' && item.kind === 'music-video');
          default:
            return results;
        }

      case 'youtube':
      case 'youtubemusic':
        return Array.isArray(results) ? results : [];

      default:
        return Array.isArray(results) ? results : [];
    }
  }
}

export const searchService = new SearchService();

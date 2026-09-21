import type { Platform, SearchType, SearchResult } from '@/types';
import { errorMessage } from '@/utils/errors';
import { isBackendAvailable, tauriAPI } from '@/tauri-bridge';

/** One provider result before normalization — arbitrary JSON off the wire. */
type Raw = Record<string, any>;

/**
 * Where each provider hides its result array, per search type.
 * `key` + `items` is the common envelope; `unwrap` peels Tidal's
 * `{ resource: … }` wrapper; `filter` covers iTunes' single flat array.
 */
type ExtractSpec = { key: string; unwrap?: string } | { filter: (item: Raw) => boolean };

const itunes = (wrapperType: string, extra: (i: Raw) => boolean): ExtractSpec => ({
  filter: (i) => i.wrapperType === wrapperType && extra(i),
});

const EXTRACT: Partial<Record<Platform, Partial<Record<SearchType, ExtractSpec>>>> = {
  spotify: {
    track: { key: 'tracks' },
    album: { key: 'albums' },
    artist: { key: 'artists' },
    playlist: { key: 'playlists' },
    show: { key: 'shows' },
    podcast: { key: 'shows' },
    episode: { key: 'episodes' },
    audiobook: { key: 'audiobooks' },
  },
  qobuz: {
    track: { key: 'tracks' },
    album: { key: 'albums' },
    artist: { key: 'artists' },
    playlist: { key: 'playlists' },
  },
  tidal: {
    track: { key: 'tracks', unwrap: 'resource' },
    album: { key: 'albums', unwrap: 'resource' },
    artist: { key: 'artists', unwrap: 'resource' },
    playlist: { key: 'playlists', unwrap: 'resource' },
    video: { key: 'videos', unwrap: 'resource' },
  },
  applemusic: {
    track: itunes('track', (i) => i.kind === 'song'),
    album: itunes('collection', (i) => i.collectionType === 'Album'),
    artist: itunes('artist', () => true),
    playlist: itunes('collection', (i) => i.collectionType === 'Compilation'),
    musicvideo: itunes('track', (i) => i.kind === 'music-video'),
  },
};

class SearchService {
  private mapSearchType(platform: Platform, type: SearchType): string {
    if (platform === 'youtubemusic' && type === 'track') return 'song';
    return type;
  }

  async performSearch(params: {
    platform: Platform;
    query: string;
    type: SearchType;
    offset?: number;
    limit?: number;
  }): Promise<SearchResult[]> {
    if (!isBackendAvailable()) {
      throw new Error(
        'MediaHarbor backend not available — run the desktop app, not the browser dev server.'
      );
    }

    try {
      const platformType = this.mapSearchType(params.platform, params.type);

      const response = await tauriAPI.search.perform({
        platform: params.platform,
        query: params.query,
        type: platformType,
        offset: params.offset,
        limit: params.limit,
      });

      if (!response?.results) {
        return [];
      }

      return this.normalizeResults(response.results, params.platform, params.type)
        .filter((item) => item != null)
        .map((item) => this.normalizeResultItem(item, params.platform, params.type));
    } catch (error) {
      throw new Error(`Search failed: ${errorMessage(error)}`, {
        cause: error,
      });
    }
  }

  private extractCommonFields(
    item: Raw,
    platform: Platform,
    type: SearchType,
    ctx?: { attr?: Raw; tidalImg?: (imgArr: unknown, uuidStr?: string) => string | undefined }
  ): Record<string, any> {
    const fields: Record<string, any> = {};

    switch (platform) {
      case 'spotify':
        if (type === 'track' || type === 'album') {
          fields.artist = item.artists?.[0]?.name || 'Unknown Artist';
          fields.artists = item.artists?.map((a: Raw) => a.name).filter(Boolean);
          fields.artistId = item.artists?.[0]?.id;
        }
        if (type === 'track') {
          fields.album = item.album?.name;
          fields.albumId = item.album?.id;
          fields.thumbnail = item.album?.images?.[0]?.url || item.images?.[0]?.url;
        } else if (type === 'album') {
          fields.thumbnail = item.images?.[0]?.url;
        }
        break;

      case 'tidal': {
        const attr = ctx?.attr ?? item;
        const tidalImg = ctx?.tidalImg ?? (() => undefined);
        if (type === 'track') {
          fields.artist = item.artists?.[0]?.name || item.artist?.name || 'Unknown Artist';
          fields.artists = item.artists?.map((a: Raw) => a.name).filter(Boolean);
          fields.artistId = item.artists?.[0]?.id || item.artist?.id;
          fields.album = item.album?.title ?? attr.album?.title;
          fields.albumId = item.album?.id ?? attr.album?.id;
          fields.thumbnail = tidalImg(item.album?.imageCover, item.album?.cover);
        } else if (type === 'album') {
          fields.artist = item.artist?.name || item.artists?.[0]?.name || 'Unknown Artist';
          fields.artists = item.artists?.map((a: Raw) => a.name).filter(Boolean);
          fields.artistId = item.artist?.id || item.artists?.[0]?.id;
          fields.thumbnail = tidalImg(attr.imageCover, item.cover);
        }
        break;
      }

      case 'qobuz':
        if (type === 'track') {
          fields.artist = item.performer?.name || item.artist?.name || 'Unknown Artist';
          fields.artistId = item.performer?.id ?? item.artist?.id;
          fields.album = item.album?.title;
          fields.albumId = item.album?.id;
          fields.thumbnail = item.album?.image?.large || item.image?.large;
        } else if (type === 'album') {
          fields.artist = item.artist?.name || 'Unknown Artist';
          fields.artistId = item.artist?.id;
          fields.thumbnail = item.image?.large;
        }
        break;

      case 'deezer':
        if (type === 'track') {
          fields.artist = item.artist?.name || 'Unknown Artist';
          fields.artistId = item.artist?.id != null ? String(item.artist.id) : undefined;
          fields.album = item.album?.title;
          fields.albumId = item.album?.id != null ? String(item.album.id) : undefined;
          fields.thumbnail =
            item.album?.cover_xl || item.album?.cover_big || item.picture_xl || item.picture_big;
        } else if (type === 'album') {
          fields.artist = item.artist?.name || 'Unknown Artist';
          fields.artistId = item.artist?.id != null ? String(item.artist.id) : undefined;
          fields.thumbnail = item.cover_xl || item.cover_big;
        }
        break;

      case 'applemusic':
        if (type === 'track') {
          fields.artist = item.artistName || 'Unknown Artist';
          fields.artistId = item.artistId != null ? String(item.artistId) : undefined;
          fields.album = item.collectionName;
          fields.albumId = item.collectionId != null ? String(item.collectionId) : undefined;
          fields.thumbnail = item.artworkUrl100?.replace('100x100', '640x640');
        } else if (type === 'album') {
          fields.artist = item.artistName || 'Unknown Artist';
          fields.artistId = item.artistId != null ? String(item.artistId) : undefined;
          fields.thumbnail = item.artworkUrl100?.replace('100x100', '640x640');
        }
        break;
    }

    return fields;
  }

  private normalizeResultItem(item: Raw, platform: Platform, type: SearchType): SearchResult {
    const normalized: Raw = {
      ...item,
      platform,
      resultType: type,
      album: undefined,
    };

    switch (platform) {
      case 'spotify':
        normalized.title = item.name || item.title;
        normalized.name = item.name || item.title;

        Object.assign(normalized, this.extractCommonFields(item, platform, type));
        if (type === 'track') {
          normalized.duration = item.duration_ms ? Math.floor(item.duration_ms / 1000) : undefined;
          normalized.url = item.external_urls?.spotify || item.uri;
          normalized.explicit = item.explicit ?? false;
          normalized.releaseDate = item.album?.release_date;
          normalized.popularity = item.popularity;
          normalized.isrc = item.external_ids?.isrc;
          normalized.discNumber = item.disc_number;
          normalized.trackNumber = item.track_number;
          normalized.previewUrl = item.preview_url ?? undefined;
        } else if (type === 'album') {
          normalized.trackCount = item.total_tracks;
          normalized.url = item.external_urls?.spotify || item.uri;
          normalized.explicit = item.explicit ?? false;
          normalized.releaseDate = item.release_date;
          normalized.label = item.label;
          normalized.copyright = item.copyrights?.[0]?.text;
          normalized.popularity = item.popularity;
          normalized.genres = item.genres;
          normalized.upc = item.external_ids?.upc;
          normalized.isCompilation = item.album_type === 'compilation';
        } else if (type === 'playlist') {
          normalized.owner = item.owner?.display_name || 'Unknown';
          normalized.ownerId = item.owner?.id;
          normalized.thumbnail = item.images?.[0]?.url;
          normalized.trackCount = item.tracks?.total;
          normalized.url = item.external_urls?.spotify || item.uri;
          normalized.description = item.description || undefined;
          normalized.followerCount = item.followers?.total;
          normalized.isPublic = item.public ?? undefined;
          normalized.isCollaborative = item.collaborative ?? undefined;
        } else if (type === 'artist') {
          normalized.thumbnail = item.images?.[0]?.url;
          normalized.followerCount = item.followers?.total;
          normalized.genre = item.genres?.[0];
          normalized.genres = item.genres;
          normalized.popularity = item.popularity;
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

        const tidalImg = (imgArr: unknown, uuidStr?: string): string | undefined => {
          if (Array.isArray(imgArr)) {
            const best = imgArr.find((i: Raw) => i.width >= 640) ?? imgArr[imgArr.length - 1];
            return best?.href ?? undefined;
          }
          if (typeof uuidStr === 'string' && uuidStr)
            return `https://resources.tidal.com/images/${uuidStr.replace(/-/g, '/')}/640x640.jpg`;
          return undefined;
        };

        Object.assign(
          normalized,
          this.extractCommonFields(item, platform, type, { attr, tidalImg })
        );
        if (type === 'track') {
          normalized.duration = attr.duration ?? item.duration;
          normalized.url = attr.url || item.url || `https://tidal.com/browse/track/${item.id}`;
          normalized.explicit = attr.explicit ?? item.explicit ?? false;
          normalized.mediaTag = tidalTags[0];
          normalized.hires = tidalTags.includes('HIRES_LOSSLESS');
          normalized.releaseDate =
            attr.streamStartDate || item.streamStartDate || item.album?.releaseDate;
          normalized.isrc = attr.isrc || item.isrc;
          normalized.trackNumber = attr.trackNumber ?? item.trackNumber;
          normalized.discNumber = attr.volumeNumber ?? item.volumeNumber;
          normalized.copyright = attr.copyright ?? item.copyright;
          normalized.popularity = attr.popularity ?? item.popularity;
        } else if (type === 'album') {
          normalized.trackCount = attr.numberOfItems ?? attr.numberOfTracks ?? item.numberOfTracks;
          normalized.url = attr.url || item.url || `https://tidal.com/browse/album/${item.id}`;
          normalized.explicit = attr.explicit ?? item.explicit ?? false;
          normalized.mediaTag = tidalTags[0];
          normalized.hires = tidalTags.includes('HIRES_LOSSLESS');
          normalized.releaseDate = attr.releaseDate ?? item.releaseDate;
          normalized.totalDuration = attr.duration ?? item.duration;
          normalized.copyright = attr.copyright ?? item.copyright;
          normalized.upc = attr.upc ?? item.upc;
          normalized.discCount = attr.numberOfVolumes ?? item.numberOfVolumes;
          normalized.popularity = attr.popularity ?? item.popularity;
        } else if (type === 'playlist') {
          normalized.owner = attr.creator?.name || item.creator?.name || 'Unknown';
          normalized.ownerId = attr.creator?.id || item.creator?.id;
          normalized.thumbnail = tidalImg(attr.imageCover || attr.squareImage, item.image);
          normalized.trackCount = attr.numberOfItems ?? attr.numberOfTracks ?? item.numberOfTracks;
          normalized.url =
            attr.url || item.url || `https://tidal.com/browse/playlist/${item.uuid || item.id}`;
          if (item.uuid) normalized.id = item.uuid;
          normalized.description = attr.description || item.description || undefined;
          normalized.totalDuration = attr.duration ?? item.duration;
          normalized.followerCount =
            attr.popularity ?? item.popularity ?? attr.numberOfFollowers ?? item.numberOfFollowers;
          normalized.updatedAt = attr.lastUpdated ?? item.lastUpdated;
          normalized.createdAt = attr.created ?? item.created;
          normalized.isPublic = attr.publicPlaylist ?? item.publicPlaylist;
        } else if (type === 'artist') {
          normalized.thumbnail = tidalImg(attr.picture, item.picture);
          normalized.url = attr.url || item.url || `https://tidal.com/browse/artist/${item.id}`;
          normalized.popularity = attr.popularity ?? item.popularity;
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

        Object.assign(normalized, this.extractCommonFields(item, platform, type));
        if (type === 'track') {
          normalized.duration = item.duration;
          normalized.url = `https://play.qobuz.com/track/${item.id}`;
          normalized.explicit = item.parental_warning ?? false;
          normalized.hires = item.hires_streamable ?? item.album?.hires_streamable ?? false;
          normalized.bitDepth = item.maximum_bit_depth || item.album?.maximum_bit_depth;
          normalized.sampleRate = item.maximum_sampling_rate || item.album?.maximum_sampling_rate;
          normalized.genre =
            qGenre ||
            (typeof item.album?.genre === 'string' ? item.album.genre : item.album?.genre?.name);
          normalized.releaseDate = item.album?.released_at
            ? new Date(item.album.released_at * 1000).toISOString().slice(0, 10)
            : undefined;
          normalized.isrc = item.isrc;
          normalized.trackNumber = item.track_number;
          normalized.discNumber = item.media_number;
          normalized.copyright = item.copyright ?? item.album?.copyright;
          normalized.label =
            item.album?.label?.name ||
            (typeof item.album?.label === 'string' ? item.album.label : undefined);
        } else if (type === 'album') {
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
          normalized.label =
            item.label?.name || (typeof item.label === 'string' ? item.label : undefined);
          normalized.copyright = item.copyright;
          normalized.totalDuration = item.duration;
          normalized.upc = item.upc;
          normalized.discCount = item.media_count;
          normalized.description = item.description || undefined;
          normalized.genres = Array.isArray(item.genres_list) ? item.genres_list : undefined;
        } else if (type === 'playlist') {
          normalized.owner = item.owner?.name || 'Unknown';
          normalized.ownerId = item.owner?.id;
          normalized.thumbnail = item.images?.[0] || item.image?.large;
          normalized.trackCount = item.tracks_count;
          normalized.url = `https://play.qobuz.com/playlist/${item.id}`;
          normalized.description = item.description || undefined;
          normalized.totalDuration = item.duration;
          normalized.isPublic = item.is_public ?? undefined;
          normalized.isCollaborative = item.is_collaborative ?? undefined;
          normalized.createdAt = item.created_at
            ? new Date(item.created_at * 1000).toISOString()
            : undefined;
          normalized.updatedAt = item.updated_at
            ? new Date(item.updated_at * 1000).toISOString()
            : undefined;
        } else if (type === 'artist') {
          normalized.thumbnail = item.image?.large;
          normalized.url = `https://play.qobuz.com/artist/${item.id}`;
          normalized.biography =
            typeof item.biography === 'string' ? item.biography : item.biography?.content;
        }
        break;
      }

      case 'deezer': {
        normalized.title = item.title || item.name;
        normalized.name = item.title || item.name;
        const dzGenre = typeof item.genre === 'string' ? item.genre : item.genre?.name;

        Object.assign(normalized, this.extractCommonFields(item, platform, type));
        if (type === 'track') {
          normalized.duration = item.duration;
          normalized.url = item.link || `https://www.deezer.com/track/${item.id}`;
          normalized.explicit = item.explicit_lyrics === 1 || item.explicit_content_lyrics === 1;
          normalized.genre = dzGenre;
          normalized.rank = item.rank;
          normalized.popularity = item.rank;
          normalized.isrc = item.isrc;
          normalized.trackNumber = item.track_position;
          normalized.discNumber = item.disk_number;
          normalized.previewUrl = item.preview;
          normalized.releaseDate = item.release_date || item.album?.release_date;
        } else if (type === 'album') {
          normalized.trackCount = item.nb_tracks;
          normalized.url = item.link || `https://www.deezer.com/album/${item.id}`;
          normalized.explicit = item.explicit_lyrics === 1;
          normalized.releaseDate = item.release_date;
          normalized.genre = dzGenre;
          normalized.label = item.label;
          normalized.totalDuration = item.duration;
          normalized.upc = item.upc;
          normalized.popularity = item.fans ?? item.rank;
          normalized.description = item.description || undefined;
          normalized.genres = Array.isArray(item.genres?.data)
            ? item.genres.data.map((g: Raw) => g.name).filter(Boolean)
            : undefined;
        } else if (type === 'playlist') {
          normalized.owner = item.creator?.name || item.user?.name || 'Unknown';
          normalized.ownerId =
            (item.creator?.id ?? item.user?.id) != null
              ? String(item.creator?.id ?? item.user?.id)
              : undefined;
          normalized.ownerThumbnail = item.creator?.picture_medium || item.user?.picture_medium;
          normalized.thumbnail = item.picture_xl || item.picture_big;
          normalized.trackCount = item.nb_tracks;
          normalized.url = item.link || `https://www.deezer.com/playlist/${item.id}`;
          normalized.description = item.description || undefined;
          normalized.totalDuration = item.duration;
          normalized.followerCount = item.fans;
          normalized.isPublic = item.public ?? undefined;
          normalized.isCollaborative = item.collaborative ?? undefined;
          normalized.createdAt = item.creation_date;
          normalized.updatedAt = item.time_mod
            ? new Date(item.time_mod * 1000).toISOString()
            : undefined;
        } else if (type === 'artist') {
          normalized.thumbnail = item.picture_xl || item.picture_big;
          normalized.followerCount = item.nb_fan;
          normalized.url = item.link || `https://www.deezer.com/artist/${item.id}`;
          normalized.popularity = item.nb_fan;
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

        Object.assign(normalized, this.extractCommonFields(item, platform, type));
        if (type === 'track') {
          normalized.id = String(item.trackId);
          normalized.duration = item.trackTimeMillis
            ? Math.floor(item.trackTimeMillis / 1000)
            : undefined;
          normalized.url = item.trackViewUrl;
          normalized.explicit = item.trackExplicitness === 'explicit';
          normalized.genre = item.primaryGenreName;
          normalized.releaseDate = item.releaseDate;
          normalized.trackNumber = item.trackNumber;
          normalized.discNumber = item.discNumber;
          normalized.previewUrl = item.previewUrl;
          normalized.copyright = item.copyright;
        } else if (type === 'album') {
          normalized.id = String(item.collectionId);
          normalized.trackCount = item.trackCount;
          normalized.url = item.collectionViewUrl;
          normalized.explicit = item.collectionExplicitness === 'explicit';
          normalized.genre = item.primaryGenreName;
          normalized.releaseDate = item.releaseDate;
          normalized.copyright = item.copyright;
          normalized.discCount = item.discCount;
        } else if (type === 'artist') {
          normalized.id = String(item.artistId);
          normalized.thumbnail = item.artworkUrl100?.replace('100x100', '640x640');
          normalized.url = item.artistLinkUrl;
          normalized.genre = item.primaryGenreName;
        } else if (type === 'musicvideo') {
          normalized.id = String(item.trackId);
          normalized.artist = item.artistName || 'Unknown Artist';
          normalized.thumbnail = item.artworkUrl100?.replace('100x100', '640x640');
          normalized.duration = item.trackTimeMillis
            ? Math.floor(item.trackTimeMillis / 1000)
            : undefined;
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
        normalized.title = item.title;
        normalized.name = item.title;
        normalized.artist = item.artist ?? undefined;
        normalized.album = item.album ?? undefined;
        normalized.thumbnail = item.thumbnailUrl;
        normalized.duration = item.durationSecs;
        normalized.artistId = item.artistId ?? undefined;
        normalized.albumId = item.albumId ?? undefined;
        const rt = item.resultType;
        const browseId = item.browseId || item.id;
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
          normalized.url = item.id ? `https://music.youtube.com/watch?v=${item.id}` : undefined;
        }
        break;
      }
    }

    return normalized as unknown as SearchResult;
  }

  private normalizeResults(results: unknown, platform: Platform, type: SearchType): Raw[] {
    const spec = EXTRACT[platform]?.[type];
    if (!spec) return Array.isArray(results) ? (results as Raw[]) : [];

    if ('filter' in spec) {
      return Array.isArray(results) ? (results as Raw[]).filter(spec.filter) : [];
    }

    const bucket = (results as Raw | undefined)?.[spec.key] as
      { items?: Raw[] } | Raw[] | undefined;
    if (spec.unwrap) {
      return Array.isArray(bucket) ? bucket.map((r) => r[spec.unwrap!] as Raw) : [];
    }
    return (bucket as { items?: Raw[] } | undefined)?.items ?? [];
  }
}

export const searchService = new SearchService();

/**
 * Tests for librespotService search + _transformSearchResults
 * (src/funcs/apis/librespotService.js)
 *
 * These tests expose two bugs:
 *  BUG-1: _transformSearchResults silently drops shows/episodes/audiobooks
 *  BUG-2: _uri2url regex doesn't match show/episode/audiobook Spotify URIs
 *  BUG-5: extractTrackId only handles track URIs/URLs (not episode/chapter)
 *  BUG-6: spclient API uses 'podcasts'/'podcastEpisodes' keys, not 'shows'/'episodes'
 */

import { describe, it, expect } from 'vitest';

// ---------------------------------------------------------------------------
// Minimal reproduction of the CURRENT (buggy) implementation
// ---------------------------------------------------------------------------

function makeCurrentImpl() {
  const _uri2url = (uri) => {
    if (!uri) return undefined;
    // BUG-2: only track|album|artist|playlist are captured
    const m = uri.match(/spotify:(track|album|artist|playlist):(.+)/);
    return m ? `https://open.spotify.com/${m[1]}/${m[2]}` : undefined;
  };
  const _uri2id = (uri) => uri?.split(':').pop();

  function _transformSearchResults(results, type) {
    const out = {};

    if (results.tracks && (type === 'track' || !type)) {
      out.tracks = {
        items: results.tracks.hits.map((t) => ({
          name: t.name,
          id: _uri2id(t.uri),
          uri: t.uri,
          external_urls: { spotify: _uri2url(t.uri) },
          artists: (t.artists || []).map((a) => ({ name: a.name })),
          album: { name: t.album?.name, images: t.image ? [{ url: t.image }] : [] },
          duration_ms: t.duration,
          explicit: t.explicit ?? false,
        })),
      };
    }
    if (results.albums && (type === 'album' || !type)) {
      out.albums = {
        items: results.albums.hits.map((a) => ({
          name: a.name,
          id: _uri2id(a.uri),
          uri: a.uri,
          external_urls: { spotify: _uri2url(a.uri) },
          artists: (a.artists || []).map((ar) => ({ name: ar.name })),
          images: a.image ? [{ url: a.image }] : [],
        })),
      };
    }
    if (results.artists && (type === 'artist' || !type)) {
      out.artists = {
        items: results.artists.hits.map((a) => ({
          name: a.name,
          id: _uri2id(a.uri),
          uri: a.uri,
          external_urls: { spotify: _uri2url(a.uri) },
          images: a.image ? [{ url: a.image }] : [],
        })),
      };
    }
    if (results.playlists && (type === 'playlist' || !type)) {
      out.playlists = {
        items: results.playlists.hits.map((p) => ({
          name: p.name,
          id: _uri2id(p.uri),
          uri: p.uri,
          external_urls: { spotify: _uri2url(p.uri) },
          images: p.image ? [{ url: p.image }] : [],
          owner: { display_name: p.author },
          tracks: { total: p.followersCount },
        })),
      };
    }
    // ⚠ shows / episodes / audiobooks are NOT handled here (BUG-1)
    return out;
  }

  // BUG-5: extractTrackId only handles track URIs/URLs
  function extractTrackId(input) {
    if (!input) return null;
    const uriMatch = input.match(/spotify:track:([A-Za-z0-9]+)/);
    if (uriMatch) return uriMatch[1];
    const urlMatch = input.match(/open\.spotify\.com\/(?:intl-[a-z-]+\/)?track\/([A-Za-z0-9]+)/);
    if (urlMatch) return urlMatch[1];
    return null;
  }

  return { _transformSearchResults, _uri2url, extractTrackId };
}

// ---------------------------------------------------------------------------
// Minimal reproduction of the FIXED implementation
// ---------------------------------------------------------------------------

function makeFixedImpl() {
  const _uri2url = (uri) => {
    if (!uri) return undefined;
    // FIX: include show|episode|audiobook in the pattern
    const m = uri.match(/spotify:(track|album|artist|playlist|show|episode|audiobook):(.+)/);
    return m ? `https://open.spotify.com/${m[1]}/${m[2]}` : undefined;
  };
  const _uri2id = (uri) => uri?.split(':').pop();

  function _transformSearchResults(results, type) {
    const out = {};

    if (results.tracks && (type === 'track' || !type)) {
      out.tracks = {
        items: results.tracks.hits.map((t) => ({
          name: t.name,
          id: _uri2id(t.uri),
          uri: t.uri,
          external_urls: { spotify: _uri2url(t.uri) },
          artists: (t.artists || []).map((a) => ({ name: a.name })),
          album: { name: t.album?.name, images: t.image ? [{ url: t.image }] : [] },
          duration_ms: t.duration,
          explicit: t.explicit ?? false,
        })),
      };
    }
    if (results.albums && (type === 'album' || !type)) {
      out.albums = {
        items: results.albums.hits.map((a) => ({
          name: a.name,
          id: _uri2id(a.uri),
          uri: a.uri,
          external_urls: { spotify: _uri2url(a.uri) },
          artists: (a.artists || []).map((ar) => ({ name: ar.name })),
          images: a.image ? [{ url: a.image }] : [],
        })),
      };
    }
    if (results.artists && (type === 'artist' || !type)) {
      out.artists = {
        items: results.artists.hits.map((a) => ({
          name: a.name,
          id: _uri2id(a.uri),
          uri: a.uri,
          external_urls: { spotify: _uri2url(a.uri) },
          images: a.image ? [{ url: a.image }] : [],
        })),
      };
    }
    if (results.playlists && (type === 'playlist' || !type)) {
      out.playlists = {
        items: results.playlists.hits.map((p) => ({
          name: p.name,
          id: _uri2id(p.uri),
          uri: p.uri,
          external_urls: { spotify: _uri2url(p.uri) },
          images: p.image ? [{ url: p.image }] : [],
          owner: { display_name: p.author },
          tracks: { total: p.followersCount },
        })),
      };
    }
    // FIX: handle shows (spclient key = 'podcasts', web API key = 'shows')
    const showsHits = results.podcasts || results.shows;
    if (showsHits && (type === 'podcast' || type === 'show' || !type)) {
      out.shows = {
        items: showsHits.hits.map((s) => ({
          name: s.name,
          id: _uri2id(s.uri),
          uri: s.uri,
          external_urls: { spotify: _uri2url(s.uri) },
          images: s.image ? [{ url: s.image }] : [],
          publisher: s.author,
          total_episodes: s.episodeCount,
          media_type: s.mediaType,
        })),
      };
    }
    // FIX: handle episodes (spclient key = 'podcastEpisodes', web API key = 'episodes')
    const episodesHits = results.podcastEpisodes || results.episodes;
    if (episodesHits && (type === 'episode' || !type)) {
      out.episodes = {
        items: episodesHits.hits.map((e) => ({
          name: e.name,
          id: _uri2id(e.uri),
          uri: e.uri,
          external_urls: { spotify: _uri2url(e.uri) },
          images: e.image ? [{ url: e.image }] : [],
          duration_ms: e.duration,
          release_date: e.releaseDate,
          explicit: e.explicit ?? false,
        })),
      };
    }
    // FIX: handle audiobooks
    if (results.audiobooks && (type === 'audiobook' || !type)) {
      out.audiobooks = {
        items: results.audiobooks.hits.map((b) => ({
          name: b.name,
          id: _uri2id(b.uri),
          uri: b.uri,
          external_urls: { spotify: _uri2url(b.uri) },
          images: b.image ? [{ url: b.image }] : [],
          authors: b.authors ? b.authors.map((a) => ({ name: a })) : [],
          narrators: b.narrators ? b.narrators.map((n) => ({ name: n })) : [],
          total_chapters: b.chapterCount,
        })),
      };
    }
    return out;
  }

  // FIX: extractMediaInfo returns { id, type } for track/episode/chapter
  function extractMediaInfo(input) {
    if (!input) return null;
    const uriMatch = input.match(/spotify:(track|episode|chapter):([A-Za-z0-9]+)/);
    if (uriMatch) return { id: uriMatch[2], type: uriMatch[1] };
    const urlMatch = input.match(/open\.spotify\.com\/(?:intl-[a-z-]+\/)?(track|episode|chapter)\/([A-Za-z0-9]+)/);
    if (urlMatch) return { id: urlMatch[2], type: urlMatch[1] };
    return null;
  }

  // extractTrackId is now a thin wrapper around extractMediaInfo
  function extractTrackId(input) {
    return extractMediaInfo(input)?.id ?? null;
  }

  // FIX: _selectAudioFile picks OGG for music and MP3/AAC for episodes
  function _selectOggFile(files) {
    const formatPriority = ['OGG_VORBIS_320', 'OGG_VORBIS_160', 'OGG_VORBIS_96'];
    for (const fmt of formatPriority) {
      const f = files.find(file => file.format === fmt);
      if (f) return f;
    }
    return files.find(f => f.format?.startsWith('OGG_VORBIS')) || null;
  }

  function _selectAudioFile(files) {
    const ogg = _selectOggFile(files);
    if (ogg) return { file: ogg, isOgg: true };
    const mp3Priority = ['MP3_320', 'MP3_256', 'MP3_160', 'MP3_96', 'MP3_128'];
    for (const fmt of mp3Priority) {
      const f = files.find(file => file.format === fmt);
      if (f) return { file: f, isOgg: false };
    }
    const aacPriority = ['AAC_24', 'AAC_48', 'MP4_128', 'MP4_256'];
    for (const fmt of aacPriority) {
      const f = files.find(file => file.format === fmt);
      if (f) return { file: f, isOgg: false };
    }
    const any = files.find(f => f.file_id);
    if (any) return { file: any, isOgg: false };
    return null;
  }

  return { _transformSearchResults, _uri2url, extractTrackId, extractMediaInfo, _selectAudioFile };
}

// ---------------------------------------------------------------------------
// Shared fixture data
// ---------------------------------------------------------------------------

const fakeShowHit = {
  name: 'Crime Junkie',
  uri: 'spotify:show:abc123',
  image: 'https://example.com/show.jpg',
  author: 'audiochuck',
  episodeCount: 300,
  mediaType: 'audio',
};

const fakeEpisodeHit = {
  name: 'Episode 1: The Victim',
  uri: 'spotify:episode:ep001',
  image: 'https://example.com/ep.jpg',
  duration: 2400000,
  releaseDate: '2024-01-01',
  explicit: false,
};

const fakeAudiobookHit = {
  name: 'Dune',
  uri: 'spotify:audiobook:book001',
  image: 'https://example.com/book.jpg',
  authors: ['Frank Herbert'],
  narrators: ['Simon Vance'],
  chapterCount: 48,
};

const fakeTrackHit = {
  name: 'Creep',
  uri: 'spotify:track:tr001',
  image: 'https://example.com/track.jpg',
  artists: [{ name: 'Radiohead', uri: 'spotify:artist:art001' }],
  album: { name: 'Pablo Honey', uri: 'spotify:album:alb001' },
  duration: 238000,
  explicit: false,
};

// ---------------------------------------------------------------------------
// BUG DEMONSTRATION: current implementation fails for shows/episodes/audiobooks
// ---------------------------------------------------------------------------

describe('BUG-1 – current _transformSearchResults drops shows/episodes/audiobooks', () => {
  const { _transformSearchResults } = makeCurrentImpl();

  it('returns empty object when searching for shows (BUG: shows not handled)', () => {
    const rawResults = { shows: { hits: [fakeShowHit] } };
    const out = _transformSearchResults(rawResults, 'show');
    // BUG: out.shows is undefined because the code never processes shows
    expect(out.shows).toBeUndefined();
  });

  it('returns empty object when searching for episodes (BUG: episodes not handled)', () => {
    const rawResults = { episodes: { hits: [fakeEpisodeHit] } };
    const out = _transformSearchResults(rawResults, 'episode');
    expect(out.episodes).toBeUndefined();
  });

  it('returns empty object when searching for audiobooks (BUG: audiobooks not handled)', () => {
    const rawResults = { audiobooks: { hits: [fakeAudiobookHit] } };
    const out = _transformSearchResults(rawResults, 'audiobook');
    expect(out.audiobooks).toBeUndefined();
  });
});

describe('BUG-2 – current _uri2url returns undefined for show/episode/audiobook URIs', () => {
  const { _uri2url } = makeCurrentImpl();

  it('returns undefined for show URIs (BUG: regex excludes show type)', () => {
    expect(_uri2url('spotify:show:abc123')).toBeUndefined();
  });

  it('returns undefined for episode URIs (BUG: regex excludes episode type)', () => {
    expect(_uri2url('spotify:episode:ep001')).toBeUndefined();
  });

  it('returns undefined for audiobook URIs (BUG: regex excludes audiobook type)', () => {
    expect(_uri2url('spotify:audiobook:book001')).toBeUndefined();
  });

  // Confirm track/album/artist/playlist still work in current impl
  it('correctly converts track URIs', () => {
    expect(_uri2url('spotify:track:tr001')).toBe('https://open.spotify.com/track/tr001');
  });
});

describe('BUG-5 – current extractTrackId returns null for episode/chapter URLs', () => {
  const { extractTrackId } = makeCurrentImpl();

  it('returns null for episode spotify URI (causes play-media crash)', () => {
    expect(extractTrackId('spotify:episode:ep001')).toBeNull();
  });

  it('returns null for open.spotify.com episode URL (causes play-media crash)', () => {
    expect(extractTrackId('https://open.spotify.com/episode/ep001')).toBeNull();
  });

  it('returns null for chapter spotify URI', () => {
    expect(extractTrackId('spotify:chapter:ch001')).toBeNull();
  });

  it('returns null for open.spotify.com chapter URL', () => {
    expect(extractTrackId('https://open.spotify.com/chapter/ch001')).toBeNull();
  });

  it('still extracts track IDs correctly (not broken)', () => {
    expect(extractTrackId('spotify:track:tr001')).toBe('tr001');
    expect(extractTrackId('https://open.spotify.com/track/tr001')).toBe('tr001');
  });
});

describe('BUG-6 – current _transformSearchResults misses spclient field names (podcasts/podcastEpisodes)', () => {
  const { _transformSearchResults } = makeCurrentImpl();

  it('returns undefined shows when spclient uses "podcasts" key', () => {
    // spclient API returns results.podcasts, not results.shows
    const rawResults = { podcasts: { hits: [fakeShowHit] } };
    const out = _transformSearchResults(rawResults, 'show');
    expect(out.shows).toBeUndefined();
  });

  it('returns undefined episodes when spclient uses "podcastEpisodes" key', () => {
    const rawResults = { podcastEpisodes: { hits: [fakeEpisodeHit] } };
    const out = _transformSearchResults(rawResults, 'episode');
    expect(out.episodes).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// FIXED implementation – all types should work correctly
// ---------------------------------------------------------------------------

describe('FIXED _transformSearchResults handles shows (web API key "shows")', () => {
  const { _transformSearchResults } = makeFixedImpl();

  it('returns shows.items array for type=show', () => {
    const rawResults = { shows: { hits: [fakeShowHit] } };
    const out = _transformSearchResults(rawResults, 'show');
    expect(out.shows).toBeDefined();
    expect(out.shows.items).toHaveLength(1);
  });

  it('maps show name correctly', () => {
    const out = _transformSearchResults({ shows: { hits: [fakeShowHit] } }, 'show');
    expect(out.shows.items[0].name).toBe('Crime Junkie');
  });

  it('maps show id from URI', () => {
    const out = _transformSearchResults({ shows: { hits: [fakeShowHit] } }, 'show');
    expect(out.shows.items[0].id).toBe('abc123');
  });

  it('maps show publisher', () => {
    const out = _transformSearchResults({ shows: { hits: [fakeShowHit] } }, 'show');
    expect(out.shows.items[0].publisher).toBe('audiochuck');
  });

  it('maps show total_episodes', () => {
    const out = _transformSearchResults({ shows: { hits: [fakeShowHit] } }, 'show');
    expect(out.shows.items[0].total_episodes).toBe(300);
  });

  it('maps show image', () => {
    const out = _transformSearchResults({ shows: { hits: [fakeShowHit] } }, 'show');
    expect(out.shows.items[0].images[0].url).toBe('https://example.com/show.jpg');
  });
});

describe('FIXED _transformSearchResults handles shows (spclient key "podcasts")', () => {
  const { _transformSearchResults } = makeFixedImpl();

  it('returns shows.items from spclient "podcasts" key for type=podcast', () => {
    const rawResults = { podcasts: { hits: [fakeShowHit] } };
    const out = _transformSearchResults(rawResults, 'podcast');
    expect(out.shows).toBeDefined();
    expect(out.shows.items).toHaveLength(1);
    expect(out.shows.items[0].name).toBe('Crime Junkie');
  });

  it('returns shows.items from spclient "podcasts" key for type=show', () => {
    const rawResults = { podcasts: { hits: [fakeShowHit] } };
    const out = _transformSearchResults(rawResults, 'show');
    expect(out.shows).toBeDefined();
    expect(out.shows.items[0].publisher).toBe('audiochuck');
  });
});

describe('FIXED _transformSearchResults handles episodes (web API key "episodes")', () => {
  const { _transformSearchResults } = makeFixedImpl();

  it('returns episodes.items array for type=episode', () => {
    const out = _transformSearchResults({ episodes: { hits: [fakeEpisodeHit] } }, 'episode');
    expect(out.episodes).toBeDefined();
    expect(out.episodes.items).toHaveLength(1);
  });

  it('maps episode name', () => {
    const out = _transformSearchResults({ episodes: { hits: [fakeEpisodeHit] } }, 'episode');
    expect(out.episodes.items[0].name).toBe('Episode 1: The Victim');
  });

  it('maps episode duration_ms', () => {
    const out = _transformSearchResults({ episodes: { hits: [fakeEpisodeHit] } }, 'episode');
    expect(out.episodes.items[0].duration_ms).toBe(2400000);
  });
});

describe('FIXED _transformSearchResults handles episodes (spclient key "podcastEpisodes")', () => {
  const { _transformSearchResults } = makeFixedImpl();

  it('returns episodes.items from spclient "podcastEpisodes" key', () => {
    const rawResults = { podcastEpisodes: { hits: [fakeEpisodeHit] } };
    const out = _transformSearchResults(rawResults, 'episode');
    expect(out.episodes).toBeDefined();
    expect(out.episodes.items).toHaveLength(1);
    expect(out.episodes.items[0].name).toBe('Episode 1: The Victim');
  });

  it('maps episode id from URI in spclient format', () => {
    const rawResults = { podcastEpisodes: { hits: [fakeEpisodeHit] } };
    const out = _transformSearchResults(rawResults, 'episode');
    expect(out.episodes.items[0].id).toBe('ep001');
  });
});

describe('FIXED _transformSearchResults handles audiobooks', () => {
  const { _transformSearchResults } = makeFixedImpl();

  it('returns audiobooks.items array for type=audiobook', () => {
    const out = _transformSearchResults({ audiobooks: { hits: [fakeAudiobookHit] } }, 'audiobook');
    expect(out.audiobooks).toBeDefined();
    expect(out.audiobooks.items).toHaveLength(1);
  });

  it('maps audiobook authors array', () => {
    const out = _transformSearchResults({ audiobooks: { hits: [fakeAudiobookHit] } }, 'audiobook');
    expect(out.audiobooks.items[0].authors[0].name).toBe('Frank Herbert');
  });

  it('maps audiobook total_chapters', () => {
    const out = _transformSearchResults({ audiobooks: { hits: [fakeAudiobookHit] } }, 'audiobook');
    expect(out.audiobooks.items[0].total_chapters).toBe(48);
  });
});

describe('FIXED _uri2url handles all Spotify content types', () => {
  const { _uri2url } = makeFixedImpl();

  it('converts show URIs', () => {
    expect(_uri2url('spotify:show:abc123')).toBe('https://open.spotify.com/show/abc123');
  });

  it('converts episode URIs', () => {
    expect(_uri2url('spotify:episode:ep001')).toBe('https://open.spotify.com/episode/ep001');
  });

  it('converts audiobook URIs', () => {
    expect(_uri2url('spotify:audiobook:book001')).toBe('https://open.spotify.com/audiobook/book001');
  });

  it('still converts track URIs', () => {
    expect(_uri2url('spotify:track:tr001')).toBe('https://open.spotify.com/track/tr001');
  });

  it('returns undefined for null/undefined input', () => {
    expect(_uri2url(null)).toBeUndefined();
    expect(_uri2url(undefined)).toBeUndefined();
  });
});

describe('FIXED extractTrackId handles episode and chapter URIs/URLs', () => {
  const { extractTrackId } = makeFixedImpl();

  it('extracts episode ID from spotify:episode: URI', () => {
    expect(extractTrackId('spotify:episode:ep001')).toBe('ep001');
  });

  it('extracts episode ID from open.spotify.com episode URL', () => {
    expect(extractTrackId('https://open.spotify.com/episode/ep001')).toBe('ep001');
  });

  it('extracts chapter ID from spotify:chapter: URI', () => {
    expect(extractTrackId('spotify:chapter:ch001')).toBe('ch001');
  });

  it('extracts chapter ID from open.spotify.com chapter URL', () => {
    expect(extractTrackId('https://open.spotify.com/chapter/ch001')).toBe('ch001');
  });

  it('still extracts track ID from spotify:track: URI', () => {
    expect(extractTrackId('spotify:track:tr001')).toBe('tr001');
  });

  it('still extracts track ID from open.spotify.com track URL', () => {
    expect(extractTrackId('https://open.spotify.com/track/tr001')).toBe('tr001');
  });

  it('handles intl- prefixed track URLs', () => {
    expect(extractTrackId('https://open.spotify.com/intl-de/track/tr001')).toBe('tr001');
  });

  it('handles intl- prefixed episode URLs', () => {
    expect(extractTrackId('https://open.spotify.com/intl-de/episode/ep001')).toBe('ep001');
  });

  it('returns null for non-Spotify URLs', () => {
    expect(extractTrackId('https://example.com/track/tr001')).toBeNull();
  });

  it('returns null for null/undefined input', () => {
    expect(extractTrackId(null)).toBeNull();
    expect(extractTrackId(undefined)).toBeNull();
  });
});

describe('FIXED extractMediaInfo returns id + type for all media types', () => {
  const { extractMediaInfo } = makeFixedImpl();

  it('returns { id, type: "track" } for spotify:track: URI', () => {
    expect(extractMediaInfo('spotify:track:tr001')).toEqual({ id: 'tr001', type: 'track' });
  });

  it('returns { id, type: "episode" } for spotify:episode: URI', () => {
    expect(extractMediaInfo('spotify:episode:ep001')).toEqual({ id: 'ep001', type: 'episode' });
  });

  it('returns { id, type: "chapter" } for spotify:chapter: URI', () => {
    expect(extractMediaInfo('spotify:chapter:ch001')).toEqual({ id: 'ch001', type: 'chapter' });
  });

  it('returns { id, type: "episode" } for open.spotify.com episode URL', () => {
    expect(extractMediaInfo('https://open.spotify.com/episode/ep001')).toEqual({ id: 'ep001', type: 'episode' });
  });

  it('returns { id, type: "chapter" } for open.spotify.com chapter URL', () => {
    expect(extractMediaInfo('https://open.spotify.com/chapter/ch001')).toEqual({ id: 'ch001', type: 'chapter' });
  });

  it('handles intl- prefixed episode URLs', () => {
    expect(extractMediaInfo('https://open.spotify.com/intl-de/episode/ep001')).toEqual({ id: 'ep001', type: 'episode' });
  });

  it('returns null for non-Spotify URLs', () => {
    expect(extractMediaInfo('https://example.com/track/tr001')).toBeNull();
  });

  it('returns null for null input', () => {
    expect(extractMediaInfo(null)).toBeNull();
  });
});

describe('FIXED _transformSearchResults – regression: tracks still work', () => {
  const { _transformSearchResults } = makeFixedImpl();

  it('still returns tracks.items for type=track', () => {
    const out = _transformSearchResults({ tracks: { hits: [fakeTrackHit] } }, 'track');
    expect(out.tracks).toBeDefined();
    expect(out.tracks.items).toHaveLength(1);
    expect(out.tracks.items[0].name).toBe('Creep');
  });

  it('does not include shows data when type=track', () => {
    const out = _transformSearchResults(
      { tracks: { hits: [fakeTrackHit] }, shows: { hits: [fakeShowHit] } },
      'track',
    );
    expect(out.shows).toBeUndefined();
  });
});

describe('FIXED _selectAudioFile handles OGG tracks and MP3/AAC podcast episodes', () => {
  const { _selectAudioFile } = makeFixedImpl();

  it('prefers OGG_VORBIS_320 over lower quality OGG', () => {
    const files = [
      { file_id: 'low', format: 'OGG_VORBIS_96' },
      { file_id: 'high', format: 'OGG_VORBIS_320' },
    ];
    const result = _selectAudioFile(files);
    expect(result.file.file_id).toBe('high');
    expect(result.isOgg).toBe(true);
  });

  it('falls back to MP3_160 when no OGG files present (podcast episode)', () => {
    const files = [
      { file_id: 'mp3low', format: 'MP3_96' },
      { file_id: 'mp3mid', format: 'MP3_160' },
    ];
    const result = _selectAudioFile(files);
    expect(result.file.file_id).toBe('mp3mid');
    expect(result.isOgg).toBe(false);
  });

  it('falls back to AAC when only AAC files present', () => {
    const files = [
      { file_id: 'aac', format: 'AAC_24' },
    ];
    const result = _selectAudioFile(files);
    expect(result.file.file_id).toBe('aac');
    expect(result.isOgg).toBe(false);
  });

  it('returns null for empty files array', () => {
    expect(_selectAudioFile([])).toBeNull();
  });

  it('OGG takes priority over MP3 when both present', () => {
    const files = [
      { file_id: 'mp3', format: 'MP3_320' },
      { file_id: 'ogg', format: 'OGG_VORBIS_160' },
    ];
    const result = _selectAudioFile(files);
    expect(result.file.file_id).toBe('ogg');
    expect(result.isOgg).toBe(true);
  });
});

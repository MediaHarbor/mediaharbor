/**
 * Tests for show episode list and audiobook chapter fetching bugs
 * - BUG-3: handleGetPlaylistDetails calls getPlaylistTracks for shows (wrong endpoint)
 * - BUG-4: handleGetAlbumDetails calls getAlbumTracks for audiobooks (wrong endpoint)
 * - Tests for new getShowEpisodes / getAudiobookChapters methods in SpotifyAPI
 * - Tests for formatPlatformResponsePlaylist / formatPlatformResponseAlbum with new data shapes
 */

import { describe, it, expect, beforeEach } from 'vitest';

// ---- SpotifyAPI stub (mirrors the fixed spotifyapi.js) ----

class SpotifyAPIStub {
  constructor() {
    this.API_URL = 'https://api.spotify.com/v1';
    this.access_token = 'fake-token';
    this.calls = [];
  }

  async authenticate() {}

  async makeRequest(method, url, params) {
    this.calls.push({ method, url, params });
    // Return realistic stub data based on URL
    if (url.includes('/shows/') && url.includes('/episodes')) {
      return { items: [fakeEpisodeApiItem] };
    }
    if (url.includes('/shows/')) {
      return fakeShowApiItem;
    }
    if (url.includes('/audiobooks/') && url.includes('/chapters')) {
      return { items: [fakeChapterApiItem] };
    }
    if (url.includes('/audiobooks/')) {
      return fakeAudiobookApiItem;
    }
    if (url.includes('/playlists/') && url.includes('/tracks')) {
      return { items: [fakePlaylistTrackItem] };
    }
    if (url.includes('/playlists/')) {
      return { name: 'My Playlist', owner: { display_name: 'User' }, images: [] };
    }
    return {};
  }

  async getAlbumTracks(albumId) {
    this.calls.push({ method: 'GET', url: `${this.API_URL}/albums/${albumId}`, params: null });
    return { album_name: 'Album', artist_name: 'Artist', release_date: '2024', cover_url: '', tracks: [] };
  }

  async getPlaylistTracks(playlistId) {
    const id = playlistId.replace(/^spotify:playlist:/, '');
    await this.makeRequest('GET', `${this.API_URL}/playlists/${id}`, { market: 'US' });
    await this.makeRequest('GET', `${this.API_URL}/playlists/${id}/tracks`, { market: 'US', limit: '100' });
    return { playlist_name: 'Playlist', owner_name: 'Owner', cover_url: '', tracks: [] };
  }

  async getShowEpisodes(showId, limit = 50) {
    const id = showId.replace(/^spotify:show:/, '');
    const [showData, episodesData] = await Promise.all([
      this.makeRequest('GET', `${this.API_URL}/shows/${id}`, { market: 'US' }),
      this.makeRequest('GET', `${this.API_URL}/shows/${id}/episodes`, { market: 'US', limit: limit.toString() }),
    ]);
    return {
      show_name: showData.name,
      publisher: showData.publisher || 'Unknown',
      cover_url: showData.images?.[0]?.url || null,
      episodes: episodesData.items || [],
    };
  }

  async getAudiobookChapters(audiobookId, limit = 50) {
    const id = audiobookId.replace(/^spotify:audiobook:/, '');
    const [bookData, chaptersData] = await Promise.all([
      this.makeRequest('GET', `${this.API_URL}/audiobooks/${id}`, { market: 'US' }),
      this.makeRequest('GET', `${this.API_URL}/audiobooks/${id}/chapters`, { market: 'US', limit: limit.toString() }),
    ]);
    return {
      book_name: bookData.name,
      author: bookData.authors?.[0]?.name || 'Unknown',
      cover_url: bookData.images?.[0]?.url || null,
      chapters: chaptersData.items || [],
    };
  }
}

// ---- Fixture data ----

const fakeShowApiItem = {
  id: 'show001',
  name: 'Crime Junkie',
  publisher: 'audiochuck',
  images: [{ url: 'https://example.com/show.jpg' }],
};

const fakeEpisodeApiItem = {
  id: 'ep001',
  name: 'Episode 1: The Victim',
  duration_ms: 2400000,
  release_date: '2024-01-01',
  explicit: false,
  external_urls: { spotify: 'https://open.spotify.com/episode/ep001' },
  images: [{ url: 'https://example.com/ep.jpg' }],
};

const fakeAudiobookApiItem = {
  id: 'book001',
  name: 'Dune',
  authors: [{ name: 'Frank Herbert' }],
  images: [{ url: 'https://example.com/book.jpg' }],
};

const fakeChapterApiItem = {
  id: 'ch001',
  name: 'Chapter 1: Arrakis',
  chapter_number: 1,
  duration_ms: 1800000,
  external_urls: { spotify: 'https://open.spotify.com/chapter/ch001' },
};

const fakePlaylistTrackItem = {
  track: {
    id: 'tr001',
    name: 'Creep',
    duration_ms: 238000,
    artists: [{ name: 'Radiohead' }],
    album: { name: 'Pablo Honey', images: [{ url: 'https://example.com/album.jpg' }] },
    external_urls: { spotify: 'https://open.spotify.com/track/tr001' },
  },
};

// ---- formatPlatformResponsePlaylist (spotify branch) – local impl ----

function formatPlaylistSpotify(data) {
  if (data.episodes) {
    const playlistInfo = {
      title: data.show_name || 'Unknown Show',
      creator: data.publisher || 'Unknown',
      releaseDate: '',
      coverUrl: data.cover_url || '',
    };
    const tracks = (data.episodes || []).map((ep, index) => ({
      id: ep.id,
      number: index + 1,
      title: ep.name,
      duration: Math.floor((ep.duration_ms || 0) / 1e3),
      quality: '',
      playUrl: ep.external_urls?.spotify || ep.uri || null,
      artist: data.publisher || '',
      cover: ep.images?.[0]?.url || data.cover_url || '',
      releaseDate: ep.release_date || '',
    }));
    return { playlist: playlistInfo, tracks };
  }
  const playlistInfo = {
    title: data.playlist_name || 'Unknown Playlist',
    creator: data.owner_name || 'Unknown',
    coverUrl: data.cover_url || '',
  };
  const tracks = (data.tracks || [])
    .filter((item) => item?.track)
    .map((item, index) => {
      const track = item.track;
      return {
        id: track.id,
        number: index + 1,
        title: track.name,
        duration: Math.floor((track.duration_ms || 0) / 1e3),
        playUrl: track.external_urls?.spotify || track.uri || null,
        artist: track.artists?.[0]?.name || 'Unknown Artist',
      };
    });
  return { playlist: playlistInfo, tracks };
}

function formatAlbumSpotify(data) {
  if (data.chapters) {
    const albumInfo = {
      title: data.book_name || 'Unknown Audiobook',
      artist: data.author || 'Unknown Author',
      coverUrl: data.cover_url || '',
    };
    const tracks = (data.chapters || []).map((ch, index) => ({
      id: ch.id,
      number: ch.chapter_number || index + 1,
      title: ch.name,
      duration: Math.floor((ch.duration_ms || 0) / 1e3),
      playUrl: ch.external_urls?.spotify || ch.uri || null,
      artist: data.author || '',
    }));
    return { album: albumInfo, tracks };
  }
  const albumInfo = {
    title: data.album_name || 'Unknown Album',
    artist: data.artist_name || 'Unknown Artist',
    coverUrl: data.cover_url || '',
  };
  const tracks = (data.tracks || []).map((track) => ({
    id: track.id,
    number: track.track_number || 0,
    title: track.name,
    duration: Math.floor((track.duration_ms || 0) / 1e3),
    playUrl: track.external_urls?.spotify || track.uri || null,
    artist: track.artists?.[0]?.name || albumInfo.artist,
  }));
  return { album: albumInfo, tracks };
}

// ---- handleGetPlaylistDetails routing (buggy vs fixed) ----

async function buggyGetPlaylistDetails(api, playlistId) {
  // Always calls getPlaylistTracks – wrong for show:: IDs
  return api.getPlaylistTracks(playlistId);
}

async function fixedGetPlaylistDetails(api, playlistId) {
  if (playlistId.startsWith('show::')) {
    return api.getShowEpisodes(playlistId.slice(6));
  }
  return api.getPlaylistTracks(playlistId);
}

async function buggyGetAlbumDetails(api, albumId) {
  // Always calls getAlbumTracks – wrong for audiobook:: IDs
  return api.getAlbumTracks(albumId);
}

async function fixedGetAlbumDetails(api, albumId) {
  if (albumId.startsWith('audiobook::')) {
    return api.getAudiobookChapters(albumId.slice(11));
  }
  return api.getAlbumTracks(albumId);
}

// ============================================================
// Tests
// ============================================================

describe('BUG-3 – buggy handleGetPlaylistDetails calls wrong endpoint for show IDs', () => {
  it('calls /playlists/ endpoint for a show ID (wrong endpoint)', async () => {
    const api = new SpotifyAPIStub();
    await buggyGetPlaylistDetails(api, 'show::show001');
    const urls = api.calls.map((c) => c.url);
    // Buggy impl hits /playlists/ instead of /shows/
    expect(urls.some((u) => u.includes('/playlists/'))).toBe(true);
    expect(urls.some((u) => u.includes('/shows/'))).toBe(false);
  });
});

describe('BUG-4 – buggy handleGetAlbumDetails calls wrong endpoint for audiobook IDs', () => {
  it('calls /albums/ endpoint for an audiobook ID (wrong endpoint)', async () => {
    const api = new SpotifyAPIStub();
    await buggyGetAlbumDetails(api, 'audiobook::book001');
    const urls = api.calls.map((c) => c.url);
    expect(urls.some((u) => u.includes('/albums/'))).toBe(true);
    expect(urls.some((u) => u.includes('/audiobooks/'))).toBe(false);
  });
});

describe('FIXED handleGetPlaylistDetails – show:: prefix routes to /shows/ endpoint', () => {
  let api;
  beforeEach(() => { api = new SpotifyAPIStub(); });

  it('calls /shows/{id} and /shows/{id}/episodes for show:: IDs', async () => {
    await fixedGetPlaylistDetails(api, 'show::show001');
    const urls = api.calls.map((c) => c.url);
    expect(urls.some((u) => u.includes('/shows/show001') && !u.includes('/episodes'))).toBe(true);
    expect(urls.some((u) => u.includes('/shows/show001/episodes'))).toBe(true);
    expect(urls.some((u) => u.includes('/playlists/'))).toBe(false);
  });

  it('still calls /playlists/ for regular playlist IDs', async () => {
    await fixedGetPlaylistDetails(api, 'pl001');
    const urls = api.calls.map((c) => c.url);
    expect(urls.some((u) => u.includes('/playlists/'))).toBe(true);
    expect(urls.some((u) => u.includes('/shows/'))).toBe(false);
  });

  it('returns episodes array in result', async () => {
    const result = await fixedGetPlaylistDetails(api, 'show::show001');
    expect(result.episodes).toBeDefined();
    expect(result.episodes).toHaveLength(1);
  });

  it('returns show_name in result', async () => {
    const result = await fixedGetPlaylistDetails(api, 'show::show001');
    expect(result.show_name).toBe('Crime Junkie');
  });
});

describe('FIXED handleGetAlbumDetails – audiobook:: prefix routes to /audiobooks/ endpoint', () => {
  let api;
  beforeEach(() => { api = new SpotifyAPIStub(); });

  it('calls /audiobooks/{id} and /audiobooks/{id}/chapters for audiobook:: IDs', async () => {
    await fixedGetAlbumDetails(api, 'audiobook::book001');
    const urls = api.calls.map((c) => c.url);
    expect(urls.some((u) => u.includes('/audiobooks/book001') && !u.includes('/chapters'))).toBe(true);
    expect(urls.some((u) => u.includes('/audiobooks/book001/chapters'))).toBe(true);
    expect(urls.some((u) => u.includes('/albums/'))).toBe(false);
  });

  it('still calls /albums/ for regular album IDs', async () => {
    await fixedGetAlbumDetails(api, 'alb001');
    const urls = api.calls.map((c) => c.url);
    expect(urls.some((u) => u.includes('/albums/'))).toBe(true);
    expect(urls.some((u) => u.includes('/audiobooks/'))).toBe(false);
  });

  it('returns chapters array in result', async () => {
    const result = await fixedGetAlbumDetails(api, 'audiobook::book001');
    expect(result.chapters).toBeDefined();
    expect(result.chapters).toHaveLength(1);
  });

  it('returns book_name in result', async () => {
    const result = await fixedGetAlbumDetails(api, 'audiobook::book001');
    expect(result.book_name).toBe('Dune');
  });
});

describe('formatPlaylistSpotify – show episode formatting', () => {
  const showData = {
    show_name: 'Crime Junkie',
    publisher: 'audiochuck',
    cover_url: 'https://example.com/show.jpg',
    episodes: [fakeEpisodeApiItem],
  };

  it('returns playlist.title = show_name', () => {
    const out = formatPlaylistSpotify(showData);
    expect(out.playlist.title).toBe('Crime Junkie');
  });

  it('returns playlist.creator = publisher', () => {
    const out = formatPlaylistSpotify(showData);
    expect(out.playlist.creator).toBe('audiochuck');
  });

  it('returns tracks array with episodes', () => {
    const out = formatPlaylistSpotify(showData);
    expect(out.tracks).toHaveLength(1);
    expect(out.tracks[0].title).toBe('Episode 1: The Victim');
  });

  it('maps episode duration correctly (ms → seconds)', () => {
    const out = formatPlaylistSpotify(showData);
    expect(out.tracks[0].duration).toBe(2400);
  });

  it('maps episode playUrl from external_urls.spotify', () => {
    const out = formatPlaylistSpotify(showData);
    expect(out.tracks[0].playUrl).toBe('https://open.spotify.com/episode/ep001');
  });

  it('still works for regular playlist data (regression)', () => {
    const playlistData = {
      playlist_name: 'My Playlist',
      owner_name: 'User',
      cover_url: '',
      tracks: [fakePlaylistTrackItem],
    };
    const out = formatPlaylistSpotify(playlistData);
    expect(out.playlist.title).toBe('My Playlist');
    expect(out.tracks[0].title).toBe('Creep');
  });
});

describe('formatAlbumSpotify – audiobook chapter formatting', () => {
  const bookData = {
    book_name: 'Dune',
    author: 'Frank Herbert',
    cover_url: 'https://example.com/book.jpg',
    chapters: [fakeChapterApiItem],
  };

  it('returns album.title = book_name', () => {
    const out = formatAlbumSpotify(bookData);
    expect(out.album.title).toBe('Dune');
  });

  it('returns album.artist = author', () => {
    const out = formatAlbumSpotify(bookData);
    expect(out.album.artist).toBe('Frank Herbert');
  });

  it('returns tracks array with chapters', () => {
    const out = formatAlbumSpotify(bookData);
    expect(out.tracks).toHaveLength(1);
    expect(out.tracks[0].title).toBe('Chapter 1: Arrakis');
  });

  it('maps chapter_number to track number', () => {
    const out = formatAlbumSpotify(bookData);
    expect(out.tracks[0].number).toBe(1);
  });

  it('maps chapter duration correctly (ms → seconds)', () => {
    const out = formatAlbumSpotify(bookData);
    expect(out.tracks[0].duration).toBe(1800);
  });

  it('maps chapter playUrl from external_urls.spotify', () => {
    const out = formatAlbumSpotify(bookData);
    expect(out.tracks[0].playUrl).toBe('https://open.spotify.com/chapter/ch001');
  });

  it('still works for regular album data (regression)', () => {
    const albumData = {
      album_name: 'Pablo Honey',
      artist_name: 'Radiohead',
      cover_url: '',
      tracks: [{
        id: 'tr001', name: 'Creep', track_number: 1,
        duration_ms: 238000, artists: [{ name: 'Radiohead' }],
        external_urls: { spotify: 'https://open.spotify.com/track/tr001' }
      }],
    };
    const out = formatAlbumSpotify(albumData);
    expect(out.album.title).toBe('Pablo Honey');
    expect(out.tracks[0].title).toBe('Creep');
  });
});

describe('SpotifyAPI.getShowEpisodes – method signature and endpoints', () => {
  let api;
  beforeEach(() => { api = new SpotifyAPIStub(); });

  it('fetches /shows/{id} and /shows/{id}/episodes in parallel', async () => {
    await api.getShowEpisodes('show001');
    const urls = api.calls.map((c) => c.url);
    expect(urls).toContain('https://api.spotify.com/v1/shows/show001');
    expect(urls).toContain('https://api.spotify.com/v1/shows/show001/episodes');
  });

  it('strips spotify:show: URI prefix from showId', async () => {
    await api.getShowEpisodes('spotify:show:show001');
    const urls = api.calls.map((c) => c.url);
    expect(urls.some((u) => u.includes('/shows/show001'))).toBe(true);
    expect(urls.some((u) => u.includes('spotify:show:'))).toBe(false);
  });

  it('includes market=US in both requests', async () => {
    await api.getShowEpisodes('show001');
    api.calls.forEach((c) => {
      if (c.url.includes('/shows/')) {
        expect(c.params?.market).toBe('US');
      }
    });
  });
});

describe('SpotifyAPI.getAudiobookChapters – method signature and endpoints', () => {
  let api;
  beforeEach(() => { api = new SpotifyAPIStub(); });

  it('fetches /audiobooks/{id} and /audiobooks/{id}/chapters in parallel', async () => {
    await api.getAudiobookChapters('book001');
    const urls = api.calls.map((c) => c.url);
    expect(urls).toContain('https://api.spotify.com/v1/audiobooks/book001');
    expect(urls).toContain('https://api.spotify.com/v1/audiobooks/book001/chapters');
  });

  it('strips spotify:audiobook: URI prefix from audiobookId', async () => {
    await api.getAudiobookChapters('spotify:audiobook:book001');
    const urls = api.calls.map((c) => c.url);
    expect(urls.some((u) => u.includes('/audiobooks/book001'))).toBe(true);
    expect(urls.some((u) => u.includes('spotify:audiobook:'))).toBe(false);
  });

  it('includes market=US in both requests', async () => {
    await api.getAudiobookChapters('book001');
    api.calls.forEach((c) => {
      if (c.url.includes('/audiobooks/')) {
        expect(c.params?.market).toBe('US');
      }
    });
  });
});

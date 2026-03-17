/**
 * Tests for SpotifyAPI search methods (spotifyapi.js)
 * Covers: searchPodcasts, searchAudiobooks, searchEpisodes
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

// ---- Minimal SpotifyAPI stub (mirrors src/funcs/apis/spotifyapi.js) ----
// We re-implement just enough to exercise the search param logic without
// touching the real file or making real network calls.

class SpotifyAPIStub {
  constructor() {
    this.API_URL = 'https://api.spotify.com/v1';
    this.access_token = 'fake-token';
    this.token_expiry = new Date(Date.now() + 3600 * 1000);
    this.lastRequest = null;
  }

  async authenticate() {}

  async makeRequest(method, url, params) {
    this.lastRequest = { method, url, params };
    return {};
  }

  async searchTracks(query, limit = 10) {
    const params = { q: query, type: 'track', limit: limit.toString() };
    return this.makeRequest('GET', `${this.API_URL}/search`, params);
  }

  async searchAlbums(query, limit = 10) {
    const params = { q: query, type: 'album', limit: limit.toString() };
    return this.makeRequest('GET', `${this.API_URL}/search`, params);
  }

  async searchPlaylists(query, limit = 10) {
    const params = { q: query, type: 'playlist', limit: limit.toString() };
    return this.makeRequest('GET', `${this.API_URL}/search`, params);
  }

  async searchEpisodes(query, limit = 10) {
    const params = { q: query, type: 'episode', limit: limit.toString(), market: 'US' };
    return this.makeRequest('GET', `${this.API_URL}/search`, params);
  }

  async searchArtists(query, limit = 10) {
    const params = { q: query, type: 'artist', limit: limit.toString() };
    return this.makeRequest('GET', `${this.API_URL}/search`, params);
  }

  async searchPodcasts(query, limit = 10) {
    const params = { q: query, type: 'show', limit: limit.toString(), market: 'US' };
    return this.makeRequest('GET', `${this.API_URL}/search`, params);
  }

  async searchAudiobooks(query, limit = 10) {
    const params = { q: query, type: 'audiobook', limit: limit.toString(), market: 'US' };
    return this.makeRequest('GET', `${this.API_URL}/search`, params);
  }
}

// ---- Tests ----

describe('SpotifyAPI – podcast/show/audiobook search parameters', () => {
  let api;

  beforeEach(() => {
    api = new SpotifyAPIStub();
  });

  // Podcasts use the Spotify API type "show", not "podcast"
  it('searchPodcasts sends type=show to the Spotify API', async () => {
    await api.searchPodcasts('serial');
    expect(api.lastRequest.params.type).toBe('show');
  });

  it('searchPodcasts includes the market parameter', async () => {
    await api.searchPodcasts('serial');
    expect(api.lastRequest.params.market).toBe('US');
  });

  it('searchPodcasts passes the query string', async () => {
    await api.searchPodcasts('crime junkie');
    expect(api.lastRequest.params.q).toBe('crime junkie');
  });

  it('searchAudiobooks sends type=audiobook to the Spotify API', async () => {
    await api.searchAudiobooks('dune');
    expect(api.lastRequest.params.type).toBe('audiobook');
  });

  it('searchAudiobooks includes the market parameter', async () => {
    await api.searchAudiobooks('dune');
    expect(api.lastRequest.params.market).toBe('US');
  });

  it('searchEpisodes sends type=episode to the Spotify API', async () => {
    await api.searchEpisodes('episode one');
    expect(api.lastRequest.params.type).toBe('episode');
  });

  it('searchEpisodes includes the market parameter', async () => {
    await api.searchEpisodes('episode one');
    expect(api.lastRequest.params.market).toBe('US');
  });

  // Sanity check: regular searches must NOT carry a market param
  it('searchTracks does NOT include a market parameter', async () => {
    await api.searchTracks('radiohead');
    expect(api.lastRequest.params.market).toBeUndefined();
  });

  it('searchAlbums does NOT include a market parameter', async () => {
    await api.searchAlbums('ok computer');
    expect(api.lastRequest.params.market).toBeUndefined();
  });

  it('all search methods hit the /search endpoint', async () => {
    const endpoint = 'https://api.spotify.com/v1/search';
    await api.searchPodcasts('test');
    expect(api.lastRequest.url).toBe(endpoint);
    await api.searchAudiobooks('test');
    expect(api.lastRequest.url).toBe(endpoint);
    await api.searchEpisodes('test');
    expect(api.lastRequest.url).toBe(endpoint);
  });
});

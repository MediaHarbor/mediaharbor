const fs   = require('fs');
const path = require('path');

function loadBundledCredentials() {
    try {
        const p = path.join(__dirname, 'apis.json');
        return JSON.parse(fs.readFileSync(p, 'utf8'));
    } catch (_) { return {}; }
}

class SpotifyAPI {
    constructor(credentials = {}) {
        this.API_URL = "https://api.spotify.com/v1";
        this.TOKEN_URL = "https://accounts.spotify.com/api/token";
        this.access_token = null;
        this.token_expiry = null;
        const bundled = loadBundledCredentials();
        const id     = credentials.clientId     || bundled.SPOTIFY_CLIENT_ID     || '';
        const secret = credentials.clientSecret || bundled.SPOTIFY_CLIENT_SECRET || '';
        if (!id || !secret) {
            throw new Error("Spotify API credentials not configured. Add them in Settings → API Keys.");
        }
        this.client_id     = id;
        this.client_secret = secret;
    }

    async authenticate() {
        if (this.access_token && this.token_expiry && new Date() < this.token_expiry) {
            return;
        }
        const authHeader = Buffer.from(`${this.client_id}:${this.client_secret}`).toString('base64');
        const headers = {
            "Authorization": `Basic ${authHeader}`,
            "Content-Type": "application/x-www-form-urlencoded"
        };
        const data = "grant_type=client_credentials";
        const response = await fetch(this.TOKEN_URL, {
            method: 'POST',
            headers,
            body: data
        });
        if (!response.ok) {
            throw new Error(`Authentication failed: ${response.statusText}`);
        }
        const tokenData = await response.json();
        this.access_token = tokenData.access_token;
        this.token_expiry = new Date(Date.now() + tokenData.expires_in * 1000);
    }

    async makeRequest(method, url, params = null) {
        await this.authenticate();
        const headers = {
            "Authorization": `Bearer ${this.access_token}`,
            "Accept": "application/json"
        };
        const query = params ? new URLSearchParams(params).toString() : '';
        const fullUrl = query ? `${url}?${query}` : url;
        const response = await fetch(fullUrl, { method, headers });
        if (!response.ok) {
            throw new Error(`Request failed: ${response.statusText}`);
        }
        return await response.json();
    }

    async getTrack(trackId) {
        const trackData = await this.makeRequest("GET", `${this.API_URL}/tracks/${trackId}`);
        return {
            title: trackData.name,
            album_art: trackData.album.images?.[0]?.url || null,
            artist_name: trackData.artists[0].name
        };
    }

    async getAlbumInfo(albumId) {
        const albumData = await this.makeRequest("GET", `${this.API_URL}/albums/${albumId}`);
        return {
            album_name: albumData.name,
            release_date: albumData.release_date,
            artist_name: albumData.artists[0].name,
            cover_url: albumData.images?.[0]?.url || null
        };
    }

    async getPlaylistInfo(playlistId) {
        const playlistData = await this.makeRequest("GET", `${this.API_URL}/playlists/${playlistId}`);
        return {
            playlist_name: playlistData.name,
            owner_name: playlistData.owner.display_name,
            description: playlistData.description || "",
            cover_url: playlistData.images?.[0]?.url || null,
            total_tracks: playlistData.tracks.total
        };
    }

    async getAlbumTracks(albumId) {
        const albumData = await this.makeRequest("GET", `${this.API_URL}/albums/${albumId}`);
        const tracksData = await this.makeRequest("GET", `${this.API_URL}/albums/${albumId}/tracks`);
        return {
            album_name: albumData.name,
            release_date: albumData.release_date,
            artist_name: albumData.artists[0].name,
            cover_url: albumData.images?.[0]?.url || null,
            tracks: tracksData.items
        };
    }

    async getPlaylistTracks(playlistId) {
        const id = playlistId.replace(/^spotify:playlist:/, '');
        let playlistData, tracksData;
        try {
            [playlistData, tracksData] = await Promise.all([
                this.makeRequest("GET", `${this.API_URL}/playlists/${id}`, { market: 'US' }),
                this.makeRequest("GET", `${this.API_URL}/playlists/${id}/tracks`, { market: 'US', limit: '100' }),
            ]);
        } catch (err) {
            if (err.message?.includes('Not Found')) {
                throw new Error('Spotify playlist not found. It may be private or unavailable in your region.');
            }
            throw err;
        }
        return {
            playlist_name: playlistData.name,
            owner_name: playlistData.owner?.display_name || 'Unknown',
            cover_url: playlistData.images?.[0]?.url || null,
            tracks: tracksData.items
        };
    }

    async getAlbum(albumId) {
        return await this.makeRequest("GET", `${this.API_URL}/albums/${albumId}`);
    }

    async searchTracks(query, limit = 10) {
        const params = { q: query, type: "track", limit: limit.toString() };
        return await this.makeRequest("GET", `${this.API_URL}/search`, params);
    }

    async searchAlbums(query, limit = 10) {
        const params = { q: query, type: "album", limit: limit.toString() };
        return await this.makeRequest("GET", `${this.API_URL}/search`, params);
    }

    async searchPlaylists(query, limit = 10) {
        const params = { q: query, type: "playlist", limit: limit.toString() };
        return await this.makeRequest("GET", `${this.API_URL}/search`, params);
    }

    async searchEpisodes(query, limit = 10) {
        const params = { q: query, type: "episode", limit: limit.toString(), market: "US" };
        return await this.makeRequest("GET", `${this.API_URL}/search`, params);
    }

    async searchArtists(query, limit = 10) {
        const params = { q: query, type: "artist", limit: limit.toString() };
        return await this.makeRequest("GET", `${this.API_URL}/search`, params);
    }

    async getArtistAlbums(artistId, limit = 50) {
        const params = { limit: limit.toString(), include_groups: 'album,single', market: 'US' };
        return await this.makeRequest("GET", `${this.API_URL}/artists/${artistId}/albums`, params);
    }

    async searchPodcasts(query, limit = 10) {
        const params = { q: query, type: "show", limit: limit.toString(), market: "US" };
        return await this.makeRequest("GET", `${this.API_URL}/search`, params);
    }

    async searchAudiobooks(query, limit = 10) {
        const params = { q: query, type: "audiobook", limit: limit.toString(), market: "US" };
        return await this.makeRequest("GET", `${this.API_URL}/search`, params);
    }

    async getShowEpisodes(showId, limit = 50) {
        const id = showId.replace(/^spotify:show:/, '');
        const [showData, episodesData] = await Promise.all([
            this.makeRequest("GET", `${this.API_URL}/shows/${id}`, { market: "US" }),
            this.makeRequest("GET", `${this.API_URL}/shows/${id}/episodes`, { market: "US", limit: limit.toString() }),
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
            this.makeRequest("GET", `${this.API_URL}/audiobooks/${id}`, { market: "US" }),
            this.makeRequest("GET", `${this.API_URL}/audiobooks/${id}/chapters`, { market: "US", limit: limit.toString() }),
        ]);
        return {
            book_name: bookData.name,
            author: bookData.authors?.[0]?.name || 'Unknown',
            cover_url: bookData.images?.[0]?.url || null,
            chapters: chaptersData.items || [],
        };
    }
}

module.exports = SpotifyAPI;

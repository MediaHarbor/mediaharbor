const fs   = require('fs');
const path = require('path');

function loadBundledCredentials() {
    try {
        const p = path.join(__dirname, 'apis.json');
        return JSON.parse(fs.readFileSync(p, 'utf8'));
    } catch (_) { return {}; }
}

class TidalAPI {
    constructor(credentials = {}) {
        this.API_URL = "https://openapi.tidal.com/v2";
        this.TOKEN_URL = "https://auth.tidal.com/v1/oauth2/token";
        this.STREAM_API_URL = "https://api.tidal.com/v1";
        this.access_token = null;
        this.token_expiry = null;
        const bundled = loadBundledCredentials();
        const id     = credentials.clientId     || bundled.TIDAL_CLIENT_ID     || '';
        const secret = credentials.clientSecret || bundled.TIDAL_CLIENT_SECRET || '';
        if (!id || !secret) {
            throw new Error("Tidal API credentials not configured. Add them in Settings → API Keys.");
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

    async makeRequest(method, url, params = null, baseUrl = null) {
        await this.authenticate();
        const headers = {
            "Authorization": `Bearer ${this.access_token}`,
            "Accept": "application/vnd.tidal.v1+json"
        };
        const fullUrl = `${baseUrl || this.API_URL}/${url}`;
        const query = params ? new URLSearchParams(params).toString() : '';
        const finalUrl = query ? `${fullUrl}?${query}` : fullUrl;
        const response = await fetch(finalUrl, { method, headers });
        if (!response.ok) {
            throw new Error(`Request failed: ${response.statusText}`);
        }
        return await response.json();
    }

    async searchRequest(params) {
        await this.authenticate();

        const { query, type, countryCode, limit } = params;

        const headers = {
            "Authorization": `Bearer ${this.access_token}`,
            "Accept": "application/vnd.api+json"
        };

        const includeTypes = {
            'TRACKS': 'tracks,albums,artists',
            'ALBUMS': 'albums,artists',
            'ARTISTS': 'artists',
            'PLAYLISTS': 'playlists,artists',
            'VIDEOS': 'videos,artists'
        };

        const includeParam = includeTypes[type] || 'tracks';
        const queryParams = new URLSearchParams({
            countryCode,
            include: includeParam
        });

        const finalUrl = `${this.API_URL}/searchResults/${encodeURIComponent(query)}?${queryParams.toString()}`;

        const response = await fetch(finalUrl, { method: 'GET', headers });

        if (!response.ok) {
            const errorText = await response.text();
            throw new Error(`Request failed: ${response.status} ${response.statusText}`);
        }

        const data = await response.json();
        const transformedData = this.transformSearchResponse(data, includeParam);

        return transformedData;
    }

    transformSearchResponse(apiResponse, includeType) {
        const results = apiResponse.included || [];

        const includedMap = {};
        results.forEach(item => {
            if (!includedMap[item.type]) includedMap[item.type] = {};
            includedMap[item.type][item.id] = item;
        });

        const grouped = {
            tracks: [],
            albums: [],
            artists: [],
            playlists: [],
            videos: []
        };

        results.forEach(item => {
            if (item.type === 'tracks') {
                if (item.relationships && item.relationships.albums && item.relationships.albums.data && item.relationships.albums.data.length > 0) {
                    const albumId = item.relationships.albums.data[0].id;
                    const album = includedMap.albums && includedMap.albums[albumId];
                    if (album) {
                        item.album = album.attributes;
                    }
                }
                if (item.relationships && item.relationships.artists && item.relationships.artists.data && item.relationships.artists.data.length > 0) {
                    item.artists = item.relationships.artists.data.map(artistRef => {
                        const artist = includedMap.artists && includedMap.artists[artistRef.id];
                        return artist ? artist.attributes : null;
                    }).filter(Boolean);
                }
                grouped.tracks.push({ resource: item });
            } else if (item.type === 'albums') {
                if (item.relationships && item.relationships.artists && item.relationships.artists.data && item.relationships.artists.data.length > 0) {
                    const artistId = item.relationships.artists.data[0].id;
                    const artist = includedMap.artists && includedMap.artists[artistId];
                    if (artist) {
                        item.artist = artist.attributes;
                    }
                }
                grouped.albums.push({ resource: item });
            } else if (item.type === 'artists') {
                grouped.artists.push({ resource: item });
            } else if (item.type === 'playlists') {
                if (item.relationships && item.relationships.artists && item.relationships.artists.data && item.relationships.artists.data.length > 0) {
                    item.artists = item.relationships.artists.data.map(artistRef => {
                        const artist = includedMap.artists && includedMap.artists[artistRef.id];
                        return artist ? artist.attributes : null;
                    }).filter(Boolean);
                }
                grouped.playlists.push({ resource: item });
            } else if (item.type === 'videos') {
                if (item.relationships && item.relationships.artists && item.relationships.artists.data && item.relationships.artists.data.length > 0) {
                    item.artists = item.relationships.artists.data.map(artistRef => {
                        const artist = includedMap.artists && includedMap.artists[artistRef.id];
                        return artist ? artist.attributes : null;
                    }).filter(Boolean);
                }
                grouped.videos.push({ resource: item });
            }
        });

        return grouped;
    }

    async getStreamUrl(trackId, countryCode, userId = null, userToken = null) {
        const headers = {
            "Authorization": `Bearer ${userToken || this.access_token}`
        };
        const params = { countryCode };
        const url = `${this.STREAM_API_URL}/tracks/${trackId}/streamUrl`;
        const query = new URLSearchParams(params).toString();
        const finalUrl = `${url}?${query}`;
        const response = await fetch(finalUrl, { headers });
        if (!response.ok) {
            throw new Error(`Request failed: ${response.statusText}`);
        }
        return await response.json();
    }

    async getTrack(trackId, countryCode) {
        const params = { countryCode, include: "artists,albums" };
        return await this.makeRequest("GET", `tracks/${trackId}`, params);
    }

    async getAlbum(albumId, countryCode) {
        const albumResponse = await this.makeRequest("GET", `albums/${albumId}`, { countryCode, include: "items" });
        if (albumResponse && !albumResponse.artist) {
            const artistId = albumResponse.artistId;
            if (artistId) {
                const artistResponse = await this.makeRequest("GET", `artists/${artistId}`, { countryCode });
                if (artistResponse) {
                    albumResponse.artistName = artistResponse.name || "Unknown Artist";
                }
            }
        }
        return albumResponse;
    }

    async searchTracks(query, countryCode = 'US', limit = 30) {
        const params = {
            query,
            countryCode,
            limit,
            type: 'TRACKS'
        };
        return await this.searchRequest(params);
    }

    async searchAlbums(query, countryCode = 'US', limit = 30) {
        const params = {
            query,
            countryCode,
            limit,
            type: 'ALBUMS'
        };
        return await this.searchRequest(params);
    }

    async searchArtists(query, countryCode = 'US', limit = 30) {
        const params = {
            query,
            countryCode,
            limit,
            type: 'ARTISTS'
        };
        return await this.searchRequest(params);
    }

    async getArtistAlbums(artistId, countryCode = 'US') {
        const params = { countryCode };
        return await this.makeRequest("GET", `artists/${artistId}/relationships/albums`, params);
    }

    async searchPlaylists(query, countryCode = 'US', limit = 30) {
        const params = {
            query,
            countryCode,
            limit,
            type: 'PLAYLISTS'
        };
        return await this.searchRequest(params);
    }

    async searchVideos(query, countryCode = 'US', limit = 30) {
        const params = {
            query,
            countryCode,
            limit,
            type: 'VIDEOS'
        };
        return await this.searchRequest(params);
    }

    async searchV1(query, type, countryCode, userToken) {
        const typeMap = { track: 'TRACKS', album: 'ALBUMS', artist: 'ARTISTS', playlist: 'PLAYLISTS', video: 'VIDEOS' };
        const tidalType = typeMap[type] || 'TRACKS';
        const url = `https://api.tidal.com/v1/search?query=${encodeURIComponent(query)}&types=${tidalType}&countryCode=${countryCode}&limit=30`;
        const resp = await fetch(url, { headers: { Authorization: `Bearer ${userToken}` } });
        if (!resp.ok) throw new Error(`Tidal v1 search failed: ${resp.status}`);
        const data = await resp.json();
        return {
            tracks:    (data.tracks?.items    || []).map(item => ({ resource: item })),
            albums:    (data.albums?.items    || []).map(item => ({ resource: item })),
            artists:   (data.artists?.items   || []).map(item => ({ resource: item })),
            playlists: (data.playlists?.items || []).map(item => ({ resource: item })),
            videos:    (data.videos?.items    || []).map(item => ({ resource: item })),
        };
    }
}

module.exports = TidalAPI;
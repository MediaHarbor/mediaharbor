const crypto = require('crypto');
const fs = require('fs');
const stream = require('stream');
const { getVenvPython } = require('../venvManager');

const TOTP_PERIOD = 30;
const TOTP_DIGITS = 6;
const TOTP_SECRETS_URL = 'https://git.gay/thereallo/totp-secrets/raw/branch/main/secrets/secretDict.json';
const SERVER_TIME_URL = 'https://open.spotify.com/api/server-time';
const SESSION_TOKEN_URL = 'https://open.spotify.com/api/token';
const CLIENT_TOKEN_URL = 'https://clienttoken.spotify.com/v1/clienttoken';
const PLAYBACK_INFO_URL = 'https://gue1-spclient.spotify.com/track-playback/v1/media/spotify:{mediaType}:{mediaId}';
const STORAGE_RESOLVE_URL = 'https://gue1-spclient.spotify.com/storage-resolve/v2/files/audio/interactive/11/{fileId}?version=10000000&product=9&platform=39&alt=json';
const PLAYPLAY_LICENSE_URL = 'https://gew4-spclient.spotify.com/playplay/v1/key/{fileId}';
const CLIENT_VERSION = '1.2.70.61.g856ccd63';
const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36';
const AES_IV = Buffer.from('72e067fbddcbcf77ebe8bc643f630d93', 'hex');

class LibrespotService {
    constructor() {
        this._accessToken = null;
        this._tokenExpiry = 0;
        this._userProfile = null;
        this._loggedIn = false;
        this._spDc = null;
        this._totpSecret = null;
        this._totpVersion = null;
        this._clientToken = null;
        this._clientId = null;
    }

    async loginFromCookies(cookiesPath) {
        if (!cookiesPath) throw new Error('Cookies file path is required');
        if (!fs.existsSync(cookiesPath)) throw new Error('Cookies file not found: ' + cookiesPath);

        const content = fs.readFileSync(cookiesPath, 'utf8');
        const spDc = this._extractSpDc(content);
        if (!spDc) throw new Error('sp_dc cookie not found in cookies file. Make sure you exported cookies from open.spotify.com');

        this._spDc = spDc;
        await this._initTotp();
        await this._refreshToken();

        try {
            this._userProfile = await this._fetchProfile();
        } catch (err) {
            this._userProfile = { name: 'Spotify User' };
        }
        this._loggedIn = true;
        return this._userProfile;
    }

    isLoggedIn() {
        return this._loggedIn && this._accessToken != null;
    }

    getProfile() {
        return this._userProfile;
    }

    logout() {
        this._accessToken = null;
        this._tokenExpiry = 0;
        this._userProfile = null;
        this._loggedIn = false;
        this._spDc = null;
    }

    async _apiCall(url, maxRetries = 2) {
        for (let attempt = 0; attempt < maxRetries; attempt++) {
            const token = await this._getValidToken();
            const headers = {
                'Authorization': `Bearer ${token}`,
                'Accept': 'application/json',
                'Origin': 'https://open.spotify.com',
                'Referer': 'https://open.spotify.com/',
                'User-Agent': UA,
                'app-platform': 'WebPlayer',
                'spotify-app-version': CLIENT_VERSION,
            };
            if (this._clientToken) headers['client-token'] = this._clientToken;
            const resp = await fetch(url, { headers });
            if (resp.status === 429) {
                const retryAfter = parseInt(resp.headers.get('Retry-After') || '0', 10);
                const waitMs = Math.max(retryAfter * 1000, (2 ** attempt) * 1000);
                if (attempt < maxRetries - 1) {
                    await new Promise(r => setTimeout(r, waitMs));
                    continue;
                }
            }
            if (!resp.ok) throw new Error(`Spotify API error: ${resp.status}`);
            return await resp.json();
        }
    }

    async search(query, type, limit = 20) {
        if (!this.isLoggedIn()) throw new Error('Spotify account not connected');
        const typeMap = { track: 'track', album: 'album', artist: 'artist', playlist: 'playlist', episode: 'episode', podcast: 'podcast', show: 'podcast', audiobook: 'audiobook' };
        const spType = typeMap[type] || type;
        const token = await this._getValidToken();
        const url = `https://spclient.wg.spotify.com/searchview/km/v4/search/${encodeURIComponent(query)}?limit=${limit}&entityVersion=2&catalogue=&platform=web&locale=en&types=${spType}`;
        const resp = await fetch(url, {
            headers: {
                'Authorization': `Bearer ${token}`,
                'Accept': 'application/json',
                'app-platform': 'WebPlayer',
            }
        });
        if (!resp.ok) throw new Error(`Spotify search error: ${resp.status}`);
        const data = await resp.json();
        return this._transformSearchResults(data.results || {}, spType);
    }

    _transformSearchResults(results, type) {
        const _uri2url = (uri) => {
            if (!uri) return undefined;
            const m = uri.match(/spotify:(track|album|artist|playlist|show|episode|audiobook):(.+)/);
            return m ? `https://open.spotify.com/${m[1]}/${m[2]}` : undefined;
        };
        const _uri2id = (uri) => uri?.split(':').pop();
        const out = {};

        if (results.tracks && (type === 'track' || !type)) {
            out.tracks = { items: results.tracks.hits.map(t => ({
                name: t.name, id: _uri2id(t.uri), uri: t.uri,
                external_urls: { spotify: _uri2url(t.uri) },
                artists: (t.artists || []).map(a => ({ name: a.name, id: _uri2id(a.uri), uri: a.uri })),
                album: { name: t.album?.name, uri: t.album?.uri, images: t.image ? [{ url: t.image }] : [] },
                duration_ms: t.duration, explicit: t.explicit ?? false,
            }))};
        }
        if (results.albums && (type === 'album' || !type)) {
            out.albums = { items: results.albums.hits.map(a => ({
                name: a.name, id: _uri2id(a.uri), uri: a.uri,
                external_urls: { spotify: _uri2url(a.uri) },
                artists: (a.artists || []).map(ar => ({ name: ar.name, id: _uri2id(ar.uri), uri: ar.uri })),
                images: a.image ? [{ url: a.image }] : [],
            }))};
        }
        if (results.artists && (type === 'artist' || !type)) {
            out.artists = { items: results.artists.hits.map(a => ({
                name: a.name, id: _uri2id(a.uri), uri: a.uri,
                external_urls: { spotify: _uri2url(a.uri) },
                images: a.image ? [{ url: a.image }] : [],
            }))};
        }
        if (results.playlists && (type === 'playlist' || !type)) {
            out.playlists = { items: results.playlists.hits.map(p => ({
                name: p.name, id: _uri2id(p.uri), uri: p.uri,
                external_urls: { spotify: _uri2url(p.uri) },
                images: p.image ? [{ url: p.image }] : [],
                owner: { display_name: p.author },
                tracks: { total: p.followersCount },
            }))};
        }
        const showsHits = (results.podcasts || results.shows);
        if (showsHits && (type === 'podcast' || type === 'show' || !type)) {
            out.shows = { items: showsHits.hits.map(s => ({
                name: s.name, id: _uri2id(s.uri), uri: s.uri,
                external_urls: { spotify: _uri2url(s.uri) },
                images: s.image ? [{ url: s.image }] : [],
                publisher: s.author,
                total_episodes: s.episodeCount,
                media_type: s.mediaType,
            }))};
        }
        const episodesHits = (results.podcastEpisodes || results.episodes);
        if (episodesHits && (type === 'episode' || !type)) {
            out.episodes = { items: episodesHits.hits.map(e => ({
                name: e.name, id: _uri2id(e.uri), uri: e.uri,
                external_urls: { spotify: _uri2url(e.uri) },
                images: e.image ? [{ url: e.image }] : [],
                duration_ms: e.duration,
                release_date: e.releaseDate,
                explicit: e.explicit ?? false,
            }))};
        }
        if (results.audiobooks && (type === 'audiobook' || !type)) {
            out.audiobooks = { items: results.audiobooks.hits.map(b => ({
                name: b.name, id: _uri2id(b.uri), uri: b.uri,
                external_urls: { spotify: _uri2url(b.uri) },
                images: b.image ? [{ url: b.image }] : [],
                authors: (b.authors || []).map(a => ({ name: a })),
                narrators: (b.narrators || []).map(n => ({ name: n })),
                total_chapters: b.chapterCount,
            }))};
        }
        return out;
    }

    async getTrackInfo(trackId) {
        if (!this.isLoggedIn()) throw new Error('Spotify account not connected');
        const token = await this._getValidToken();
        const vars = JSON.stringify({ uri: `spotify:track:${trackId}` });
        const ext = JSON.stringify({ persistedQuery: { version: 1, sha256Hash: 'ae85b52abb74d20a4c331d4143d4772c95f34757bfa8c625474b912b9055b5c0' } });
        const url = `https://api-partner.spotify.com/pathfinder/v1/query?operationName=getTrack&variables=${encodeURIComponent(vars)}&extensions=${encodeURIComponent(ext)}`;
        const resp = await fetch(url, {
            headers: { 'Authorization': `Bearer ${token}`, 'Accept': 'application/json', 'app-platform': 'WebPlayer' }
        });
        if (!resp.ok) throw new Error(`Spotify track info error: ${resp.status}`);
        const data = await resp.json();
        const t = data.data?.trackUnion;
        if (!t) throw new Error('Track not found');
        return {
            id: t.id, name: t.name, uri: t.uri,
            duration_ms: t.duration?.totalMilliseconds,
            explicit: t.contentRating?.label === 'EXPLICIT',
            external_urls: { spotify: t.sharingInfo?.shareUrl || `https://open.spotify.com/track/${trackId}` },
            album: { name: t.albumOfTrack?.name },
            artists: (t.firstArtist?.items || []).map(a => ({ name: a.profile?.name })),
            preview_url: null,
        };
    }

    static extractTrackId(input) {
        return LibrespotService.extractMediaInfo(input)?.id ?? null;
    }

    static extractMediaInfo(input) {
        if (!input) return null;
        const uriMatch = input.match(/spotify:(track|episode|chapter):([A-Za-z0-9]+)/);
        if (uriMatch) return { id: uriMatch[2], type: uriMatch[1] };
        const urlMatch = input.match(/open\.spotify\.com\/(?:intl-[a-z-]+\/)?(track|episode|chapter)\/([A-Za-z0-9]+)/);
        if (urlMatch) return { id: urlMatch[2], type: urlMatch[1] };
        return null;
    }

    _extractSpDc(cookieFileContent) {
        for (const line of cookieFileContent.split('\n')) {
            const trimmed = line.trim();
            if (trimmed.startsWith('#') || !trimmed) continue;
            const parts = trimmed.split('\t');
            if (parts.length >= 7 && parts[5] === 'sp_dc') {
                return parts[6].trim();
            }
        }
        return null;
    }

    async _initTotp() {
        if (this._totpSecret) return;
        const resp = await fetch(TOTP_SECRETS_URL);
        if (!resp.ok) throw new Error('Failed to fetch TOTP secrets');
        const secrets = await resp.json();
        const version = Object.keys(secrets).reduce((a, b) => parseInt(a) > parseInt(b) ? a : b);
        const ciphertext = secrets[version];
        const derived = ciphertext.map((byte, i) => String(byte ^ ((i % 33) + 9))).join('');
        this._totpSecret = Buffer.from(derived, 'ascii');
        this._totpVersion = version;
    }

    _generateTotp(timestampSec) {
        const counter = Math.floor(timestampSec / TOTP_PERIOD);
        const counterBuf = Buffer.alloc(8);
        counterBuf.writeBigUInt64BE(BigInt(counter));
        const hmac = crypto.createHmac('sha1', this._totpSecret).update(counterBuf).digest();
        const offset = hmac[hmac.length - 1] & 0x0f;
        const binary = ((hmac[offset] & 0x7f) << 24) | ((hmac[offset + 1] & 0xff) << 16) |
                        ((hmac[offset + 2] & 0xff) << 8) | (hmac[offset + 3] & 0xff);
        return String(binary % (10 ** TOTP_DIGITS)).padStart(TOTP_DIGITS, '0');
    }

    async _refreshToken() {
        await this._initTotp();

        const timeResp = await fetch(SERVER_TIME_URL, {
            headers: { 'user-agent': UA }
        });
        if (!timeResp.ok) throw new Error('Failed to get Spotify server time');
        const timeData = await timeResp.json();
        const serverTimeSec = timeData.serverTime;

        const totp = this._generateTotp(serverTimeSec);

        const params = new URLSearchParams({
            reason: 'init',
            productType: 'web-player',
            totp: totp,
            totpServer: totp,
            totpVer: this._totpVersion,
        });

        const tokenResp = await fetch(`${SESSION_TOKEN_URL}?${params}`, {
            headers: {
                'cookie': `sp_dc=${this._spDc}`,
                'user-agent': UA,
                'app-platform': 'WebPlayer',
                'spotify-app-version': CLIENT_VERSION,
            }
        });
        if (!tokenResp.ok) {
            const text = await tokenResp.text();
            throw new Error(`Failed to get access token: ${tokenResp.status} — ${text.substring(0, 200)}`);
        }
        const data = await tokenResp.json();
        if (!data.accessToken) throw new Error('No access token in response. Cookie may be expired.');
        if (data.isAnonymous) throw new Error('Cookie is expired or invalid — got anonymous token. Re-export your cookies.');

        this._accessToken = data.accessToken;
        this._clientId = data.clientId;
        this._tokenExpiry = data.accessTokenExpirationTimestampMs || (Date.now() + 3600000);

        await this._refreshClientToken();
    }

    async _refreshClientToken() {
        if (!this._clientId) return;
        const resp = await fetch(CLIENT_TOKEN_URL, {
            method: 'POST',
            headers: { 'Accept': 'application/json', 'Content-Type': 'application/json' },
            body: JSON.stringify({
                client_data: {
                    client_version: CLIENT_VERSION,
                    client_id: this._clientId,
                    js_sdk_data: {},
                }
            }),
        });
        if (!resp.ok) {
            return;
        }
        const data = await resp.json();
        this._clientToken = data?.granted_token?.token || null;
    }

    async _getValidToken() {
        if (Date.now() >= this._tokenExpiry - 60000) {
            await this._refreshToken();
        }
        return this._accessToken;
    }

    async _fetchProfile() {
        const token = await this._getValidToken();
        const resp = await fetch('https://api.spotify.com/v1/me', {
            headers: { Authorization: `Bearer ${token}`, Accept: 'application/json' }
        });
        if (!resp.ok) throw new Error(`Profile fetch failed: ${resp.status}`);
        const data = await resp.json();
        return { name: data.display_name || data.id, plan: data.product, email: data.email, id: data.id };
    }

    _authHeaders() {
        const h = {
            'Authorization': `Bearer ${this._accessToken}`,
            'Accept': 'application/json',
            'user-agent': UA,
            'app-platform': 'WebPlayer',
            'spotify-app-version': CLIENT_VERSION,
            'origin': 'https://open.spotify.com/',
            'referer': 'https://open.spotify.com/',
        };
        if (this._clientToken) h['client-token'] = this._clientToken;
        return h;
    }

    static _idToGid(id) {
        const CHARSET = '0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ';
        let n = BigInt(0);
        for (const c of id) {
            n = n * 62n + BigInt(CHARSET.indexOf(c));
        }
        return n.toString(16).padStart(32, '0');
    }

    async _getPlaybackInfo(trackId, mediaType = 'track') {
        const url = PLAYBACK_INFO_URL.replace('{mediaType}', mediaType).replace('{mediaId}', trackId);
        const formatsToTry = mediaType === 'episode' || mediaType === 'chapter'
            ? ['file_ids_mp4', 'file_ids_mp4_dual', 'file_ids_ogg']
            : ['file_ids_mp4'];

        for (const fmt of formatsToTry) {
            for (let attempt = 0; attempt < 3; attempt++) {
                const token = await this._getValidToken();
                const resp = await fetch(`${url}?manifestFileFormat=${fmt}`, {
                    headers: this._authHeaders(),
                });
                if (resp.ok) {
                    const data = await resp.json();
                    if (this._manifestHasFiles(data)) return data;
                    break;
                }
                if ((resp.status === 502 || resp.status === 503 || resp.status === 504) && attempt < 2) {
                    await new Promise(r => setTimeout(r, 800 * (attempt + 1)));
                    continue;
                }
                if (resp.status === 400) break; // Bad format, try next
                const text = await resp.text();
                throw new Error(`Playback info failed: ${resp.status} — ${text.substring(0, 300)}`);
            }
        }
        const token = await this._getValidToken();
        const resp = await fetch(`${url}?manifestFileFormat=file_ids_mp4`, {
            headers: this._authHeaders(),
        });
        if (!resp.ok) {
            const text = await resp.text();
            throw new Error(`Playback info failed: ${resp.status} — ${text.substring(0, 300)}`);
        }
        return await resp.json();
    }

    _manifestHasFiles(playbackInfo) {
        const checkManifest = (m) => {
            if (!m) return false;
            if (Array.isArray(m.file_ids_mp4) && m.file_ids_mp4.length > 0) return true;
            if (Array.isArray(m.file_ids_ogg) && m.file_ids_ogg.length > 0) return true;
            if (m.url) return true;
            return false;
        };
        if (checkManifest(playbackInfo?.manifest)) return true;
        if (playbackInfo?.media) {
            for (const key of Object.keys(playbackInfo.media)) {
                const entry = playbackInfo.media[key];
                if (checkManifest(entry?.item?.manifest)) return true;
                if (checkManifest(entry?.manifest)) return true;
                if (Array.isArray(entry?.items) && checkManifest(entry.items[0]?.manifest)) return true;
            }
        }
        return false;
    }

    async _getStreamUrls(fileId) {
        const token = await this._getValidToken();
        const url = STORAGE_RESOLVE_URL.replace('{fileId}', fileId);
        const resp = await fetch(url, {
            headers: {
                'Authorization': `Bearer ${token}`,
                'Accept': 'application/json',
                'app-platform': 'Android',
                'user-agent': 'Spotify/8.9.86.551 Android/34 (Google Pixel 8)',
                ...(this._clientToken ? { 'client-token': this._clientToken } : {}),
            }
        });
        if (!resp.ok) throw new Error(`Storage resolve failed: ${resp.status}`);
        return await resp.json();
    }

    async _getDecryptionKey(fileId) {
        const reUnplayplay = await import('re-unplayplay');
        const ppToken = reUnplayplay.getToken();

        const fileIdBuf = Buffer.from(fileId, 'hex');
        const ppTokenField = this._encodeProtobufField(1, ppToken);
        const fileIdField = this._encodeProtobufField(2, fileIdBuf);
        const body = Buffer.concat([ppTokenField, fileIdField]);

        const token = await this._getValidToken();
        const url = PLAYPLAY_LICENSE_URL.replace('{fileId}', fileId);
        const resp = await fetch(url, {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${token}`,
                'Content-Type': 'application/x-protobuf',
                'Accept': 'application/x-protobuf',
                'app-platform': 'Android',
                'user-agent': 'Spotify/8.9.86.551 Android/34 (Google Pixel 8)',
                ...(this._clientToken ? { 'client-token': this._clientToken } : {}),
            },
            body: body,
        });
        if (!resp.ok) throw new Error(`PlayPlay license failed: ${resp.status}`);
        const respBuf = Buffer.from(await resp.arrayBuffer());

        const obfuscatedKey = this._parseProtobufField(respBuf, 1);
        if (!obfuscatedKey) throw new Error('No obfuscated key in PlayPlay response');

        return reUnplayplay.decryptAndBindKey(obfuscatedKey, fileIdBuf);
    }

    _encodeProtobufField(fieldNumber, data) {
        const tag = (fieldNumber << 3) | 2;
        const tagBuf = this._encodeVarint(tag);
        const lenBuf = this._encodeVarint(data.length);
        return Buffer.concat([tagBuf, lenBuf, data]);
    }

    _encodeVarint(value) {
        const bytes = [];
        while (value > 0x7f) {
            bytes.push((value & 0x7f) | 0x80);
            value >>>= 7;
        }
        bytes.push(value & 0x7f);
        return Buffer.from(bytes);
    }

    _parseProtobufField(buf, targetField) {
        let offset = 0;
        while (offset < buf.length) {
            const { value: tag, bytesRead: tagBytes } = this._decodeVarint(buf, offset);
            offset += tagBytes;
            const fieldNum = tag >> 3;
            const wireType = tag & 0x7;
            if (wireType === 2) {
                const { value: len, bytesRead: lenBytes } = this._decodeVarint(buf, offset);
                offset += lenBytes;
                if (fieldNum === targetField) return buf.subarray(offset, offset + len);
                offset += len;
            } else if (wireType === 0) {
                const { bytesRead } = this._decodeVarint(buf, offset);
                offset += bytesRead;
            } else {
                break;
            }
        }
        return null;
    }

    _decodeVarint(buf, offset) {
        let value = 0;
        let shift = 0;
        let bytesRead = 0;
        while (offset < buf.length) {
            const byte = buf[offset++];
            bytesRead++;
            value |= (byte & 0x7f) << shift;
            if (!(byte & 0x80)) break;
            shift += 7;
        }
        return { value, bytesRead };
    }

    _createDecryptStream(key, isOgg = true) {
        let byteCounter = 0;
        let blockCounter = 0;
        let headerFound = false;
        let pendingBuffers = [];

        return new stream.Transform({
            transform(chunk, encoding, callback) {
                const blockSize = 16;
                const output = [];

                for (let i = 0; i < chunk.length; i += blockSize) {
                    const block = chunk.subarray(i, Math.min(i + blockSize, chunk.length));

                    const iv = Buffer.from(AES_IV);
                    let carry = blockCounter;
                    for (let j = 15; j >= 0 && carry > 0; j--) {
                        carry += iv[j];
                        iv[j] = carry & 0xff;
                        carry >>= 8;
                    }

                    const cipher = crypto.createCipheriv('aes-128-ecb', key, null);
                    cipher.setAutoPadding(false);
                    const keystream = cipher.update(iv);

                    const decrypted = Buffer.alloc(block.length);
                    for (let j = 0; j < block.length; j++) {
                        decrypted[j] = block[j] ^ keystream[j];
                    }

                    blockCounter++;
                    byteCounter += decrypted.length;
                    pendingBuffers.push(decrypted);
                }

                if (!headerFound) {
                    const combined = Buffer.concat(pendingBuffers);
                    const oggOffset = combined.indexOf(Buffer.from('OggS'));
                    if (oggOffset >= 0) {
                        headerFound = true;
                        output.push(combined.subarray(oggOffset));
                        pendingBuffers = [];
                    } else if (combined.length > 0xa7) {
                        headerFound = true;
                        output.push(combined.subarray(0xa7));
                        pendingBuffers = [];
                    }
                } else {
                    output.push(...pendingBuffers);
                    pendingBuffers = [];
                }

                if (output.length > 0) {
                    this.push(Buffer.concat(output));
                }
                callback();
            },
            flush(callback) {
                if (pendingBuffers.length > 0) {
                    const combined = Buffer.concat(pendingBuffers);
                    if (!headerFound) {
                        const oggOffset = combined.indexOf(Buffer.from('OggS'));
                        if (oggOffset >= 0) {
                            this.push(combined.subarray(oggOffset));
                        } else if (combined.length > 0xa7) {
                            this.push(combined.subarray(0xa7));
                        }
                    } else {
                        this.push(combined);
                    }
                }
                callback();
            }
        });
    }

    async _getGidMetadata(trackId, mediaType = 'track') {
        const gid = LibrespotService._idToGid(trackId);
        const token = await this._getValidToken();
        const headers = {
            'Authorization': `Bearer ${token}`,
            'Accept': 'application/json',
            'app-platform': 'Android',
            'user-agent': 'Spotify/8.9.86.551 Android/34 (Google Pixel 8)',
            ...(this._clientToken ? { 'client-token': this._clientToken } : {}),
        };
        const base = `https://spclient.wg.spotify.com/metadata/4/${mediaType}/${gid}`;
        for (const url of [`${base}?market=from_token`, base]) {
            const resp = await fetch(url, { headers });
            if (resp.ok) return await resp.json();
            if (resp.status !== 404) {
                const text = await resp.text();
                throw new Error(`GID metadata failed: ${resp.status} — ${text.substring(0, 300)}`);
            }
        }
        throw new Error(`GID metadata failed: 404 — not found`);
    }

    _selectOggFile(files) {
        const formatPriority = ['OGG_VORBIS_320', 'OGG_VORBIS_160', 'OGG_VORBIS_96'];
        for (const fmt of formatPriority) {
            const f = files.find(file => file.format === fmt);
            if (f) return f;
        }
        return files.find(f => f.format?.startsWith('OGG_VORBIS')) || null;
    }

    _selectAudioFile(files) {
        const ogg = this._selectOggFile(files);
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

    _extractFeedUrl(data) {
        if (!data) return null;
        const d = data.show || data;
        return d.rssFeedUrl || d.rss_url || d.feed_url || d.feedUrl || d.rssUrl || d.external_url || null;
    }

    _extractEpisodeExternalUrl(data) {
        if (!data) return null;
        const ep = data.episode || data;
        return ep.externalUrl || ep.external_url || ep.audioUrl || ep.audio?.url || ep.media?.url || null;
    }

    async _getExternalEpisodeUrl(episodeId, showId, showName, episodeName, durationMs) {
        const token = await this._getValidToken();
        const headers = {
            'Authorization': `Bearer ${token}`,
            'Accept': 'application/json',
            'app-platform': 'Android',
            'user-agent': 'Spotify/8.9.86.551 Android/34 (Google Pixel 8)',
            ...(this._clientToken ? { 'client-token': this._clientToken } : {}),
        };

        const tryFetch = async (url) => {
            try {
                const r = await fetch(url, { headers });
                if (r.ok) return await r.json();
            } catch (_e) {}
            return null;
        };

        const peEp = await tryFetch(
            `https://spclient.wg.spotify.com/podcast-experience/v2/episodes/${episodeId}`
        );
        if (peEp) {
            const directUrl = this._extractEpisodeExternalUrl(peEp);
            if (directUrl) return directUrl;
            const feedUrl = this._extractFeedUrl(peEp);
            if (feedUrl) {
                const u = await this._findEpisodeInRss(feedUrl, episodeId, episodeName, durationMs);
                if (u) return u;
            }
        }

        if (showId) {
            const peShow = await tryFetch(
                `https://spclient.wg.spotify.com/podcast-experience/v2/shows/${showId}`
            );
            if (peShow) {
                const feedUrl = this._extractFeedUrl(peShow);
                if (feedUrl) {
                    const u = await this._findEpisodeInRss(feedUrl, episodeId, episodeName, durationMs);
                    if (u) return u;
                }
            }
        }

        if (showId) {
            try {
                const showGid = LibrespotService._idToGid(showId);
                const base = `https://spclient.wg.spotify.com/metadata/4/podcast/${showGid}`;
                for (const url of [`${base}?market=from_token`, base]) {
                    const r = await fetch(url, { headers });
                    if (r.ok) {
                        const showMeta = await r.json();
                        const feedUrl = this._extractFeedUrl(showMeta);
                        if (feedUrl) {
                            const u = await this._findEpisodeInRss(feedUrl, episodeId, episodeName, durationMs);
                            if (u) return u;
                        }
                        break;
                    }
                    if (r.status !== 404) break;
                }
            } catch (_e) {}
        }

        return null;
    }

    async _findEpisodeInRss(feedUrl, episodeId, episodeName, durationMs) {
        const xml2js = require('xml2js');
        const resp = await fetch(feedUrl, {
            redirect: 'follow',
            headers: { 'User-Agent': 'Mozilla/5.0 (compatible; podcast-player/1.0)' },
        });
        if (!resp.ok) return null;
        const text = await resp.text();
        const data = await xml2js.parseStringPromise(text, { explicitArray: true });
        const items = data?.rss?.channel?.[0]?.item || [];
        const spotifyUri = `spotify:episode:${episodeId}`;

        let bestMatch = null;
        for (const item of items) {
            const enclosure = item.enclosure?.[0]?.$ || {};
            const audioUrl = enclosure.url;
            if (!audioUrl) continue;

            const guid = item.guid?.[0];
            const guidVal = typeof guid === 'string' ? guid : guid?._ || '';
            if (guidVal === spotifyUri) return audioUrl;

            const rawTitle = Array.isArray(item.title) ? item.title[0] : item.title;
            const titleStr = typeof rawTitle === 'string' ? rawTitle : rawTitle?._ || '';
            if (episodeName && titleStr.toLowerCase().trim() === episodeName.toLowerCase().trim()) {
                return audioUrl;
            }

            if (episodeName && titleStr.toLowerCase().includes(episodeName.toLowerCase().substring(0, 20))) {
                bestMatch = bestMatch || audioUrl;
            }

            if (durationMs) {
                const itunesDuration = item['itunes:duration']?.[0];
                if (itunesDuration) {
                    const parsed = this._parseItunesDuration(itunesDuration);
                    if (Math.abs(parsed - durationMs) < 5000) bestMatch = audioUrl;
                }
            }
        }
        return bestMatch;
    }

    _parseItunesDuration(duration) {
        if (typeof duration !== 'string') return parseInt(duration) * 1000 || 0;
        const parts = duration.split(':').map(Number);
        if (parts.length === 3) return ((parts[0] * 3600) + (parts[1] * 60) + parts[2]) * 1000;
        if (parts.length === 2) return ((parts[0] * 60) + parts[1]) * 1000;
        return parseFloat(duration) * 1000 || 0;
    }

    async getTrackStream(trackId, mediaType = 'track') {
        if (!this.isLoggedIn()) throw new Error('Spotify account not connected');

        let fileId = null;
        let isOgg = false;
        let episodeExternalUrl = null; // For RSS-hosted podcast episodes (no file IDs)
        let durationMs = 0;
        let gidError = null;

        try {
            const metadata = await this._getGidMetadata(trackId, mediaType);
            durationMs = metadata.duration || 0;
            let files = metadata.file || metadata.audio;
            if ((!files || files.length === 0) && metadata.alternative) {
                for (const alt of metadata.alternative) {
                    const altFiles = alt.file || alt.audio;
                    if (altFiles && altFiles.length > 0) { files = altFiles; break; }
                }
            }
            if (files && files.length > 0) {
                const selected = this._selectAudioFile(files);
                if (selected) {
                    fileId = selected.file.file_id;
                    isOgg = selected.isOgg;
                }
            }
            if (!fileId && metadata.external_url) {
                episodeExternalUrl = metadata.external_url;
            }
        } catch (err) {
            gidError = err;
        }

        if (!fileId && episodeExternalUrl) {
            const { Readable } = require('stream');
            const resp = await fetch(episodeExternalUrl);
            if (!resp.ok) throw new Error(`External episode fetch failed: ${resp.status}`);
            return {
                stream: Readable.fromWeb(resp.body),
                contentType: 'audio/mpeg',
                durationMs,
            };
        }

        if (!fileId) {
            let playbackInfo;
            try {
                playbackInfo = await this._getPlaybackInfo(trackId, mediaType);
            } catch (err) {
                const msg = gidError ? `GID: ${gidError.message} | Playback: ${err.message}` : err.message;
                throw new Error(`No playable file found: ${msg}`);
            }

            let manifest = playbackInfo.manifest;
            let itemMeta = playbackInfo.metadata;
            if (!manifest && playbackInfo.media) {
                for (const key of Object.keys(playbackInfo.media)) {
                    const entry = playbackInfo.media[key];
                    if (entry?.item?.manifest) {
                        manifest = entry.item.manifest;
                        itemMeta = entry.item.metadata;
                        break;
                    }
                    if (entry?.manifest) {
                        manifest = entry.manifest;
                        itemMeta = entry.metadata;
                        break;
                    }
                    if (Array.isArray(entry?.items) && entry.items[0]?.manifest) {
                        manifest = entry.items[0].manifest;
                        itemMeta = entry.items[0].metadata;
                        break;
                    }
                }
            }
            if (itemMeta?.duration) durationMs = itemMeta.duration;

            const noMp4 = !manifest?.file_ids_mp4 || manifest.file_ids_mp4.length === 0;
            const noOgg = !manifest?.file_ids_ogg || manifest.file_ids_ogg.length === 0;
            if (manifest?.url && noMp4 && noOgg) {
                const { Readable } = require('stream');
                const resp = await fetch(manifest.url);
                if (!resp.ok) throw new Error(`External episode fetch failed: ${resp.status}`);
                return {
                    stream: Readable.fromWeb(resp.body),
                    contentType: resp.headers.get('content-type') || 'audio/mpeg',
                    durationMs,
                };
            }

            const oggManifestFiles = manifest?.file_ids_ogg;
            if (Array.isArray(oggManifestFiles) && oggManifestFiles.length > 0) {
                const preferred = oggManifestFiles.find(f => f.format === 'OGG_VORBIS_320')
                    || oggManifestFiles.find(f => f.format === 'OGG_VORBIS_160')
                    || oggManifestFiles[0];
                if (preferred) { fileId = preferred.file_id; isOgg = true; }
            }

            if (!fileId) {
                const mp4Files = manifest?.file_ids_mp4;
                if (Array.isArray(mp4Files) && mp4Files.length > 0) {
                    const preferred = mp4Files.find(f => f.format === '11') || mp4Files.find(f => f.format === '10') || mp4Files[0];
                    if (preferred) fileId = preferred.file_id;
                }
            }

            if (!fileId && Array.isArray(manifest?.file_ids_mp4_dual)) {
                const audioEntry = manifest.file_ids_mp4_dual.find(e =>
                    e.type === 'audio' || (Array.isArray(e.qualities) && e.qualities.length > 0)
                );
                const files = audioEntry?.qualities || manifest.file_ids_mp4_dual;
                if (Array.isArray(files) && files.length > 0) {
                    const preferred = files.find(f => f.format === '11') || files.find(f => f.format === '10') || files[0];
                    if (preferred?.file_id) fileId = preferred.file_id;
                }
            }

            if (!fileId && mediaType === 'episode') {
                const showId = itemMeta?.group_uri?.split(':').pop() ?? null;
                const showName = itemMeta?.group_name || itemMeta?.context_description;
                const audioUrl = await this._getExternalEpisodeUrl(
                    trackId, showId, showName, itemMeta?.name, itemMeta?.duration || durationMs
                );
                if (audioUrl) {
                    const { Readable } = require('stream');
                    const resp = await fetch(audioUrl, { redirect: 'follow' });
                    if (resp.ok) {
                        return {
                            stream: Readable.fromWeb(resp.body),
                            contentType: resp.headers.get('content-type') || 'audio/mpeg',
                            durationMs: itemMeta?.duration || durationMs,
                        };
                    }
                }
            }

            if (!fileId) {
                const gidMsg = gidError ? ` (GID error: ${gidError.message})` : '';
                throw new Error(`No playable file found. Neither OGG nor MP4 files available.${gidMsg}`);
            }
        }

        const streamUrls = await this._getStreamUrls(fileId);
        const cdnUrl = streamUrls?.cdnurl?.[0];
        if (!cdnUrl) throw new Error('No CDN URL from storage-resolve');

        let decryptKey;
        try {
            decryptKey = await this._getDecryptionKey(fileId);
        } catch (err) {
            if (!isOgg) {
                const result = await this._streamMp4WithFfmpeg(cdnUrl, fileId);
                result.durationMs = durationMs;
                return result;
            }
            throw err;
        }

        const cdnResp = await fetch(cdnUrl);
        if (!cdnResp.ok) throw new Error(`CDN fetch failed: ${cdnResp.status}`);

        const { Readable } = require('stream');
        const encryptedStream = Readable.fromWeb(cdnResp.body);
        const decryptStream = this._createDecryptStream(decryptKey);

        const outputStream = encryptedStream.pipe(decryptStream);
        return { stream: outputStream, contentType: isOgg ? 'audio/ogg' : 'audio/mp4', durationMs };
    }

    async _streamMp4WithFfmpeg(cdnUrl, fileId) {
        const path = require('path');
        const { app } = require('electron');
        const settingsPath = path.join(app.getPath('userData'), 'mh-settings.json');
        let wvdPath;
        try {
            const settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
            wvdPath = settings.spotify_wvd_path;
        } catch (e) {
            throw new Error('Could not read settings file for WVD path.');
        }
        if (!wvdPath) {
            throw new Error('WVD file required for Spotify streaming. Go to Settings → Spotify → set WVD Path.');
        }
        if (!fs.existsSync(wvdPath)) {
            throw new Error(`WVD file not found: ${wvdPath}`);
        }

        const seekResp = await fetch(`https://seektables.scdn.co/seektable/${fileId}.json`, {
            headers: { 'Accept': '*/*', 'Origin': 'https://open.spotify.com/', 'Referer': 'https://open.spotify.com/', 'User-Agent': UA }
        });
        if (!seekResp.ok) throw new Error(`Seek table failed: ${seekResp.status}`);
        const seekTable = await seekResp.json();
        const pssh = seekTable.pssh || seekTable.widevine_pssh;
        if (!pssh) throw new Error('No PSSH found — cannot decrypt MP4 stream');

        const token = await this._getValidToken();
        const decryptKeyHex = await this._getWidevineKeyViaPython(pssh, wvdPath, token);

        const { spawn } = require('child_process');
        const { PassThrough } = require('stream');

        const ffmpeg = spawn('ffmpeg', [
            '-y', '-loglevel', 'error',
            '-decryption_key', decryptKeyHex,
            '-i', cdnUrl,
            '-c', 'copy',
            '-f', 'adts',
            'pipe:1'
        ]);

        const output = new PassThrough();
        ffmpeg.stdout.pipe(output);
        ffmpeg.stderr.on('data', () => {});
        ffmpeg.on('error', (err) => { output.destroy(err); });
        ffmpeg.on('close', () => { output.end(); });

        return { stream: output, contentType: 'audio/aac' };
    }

    async _getWidevineKeyViaPython(pssh, wvdPath, accessToken) {
        const { spawn } = require('child_process');
        const clientToken = this._clientToken || '';
        const spDc = this._spDc || '';

        const pyScript = `
import sys, json, base64
from pywidevine import PSSH, Cdm, Device

args = json.loads(sys.stdin.readline())
device = Device.load(args["wvd_path"])
cdm = Cdm.from_device(device)
session_id = cdm.open()
pssh_obj = PSSH(args["pssh"])
challenge = cdm.get_license_challenge(session_id, pssh_obj)

# Output challenge as base64 for Node.js to POST
sys.stdout.write(json.dumps({"challenge": base64.b64encode(challenge).decode()}) + "\\n")
sys.stdout.flush()

# Wait for license response from Node.js
license_line = sys.stdin.readline().strip()
license_b64 = json.loads(license_line)["license"]
license_bytes = base64.b64decode(license_b64)

cdm.parse_license(session_id, license_bytes)
keys = [k for k in cdm.get_keys(session_id) if k.type == "CONTENT"]
cdm.close(session_id)

if not keys:
    sys.stdout.write(json.dumps({"error": "No content keys in license response"}) + "\\n")
    sys.exit(1)

kid_hex = keys[0].kid.hex if isinstance(keys[0].kid.hex, str) else keys[0].kid.hex()
key_hex = keys[0].key.hex() if callable(keys[0].key.hex) else keys[0].key.hex
sys.stdout.write(json.dumps({"key": key_hex, "kid": kid_hex}) + "\\n")
`;

        return new Promise((resolve, reject) => {
            const child = spawn(getVenvPython(), ['-c', pyScript]);
            let stderr = '';
            child.stderr.on('data', (d) => { stderr += d; });
            child.on('error', (err) => reject(new Error(`pywidevine spawn failed: ${err.message}`)));

            let buf = '';
            const lines = [];
            child.stdout.on('data', (chunk) => {
                buf += chunk.toString();
                let nl;
                while ((nl = buf.indexOf('\n')) !== -1) {
                    lines.push(buf.slice(0, nl));
                    buf = buf.slice(nl + 1);
                }
            });

            child.stdin.write(JSON.stringify({ pssh, wvd_path: wvdPath }) + '\n');

            const waitForChallenge = () => {
                if (lines.length > 0) {
                    handleChallenge(lines.shift());
                } else {
                    child.stdout.once('data', () => setTimeout(waitForChallenge, 10));
                }
            };
            setTimeout(waitForChallenge, 100);

            const handleChallenge = async (line) => {
                try {
                    const { challenge } = JSON.parse(line);
                    if (!challenge) throw new Error('No challenge from pywidevine');

                    const resp = await fetch('https://gue1-spclient.spotify.com/widevine-license/v1/audio/license', {
                        method: 'POST',
                        headers: {
                            'Authorization': `Bearer ${accessToken}`,
                            'client-token': clientToken,
                            'Content-Type': 'application/octet-stream',
                            'Accept': '*/*',
                            'app-platform': 'WebPlayer',
                            'spotify-app-version': '1.2.70.61.g856ccd63',
                            'Origin': 'https://open.spotify.com',
                            'Referer': 'https://open.spotify.com/',
                            'User-Agent': UA,
                        },
                        body: Buffer.from(challenge, 'base64'),
                    });

                    if (!resp.ok) {
                        const text = await resp.text();
                        child.kill();
                        return reject(new Error(`Widevine license server returned ${resp.status}: ${text.substring(0, 200)}`));
                    }

                    const licenseBytes = Buffer.from(await resp.arrayBuffer());

                    child.stdin.write(JSON.stringify({ license: licenseBytes.toString('base64') }) + '\n');
                    child.stdin.end();

                    child.on('close', (code) => {
                        if (buf.trim()) lines.push(buf.trim());
                        const lastLine = lines.pop();
                        if (!lastLine) return reject(new Error(`pywidevine exited ${code} with no output`));
                        try {
                            const result = JSON.parse(lastLine);
                            if (result.error) return reject(new Error(result.error));
                            resolve(result.key);
                        } catch (e) {
                            reject(new Error(`Failed to parse pywidevine output: ${lastLine.substring(0, 200)}`));
                        }
                    });
                } catch (err) {
                    child.kill();
                    reject(err);
                }
            };
        });
    }
}

const librespotService = new LibrespotService();
module.exports = librespotService;
module.exports.extractTrackId = LibrespotService.extractTrackId;
module.exports.extractMediaInfo = LibrespotService.extractMediaInfo;

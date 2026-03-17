'use strict';

const https = require('https');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { getEncryptedDeezerUrl } = require('./crypto');
const { downloadAndDecryptDeezer, downloadFile, streamAndDecryptDeezer } = require('./downloader');
const { tagFile } = require('./tagger');
const logger = require('../logger');

const GW_BASE = 'https://www.deezer.com/ajax/gw-light.php';
const MEDIA_BASE = 'https://media.deezer.com/v1';
const PUBLIC_API = 'https://api.deezer.com';

function buildFileName(template, vars, restrictChars = true, truncateTo = 120) {
    let tpl = template.replace(/[\[(][^\])]*/g, (seg, offset, str) => {
        const tokens = [...seg.matchAll(/\{(\w+)(?::\d+)?\}/g)];
        if (tokens.length > 0 && tokens.every(m => !vars[m[1]] && vars[m[1]] !== 0)) return '';
        return seg;
    });
    let name = tpl.replace(/\{(\w+)(?::(\d+))?\}/g, (_, key, pad) => {
        let val = vars[key] != null ? String(vars[key]) : '';
        if (pad && /^\d+$/.test(val)) val = val.padStart(Number(pad), '0');
        return val;
    });
    if (restrictChars) name = name.replace(/[<>:"/\\|?*\x00-\x1f]/g, '_');
    if (truncateTo > 0) name = name.slice(0, truncateTo);
    return name.trim();
}

const QUALITY_MAP = {
    0: { format: 'MP3_128', ext: 'mp3' },
    1: { format: 'MP3_320', ext: 'mp3' },
    2: { format: 'FLAC',    ext: 'flac' },
};

function httpRequest(url, options = {}, body = null) {
    return new Promise((resolve, reject) => {
        const parsed = new URL(url);
        const transport = parsed.protocol === 'https:' ? https : http;
        const reqOptions = {
            hostname: parsed.hostname,
            port: parsed.port || (parsed.protocol === 'https:' ? 443 : 80),
            path: parsed.pathname + parsed.search,
            method: options.method || 'GET',
            headers: options.headers || {},
        };

        const req = transport.request(reqOptions, (res) => {
            if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                return httpRequest(res.headers.location, options, body).then(resolve).catch(reject);
            }
            const chunks = [];
            res.on('data', (c) => chunks.push(c));
            res.on('end', () => resolve({
                status: res.statusCode,
                headers: res.headers,
                body: Buffer.concat(chunks).toString('utf-8'),
            }));
            res.on('error', reject);
        });

        req.on('error', reject);
        if (body) req.write(body);
        req.end();
    });
}

function buildQueryString(params) {
    return Object.entries(params)
        .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`)
        .join('&');
}

// Fetch a URL as a stream, following up to 5 redirects
function httpGetStream(url, headers = {}, redirectsLeft = 5) {
    return new Promise((resolve, reject) => {
        if (redirectsLeft <= 0) return reject(new Error('Too many redirects'));
        const parsed = new URL(url);
        const transport = parsed.protocol === 'https:' ? https : http;
        const req = transport.get(url, { headers: { 'User-Agent': 'Mozilla/5.0', ...headers } }, (res) => {
            if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                res.resume(); // drain the redirect response
                const next = res.headers.location.startsWith('http')
                    ? res.headers.location
                    : new URL(res.headers.location, url).href;
                return httpGetStream(next, headers, redirectsLeft - 1).then(resolve).catch(reject);
            }
            if (res.statusCode < 200 || res.statusCode >= 400) {
                res.resume();
                return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
            }
            resolve(res);
        });
        req.on('error', reject);
    });
}

class DeezerClient {
    constructor() {
        this.arl = null;
        this.sid = null;
        this.apiToken = null;
        this.loggedIn = false;
        this.licenseToken = null;
        this.maxQuality = 1;
    }

    async login(arl) {
        this.arl = (arl || '').trim();
        const resp = await this._gwGet('deezer.getUserData');
        if (!resp || !resp.results) {
            throw new Error('Deezer login failed — invalid API response. Check your internet connection.');
        }
        const userId = resp.results?.USER?.USER_ID;
        if (!userId || Number(userId) === 0) {
            throw new Error('Deezer ARL is invalid or expired. Update it in Settings → Deezer → ARL Token.');
        }
        this.apiToken = resp.results.checkForm;
        this.licenseToken = resp.results.USER?.OPTIONS?.license_token || null;

        const opts = resp.results.USER?.OPTIONS || {};
        if (opts.web_lossless || opts.mobile_lossless) {
            this.maxQuality = 2;
        } else if (opts.web_hq || opts.mobile_hq) {
            this.maxQuality = 1;
        } else {
            this.maxQuality = 0;
        }

        this.loggedIn = true;
    }

    async getPublicTrack(trackId) {
        const resp = await httpRequest(`${PUBLIC_API}/track/${trackId}`, {
            headers: { 'User-Agent': 'Mozilla/5.0' },
        });
        if (resp.status !== 200) throw new Error(`Deezer public API error: ${resp.status}`);
        const track = JSON.parse(resp.body);
        try {
            const contribResp = await httpRequest(`${PUBLIC_API}/track/${trackId}/contributors`, {
                headers: { 'User-Agent': 'Mozilla/5.0' },
            });
            if (contribResp.status === 200) {
                track.contributors = JSON.parse(contribResp.body);
            }
        } catch {}
        return track;
    }

    async getPublicAlbum(albumId) {
        const resp = await httpRequest(`${PUBLIC_API}/album/${albumId}`, {
            headers: { 'User-Agent': 'Mozilla/5.0' },
        });
        if (resp.status !== 200) throw new Error(`Deezer public API error: ${resp.status}`);
        return JSON.parse(resp.body);
    }

    async getPublicPlaylist(playlistId) {
        const resp = await httpRequest(`${PUBLIC_API}/playlist/${playlistId}`, {
            headers: { 'User-Agent': 'Mozilla/5.0' },
        });
        if (resp.status !== 200) throw new Error(`Deezer public API error: ${resp.status}`);
        return JSON.parse(resp.body);
    }

    async getAlbumTracks(albumId) {
        const album = await this.getPublicAlbum(albumId);
        const resp = await httpRequest(`${PUBLIC_API}/album/${albumId}/tracks?limit=500`, { headers: { 'User-Agent': 'Mozilla/5.0' } });
        const tracks = JSON.parse(resp.body);
        const items = tracks.data || [];
        return {
            trackIds: items.map(t => String(t.id)),
            trackDiscNumbers: items.map(t => t.disk_number || 1),
            numberOfVolumes: album.nb_disk || 1,
            title: album.title || `Album ${albumId}`,
            artist: album.artist?.name || '',
            thumbnail: album.cover_medium || '',
            year: album.release_date?.split('-')[0] || '',
            genre: album.genres?.data?.[0]?.name || '',
            label: album.label || '',
        };
    }

    async getPlaylistTracks(playlistId) {
        const meta = await this.getPublicPlaylist(playlistId);
        return {
            trackIds: (meta.tracks?.data || []).map(t => String(t.id)),
            title: meta.title || `Playlist ${playlistId}`,
            artist: meta.creator?.name || '',
            thumbnail: meta.picture_medium || '',
        };
    }

    async getArtistAlbums(artistId) {
        const resp = await httpRequest(`${PUBLIC_API}/artist/${artistId}/albums?limit=500`, {
            headers: { 'User-Agent': 'Mozilla/5.0' },
        });
        if (resp.status !== 200) throw new Error(`Deezer: artist ${artistId} not found`);
        const json = JSON.parse(resp.body);
        return (json.data || []).map(a => String(a.id));
    }

    async getLabelAlbums(labelId) {
        throw new Error(`Deezer does not support label downloads via its public API (label id: ${labelId}). Use Qobuz for label downloads.`);
    }

    async getPrivateEpisodeInfo(episodeId) {
        this._requireLogin();
        const resp = await this._gwPost('episode.getData', { EPISODE_ID: String(episodeId) });
        const data = resp?.results?.DATA || resp?.results;
        if (!data || Object.keys(data).length === 0) throw new Error(`Failed to get episode info for ${episodeId}`);
        return data;
    }

    async getPrivateTrackInfo(trackId) {
        this._requireLogin();
        let trackInfo = null;

        try {
            const pageResp = await this._gwPost('deezer.pageTrack', { SNG_ID: String(trackId) });
            if (pageResp?.results?.DATA) {
                trackInfo = pageResp.results.DATA;
                if (pageResp.results.LYRICS) trackInfo.LYRICS = pageResp.results.LYRICS;
            }
        } catch (e) {}

        if (!trackInfo) {
            const resp = await this._gwPost('song.getData', { SNG_ID: String(trackId) });
            if (!resp || !resp.results) {
                throw new Error(`Failed to get private track info for ${trackId}`);
            }
            trackInfo = resp.results;
        }

        return trackInfo;
    }

    async getTrackUrl(trackId, quality = 1) {
        this._requireLogin();
        quality = Math.min(quality, this.maxQuality);
        const { format, ext } = QUALITY_MAP[quality] || QUALITY_MAP[0];
        const trackInfo = await this.getPrivateTrackInfo(trackId);

        // Podcast episodes use a direct stream URL — no encryption or CDN token needed
        if (trackInfo.SHOW_IS_DIRECT_STREAM === '1' && trackInfo.EPISODE_DIRECT_STREAM_URL) {
            return { url: trackInfo.EPISODE_DIRECT_STREAM_URL, ext: 'mp3', trackInfo, quality: 0, isDirect: true };
        }

        const token = trackInfo.TRACK_TOKEN;

        let url = null;

        if (token) {
            try {
                url = await this._getTokenUrl(token, format);
            } catch (e) {}
        }

        if (!url && trackInfo.FALLBACK?.TRACK_TOKEN) {
            try {
                url = await this._getTokenUrl(trackInfo.FALLBACK.TRACK_TOKEN, format);
            } catch (e) {}
        }

        if (!url) {
            const md5 = trackInfo.MD5_ORIGIN || trackInfo.FALLBACK?.MD5_ORIGIN;
            const mediaVersion = trackInfo.MEDIA_VERSION || trackInfo.FALLBACK?.MEDIA_VERSION || '1';
            if (!md5) {
                throw new Error(`Deezer: Track ${trackId} is not available for streaming. It may be region-locked or unavailable on your subscription tier.`);
            }
            const effectiveTrackId = trackInfo.FALLBACK?.SNG_ID || trackId;
            url = getEncryptedDeezerUrl(String(effectiveTrackId), md5, String(mediaVersion), quality);
        }

        const effectiveTrackIdForFilesize = trackInfo.FALLBACK?.SNG_ID || trackId;
        const fileSizeKey = `FILESIZE_${format}`;
        const fileSize = parseInt(
            (effectiveTrackIdForFilesize !== trackId && trackInfo.FALLBACK?.[fileSizeKey])
                ? trackInfo.FALLBACK[fileSizeKey]
                : (trackInfo[fileSizeKey] || '0'),
            10
        );

        if (fileSize === 0 && quality > 0) {
            return this.getTrackUrl(trackId, quality - 1);
        }
        if (fileSize === 0 && quality === 0) {
            throw new Error(`Track ${trackId} is not available on Deezer CDN.`);
        }

        return { url, ext, trackInfo, quality };
    }

    async downloadTrack(trackId, quality, destDir, onProgress = null, onInfo = null, settings = {}, createAlbumSubfolder = false) {
        this._requireLogin();

        const { url, ext, trackInfo } = await this.getTrackUrl(trackId, quality);

        const publicMeta = await this.getPublicTrack(trackId).catch(() => null);
        const albumMeta = publicMeta?.album?.id
            ? await this.getPublicAlbum(publicMeta.album.id).catch(() => null)
            : null;

        const title = publicMeta?.title || trackInfo.SNG_TITLE || `track_${trackId}`;
        const artist = publicMeta?.artist?.name || trackInfo.ART_NAME || 'Unknown';
        const albumartist = albumMeta?.artist?.name || artist;
        const album = publicMeta?.album?.title || '';
        const trackNum = publicMeta?.track_position || 0;
        const discNum = publicMeta?.disk_number || 1;
        const tracktotal = albumMeta?.nb_tracks || '';
        const year = publicMeta?.release_date?.split('-')[0] || '';
        const genre = albumMeta?.genres?.data?.[0]?.name || '';

        if (createAlbumSubfolder) {
            const safe = (s) => String(s || '').replace(/[<>:"/\\|?*]/g, '_');
            const folderTemplate = settings.filepaths_folder_format || '{albumartist} - {album} ({year})';
            const container = ext === 'flac' ? 'FLAC' : 'MP3';
            const bit_depth = ext === 'flac' ? '16' : '';
            const sampling_rate = ext === 'flac' ? '44.1' : '';
            const label = safe(albumMeta?.label || '');
            const vars = {
                albumartist: safe(albumartist), album: safe(album), title: safe(album),
                artist: safe(artist), year, genre: safe(genre), label,
                container, bit_depth, sampling_rate,
            };
            const folderName = buildFileName(folderTemplate, vars, true, 150) || safe(album).slice(0, 100) || `track_${trackId}`;
            destDir = path.join(destDir, folderName);
            fs.mkdirSync(destDir, { recursive: true });
        }

        const effectiveTrackNum = settings._renumber_override != null ? settings._renumber_override : trackNum;
        const effectiveTrackTotal = settings._renumber_total != null ? settings._renumber_total : (tracktotal || '');

        const trackTemplate = settings.filepaths_track_format || '{tracknumber:02}. {artist} - {title}';
        const fileName = buildFileName(trackTemplate, { title, artist, albumartist, album, tracknumber: effectiveTrackNum, discnumber: discNum, tracktotal: effectiveTrackTotal, year, genre, explicit: publicMeta?.explicit_lyrics ? ' (Explicit)' : '' }, settings.filepaths_restrict_characters !== false, settings.filepaths_truncate_to || 120) + '.' + ext;
        const destPath = path.join(destDir, fileName);

        if (onInfo) {
            onInfo({
                title,
                artist,
                album,
                thumbnail: publicMeta?.album?.cover_medium || '',
            });
        }

        const coverUrl = publicMeta?.album?.cover_xl || publicMeta?.album?.cover_medium || albumMeta?.cover_xl;
        const embedCover = settings.embed_cover !== false;
        const saveCover = !!settings.save_cover;

        const contribs = publicMeta?.contributors?.data || [];
        const byRole = (role) => contribs.filter(c => c.role === role).map(c => c.name).join(', ') || undefined;

        let lyrics;
        let lrcContent = null;
        try {
            const gwLyrics = await this._gwGetLyrics(trackId);
            const ldata = gwLyrics?.results;
            if (ldata && !gwLyrics.error?.length) {
                lyrics = ldata.LYRICS_TEXT || undefined;
                const syncJson = ldata.LYRICS_SYNC_JSON;
                if (Array.isArray(syncJson) && syncJson.length > 0) {
                    lrcContent = syncJson
                        .filter(l => l.lrc_timestamp)
                        .map(l => `${l.lrc_timestamp}${l.line}`)
                        .join('\n');
                }
            }
        } catch (e) { logger.warn('deezer', `Failed to fetch lyrics for track ${trackId}: ${e.message || e}`); }

        const { downloadCoverArt } = require('./tagger');
        let coverLocalPath = null;
        if (coverUrl && (embedCover || saveCover)) {
            coverLocalPath = await downloadCoverArt(coverUrl);
        }

        await downloadAndDecryptDeezer(url, String(trackId), destPath, {}, onProgress);

        if (saveCover && coverUrl) {
            const coverDest = path.join(destDir, 'cover.jpg');
            if (coverLocalPath) {
                try { fs.copyFileSync(coverLocalPath, coverDest); } catch {}
            } else {
                const tmp = await downloadCoverArt(coverUrl);
                if (tmp) { try { fs.copyFileSync(tmp, coverDest); } catch {} fs.unlink(tmp, () => {}); }
            }
        }

        try {
            await tagFile(destPath, {
                title: publicMeta?.title,
                artist: publicMeta?.artist?.name,
                album: publicMeta?.album?.title,
                albumartist: albumMeta?.artist?.name,
                date: publicMeta?.release_date,
                tracknumber: effectiveTrackNum,
                tracktotal: effectiveTrackTotal || undefined,
                discnumber: discNum,
                isrc: publicMeta?.isrc,
                genre,
                bpm: publicMeta?.bpm || undefined,
                composer: byRole('Composer'),
                lyricist: byRole('Lyricist'),
                producer: byRole('Producer'),
                engineer: byRole('Engineer'),
                copyright: albumMeta?.copyright,
                label: albumMeta?.label,
                upc: albumMeta?.upc,
                lyrics,
            }, embedCover ? coverLocalPath : null, settings.metadata_exclude || []);

            if (settings.save_lrc_files && lrcContent) {
                const lrcPath = destPath.replace(/\.[^/.]+$/, '.lrc');
                fs.writeFileSync(lrcPath, lrcContent, 'utf8');
            }
        } catch (e) { logger.warn('deezer', `Failed to tag file ${destPath}: ${e.message || e}`); }

        if (coverLocalPath) fs.unlink(coverLocalPath, () => {});

        return destPath;
    }

    async getTrackStream(trackId, quality = 1) {
        this._requireLogin();
        const { url, ext, isDirect } = await this.getTrackUrl(trackId, quality);
        if (isDirect) {
            // Podcast episode: stream directly without Deezer encryption
            const { PassThrough } = require('stream');
            const output = new PassThrough();
            httpGetStream(url).then(res => res.pipe(output)).catch(e => output.destroy(e));
            return { stream: output, ext };
        }
        return { stream: streamAndDecryptDeezer(url, String(trackId)), ext };
    }

    _requireLogin() {
        if (!this.loggedIn) throw new Error('DeezerClient: not logged in. Call login(arl) first.');
    }

    _cookie() {
        const parts = [`arl=${this.arl}`];
        if (this.sid) parts.push(`sid=${this.sid}`);
        return parts.join('; ');
    }

    async _gwGetLyrics(trackId) {
        const params = buildQueryString({
            method: 'song.getLyrics',
            api_version: '1.0',
            api_token: this.apiToken || 'null',
            sng_id: String(trackId),
        });
        const resp = await httpRequest(`${GW_BASE}?${params}`, {
            headers: {
                'Cookie': this._cookie(),
                'User-Agent': 'Mozilla/5.0',
            },
        });
        return JSON.parse(resp.body);
    }

    async _gwGet(method) {
        const params = buildQueryString({
            method,
            input: '3',
            api_version: '1.0',
            api_token: this.apiToken || 'null',
        });
        const resp = await httpRequest(`${GW_BASE}?${params}`, {
            headers: {
                'Cookie': this._cookie(),
                'User-Agent': 'Mozilla/5.0',
            },
        });

        if (resp.headers['set-cookie']) {
            const cookies = Array.isArray(resp.headers['set-cookie'])
                ? resp.headers['set-cookie']
                : [resp.headers['set-cookie']];
            for (const c of cookies) {
                const m = c.match(/sid=([^;]+)/);
                if (m) this.sid = m[1];
            }
        }

        return JSON.parse(resp.body);
    }

    async _gwPost(method, body) {
        const params = buildQueryString({
            method,
            input: '3',
            api_version: '1.0',
            api_token: this.apiToken || 'null',
        });
        const bodyStr = JSON.stringify(body);
        const resp = await httpRequest(`${GW_BASE}?${params}`, {
            method: 'POST',
            headers: {
                'Cookie': this._cookie(),
                'User-Agent': 'Mozilla/5.0',
                'Content-Type': 'application/json',
                'Content-Length': Buffer.byteLength(bodyStr),
            },
        }, bodyStr);
        return JSON.parse(resp.body);
    }

    async _getTokenUrl(trackToken, format) {
        const body = JSON.stringify({
            license_token: this.licenseToken,
            media: [{ type: 'FULL', formats: [{ cipher: 'BF_CBC_STRIPE', format }] }],
            track_tokens: [trackToken],
        });
        const resp = await httpRequest(`${MEDIA_BASE}/get_url`, {
            method: 'POST',
            headers: {
                'Cookie': this._cookie(),
                'User-Agent': 'Mozilla/5.0',
                'Content-Type': 'application/json',
                'Content-Length': Buffer.byteLength(body),
            },
        }, body);

        if (resp.status !== 200) return null;
        const json = JSON.parse(resp.body);
        return json?.data?.[0]?.media?.[0]?.sources?.[0]?.url || null;
    }
}

function extractDeezerTrackId(input) {
    if (!input) return null;
    if (/^\d+$/.test(input)) return input;
    // Support both track and episode URLs
    const m = input.match(/deezer\.com\/(?:[a-z]{2}\/)?(?:track|episode)\/(\d+)/);
    return m ? m[1] : null;
}

async function getTrackDuration(trackId) {
    try {
        const resp = await httpRequest(`${PUBLIC_API}/track/${trackId}`, {
            headers: { 'User-Agent': 'Mozilla/5.0' },
        });
        const data = JSON.parse(resp.body);
        return (data.duration || 0) * 1000;
    } catch {
        return 0;
    }
}

async function getEpisodeDuration(episodeId) {
    try {
        const resp = await httpRequest(`${PUBLIC_API}/episode/${episodeId}`, {
            headers: { 'User-Agent': 'Mozilla/5.0' },
        });
        const data = JSON.parse(resp.body);
        return (data.duration || 0) * 1000;
    } catch {
        return 0;
    }
}

async function getEpisodeStreamFromInfo(client, trackId, episodeInfo) {
    if (episodeInfo.SHOW_IS_DIRECT_STREAM === '1' && episodeInfo.EPISODE_DIRECT_STREAM_URL) {
        const directUrl = episodeInfo.EPISODE_DIRECT_STREAM_URL;
        const durationMs = (parseInt(episodeInfo.DURATION || '0', 10) * 1000) ||
            await getEpisodeDuration(trackId);

        const { PassThrough } = require('stream');
        const output = new PassThrough();
        httpGetStream(directUrl).then(res => res.pipe(output)).catch(e => output.destroy(e));
        return { stream: output, contentType: 'audio/mpeg', durationMs };
    }

    if (episodeInfo.TRACK_TOKEN) {
        const tokenUrl = await client._getTokenUrl(episodeInfo.TRACK_TOKEN, 'MP3_128').catch(() => null);
        if (tokenUrl) {
            const { PassThrough } = require('stream');
            const output = new PassThrough();
            httpGetStream(tokenUrl).then(res => res.pipe(output)).catch(e => output.destroy(e));
            const durationMs = (parseInt(episodeInfo.DURATION || '0', 10) * 1000) ||
                await getEpisodeDuration(trackId);
            return { stream: output, contentType: 'audio/mpeg', durationMs };
        }
    }

    throw new Error(`Deezer episode ${trackId} is not streamable — no direct URL or token available.`);
}

async function getTrackStream(url, settings) {
    const isEpisode = url && /\/episode\//.test(url);
    const trackId = extractDeezerTrackId(url);
    if (!trackId) throw new Error('Could not extract Deezer track/episode ID from URL');

    const arl = settings.deezer_arl;
    if (!arl) throw new Error('Deezer ARL token required. Set it in Settings → Deezer → ARL.');

    const quality = settings.deezer_quality ?? 1;

    const client = new DeezerClient();
    await client.login(arl);

    if (isEpisode) {
        const episodeInfo = await client.getPrivateEpisodeInfo(trackId);
        return getEpisodeStreamFromInfo(client, trackId, episodeInfo);
    }

    try {
        const [{ stream, ext }, durationMs] = await Promise.all([
            client.getTrackStream(trackId, quality),
            getTrackDuration(trackId),
        ]);

        const contentType = ext === 'flac' ? 'audio/flac' : 'audio/mpeg';
        return { stream, contentType, durationMs };
    } catch (trackErr) {
        // Some podcast episodes can still be routed through track URLs in the UI/queue.
        // If normal track playback fails, probe the episode API before surfacing an error.
        try {
            const episodeInfo = await client.getPrivateEpisodeInfo(trackId);
            return await getEpisodeStreamFromInfo(client, trackId, episodeInfo);
        } catch {
            throw trackErr;
        }
    }
}

module.exports = {
    DeezerClient,
    extractDeezerTrackId,
    getTrackStream,
    getTrackDuration,
};

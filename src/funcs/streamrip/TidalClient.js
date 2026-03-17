'use strict';

const https = require('https');
const http = require('http');
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { spawn } = require('child_process');
const { decryptTidalMQA } = require('./crypto');
const { downloadFile, downloadSegments } = require('./downloader');
const { tagFile } = require('./tagger');
const logger = require('../logger');

const BASE = 'https://api.tidalhifi.com/v1';
const AUTH_URL = 'https://auth.tidal.com/v1/oauth2';

const CLIENT_ID = '6BDSRdpK9hqEBTgU';
const CLIENT_SECRET = 'xeuPmY7nbpZ9IIbLAcQ93shka1VNheUAqN6IcszjTG8=';

const QUALITY_MAP = {
    0: 'LOW',
    1: 'HIGH',
    2: 'LOSSLESS',
    3: 'HI_RES',
};

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

function httpRequest(url, options = {}, bodyStr = null) {
    return new Promise((resolve, reject) => {
        const parsed = new URL(url);
        const transport = parsed.protocol === 'https:' ? https : http;
        const opts = {
            hostname: parsed.hostname,
            port: parsed.port || (parsed.protocol === 'https:' ? 443 : 80),
            path: parsed.pathname + parsed.search,
            method: options.method || 'GET',
            headers: { 'User-Agent': 'TIDAL_ANDROID/1039 okhttp/3.14.9', ...options.headers },
        };
        const req = transport.request(opts, (res) => {
            if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                return httpRequest(res.headers.location, options, bodyStr).then(resolve).catch(reject);
            }
            const chunks = [];
            res.on('data', (c) => chunks.push(c));
            res.on('end', () => resolve({
                status: res.statusCode,
                headers: res.headers,
                text: Buffer.concat(chunks).toString('utf-8'),
            }));
            res.on('error', reject);
        });
        req.on('error', reject);
        if (bodyStr) req.write(bodyStr);
        req.end();
    });
}

function buildQs(params) {
    return Object.entries(params)
        .filter(([, v]) => v != null)
        .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`)
        .join('&');
}

class TidalClient {
    constructor() {
        this.accessToken = null;
        this.refreshToken = null;
        this.tokenExpiry = 0;
        this.userId = null;
        this.countryCode = 'US';
        this.loggedIn = false;
    }

    async login(settings) {
        if (!settings.tidal_access_token) {
            throw new Error(
                'Tidal access token not set. Go to Settings → Tidal and follow the instructions to get your token ' 
            );
        }
        this.accessToken = settings.tidal_access_token;
        this.refreshToken = settings.tidal_refresh_token || null;
        this.tokenExpiry = parseFloat(settings.tidal_token_expiry || '0');
        this.userId = settings.tidal_user_id || null;
        this.countryCode = settings.tidal_country_code || 'US';

        if (this.tokenExpiry - Date.now() / 1000 < 86400) {
            if (this.refreshToken) {
                await this._refreshAccessToken();
            }
        } else {
            await this._verifyToken();
        }
        this.loggedIn = true;
    }

    async downloadTrack(trackId, quality, destDir, onProgress = null, onInfo = null, settings = {}, createAlbumSubfolder = false) {
        this._requireLogin();

        const downloadable = await this._getDownloadable(String(trackId), quality);
        const [meta, contributors] = await Promise.all([
            this._apiGet(`tracks/${trackId}`).then(r => JSON.parse(r.text)).catch(() => null),
            this._apiGet(`tracks/${trackId}/contributors`).then(r => JSON.parse(r.text)).catch(() => null),
        ]);
        const albumMeta = meta?.album?.id
            ? await this._apiGet(`albums/${meta.album.id}`).then(r => JSON.parse(r.text)).catch(() => null)
            : null;

        const title = meta?.title || `track_${trackId}`;
        const artist = meta?.artists?.map(a => a.name).join(', ') || 'Unknown';
        const albumartist = albumMeta?.artist?.name || artist;
        const album = meta?.album?.title || '';
        const trackNum = meta?.trackNumber || 0;
        const discNum = meta?.volumeNumber || 1;
        const tracktotal = albumMeta?.numberOfTracks || '';
        const disctotal = albumMeta?.numberOfVolumes || '';
        const year = meta?.album?.releaseDate?.split('-')[0] || '';
        const genre = albumMeta?.genre || '';

        if (createAlbumSubfolder) {
            const safe = (s) => String(s || '').replace(/[<>:"/\\|?*]/g, '_');
            const folderTemplate = settings.filepaths_folder_format || '{albumartist} - {album} ({year})';
            const tidalExt = downloadable.ext || 'flac';
            const container = tidalExt === 'm4a' ? 'AAC' : 'FLAC';
            const bit_depth = tidalExt === 'm4a' ? '' : (quality <= 2 ? '16' : '24');
            const sampling_rate = tidalExt === 'm4a' ? '' : (quality <= 2 ? '44.1' : quality === 3 ? '96' : '192');
            const label = safe(albumMeta?.label?.name || '');
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
        const fileName = buildFileName(trackTemplate, { title, artist, albumartist, album, tracknumber: effectiveTrackNum, discnumber: discNum, tracktotal: effectiveTrackTotal, disctotal, year, genre, explicit: meta?.explicit ? ' (Explicit)' : '' }, settings.filepaths_restrict_characters !== false, settings.filepaths_truncate_to || 120) + '.' + downloadable.ext;
        const destPath = path.join(destDir, fileName);

        if (onInfo) {
            onInfo({
                title,
                artist,
                album,
                thumbnail: albumMeta ? this._getAlbumArt(albumMeta) : '',
                quality: downloadable.actualQuality || QUALITY_MAP[quality],
            });
        }

        const coverUrl = albumMeta ? this._getAlbumArt(albumMeta) : null;
        const embedCover = settings.embed_cover !== false;
        const saveCover = !!settings.save_cover;

        const contribs = contributors?.items || [];
        const byRole = (role) => contribs.filter(c => c.role === role).map(c => c.name).join(', ') || undefined;

        let lyrics;
        let lrcContent = null;
        try {
            const lyricsJson = await this._fetchLyrics(trackId);
            if (lyricsJson) {
                lyrics = lyricsJson.lyrics || undefined;
                if (lyricsJson.subtitles) lrcContent = lyricsJson.subtitles;
            }
        } catch (e) { logger.warn('tidal', `Failed to fetch lyrics for track ${trackId}: ${e.message || e}`); }

        const { downloadCoverArt } = require('./tagger');
        let coverLocalPath = null;
        if (coverUrl && (embedCover || saveCover)) {
            coverLocalPath = await downloadCoverArt(coverUrl);
        }

        if (downloadable.type === 'dash') {
            await this._downloadDash(downloadable, destPath, onProgress);
        } else if (downloadable.type === 'direct') {
            const tmpPath = destPath + '.tmp';
            await downloadFile(downloadable.url, tmpPath, {
                Authorization: `Bearer ${this.accessToken}`,
            }, onProgress);

            if (downloadable.encKey) {
                const enc = fs.readFileSync(tmpPath);
                const dec = decryptTidalMQA(enc, downloadable.encKey);
                fs.writeFileSync(destPath, dec);
                fs.unlinkSync(tmpPath);
            } else {
                fs.renameSync(tmpPath, destPath);
            }
        }

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
                title: meta?.title,
                artist: meta?.artists?.map(a => a.name).join(', '),
                album: meta?.album?.title,
                albumartist: albumMeta?.artist?.name,
                date: meta?.album?.releaseDate,
                tracknumber: effectiveTrackNum,
                tracktotal: effectiveTrackTotal || undefined,
                discnumber: discNum,
                disctotal: albumMeta?.numberOfVolumes || undefined,
                isrc: meta?.isrc,
                genre,
                composer: byRole('Composer'),
                lyricist: byRole('Lyricist'),
                producer: byRole('Producer'),
                engineer: byRole('Engineer'),
                mixer: byRole('Mixer'),
                copyright: albumMeta?.copyright,
                label: albumMeta?.label?.name,
                upc: albumMeta?.upc,
                lyrics,
            }, embedCover ? coverLocalPath : null, settings.metadata_exclude || []);

            if (settings.save_lrc_files && lrcContent) {
                const lrcPath = destPath.replace(/\.[^/.]+$/, '.lrc');
                fs.writeFileSync(lrcPath, lrcContent, 'utf8');
            }
        } catch (e) { logger.warn('tidal', `Failed to tag file ${destPath}: ${e.message || e}`); }

        if (coverLocalPath) fs.unlink(coverLocalPath, () => {});

        return destPath;
    }

    async getTrackStream(trackId, settings, onTokenRefresh = null) {
        await this.login(settings);

        const downloadable = await this._getDownloadable(String(trackId), settings.tidal_quality ?? 2);

        if (downloadable.type === 'dash') {
            const { PassThrough } = require('stream');
            const output = new PassThrough();
            this._streamDash(downloadable, output).catch(e => output.destroy(e));
            const durationMs = downloadable.durationMs || 0;
            return { stream: output, contentType: 'audio/flac', durationMs };
        }

        const { PassThrough } = require('stream');
        const output = new PassThrough();
        const url = downloadable.url;

        const parsed = new URL(url);
        const transport = parsed.protocol === 'https:' ? https : http;
        const req = transport.get(url, {
            headers: { Authorization: `Bearer ${this.accessToken}`, 'User-Agent': 'TIDAL_ANDROID/1039 okhttp/3.14.9' },
        }, (res) => {
            if (downloadable.encKey) {
                const chunks = [];
                res.on('data', c => chunks.push(c));
                res.on('end', () => {
                    try {
                        const dec = decryptTidalMQA(Buffer.concat(chunks), downloadable.encKey);
                        output.end(dec);
                    } catch (e) {
                        output.destroy(e);
                    }
                });
            } else {
                res.pipe(output);
            }
        });
        req.on('error', e => output.destroy(e));

        const ext = downloadable.ext;
        const contentType = ext === 'flac' ? 'audio/flac' : 'audio/mp4';
        return { stream: output, contentType, durationMs: 0 };
    }

    async _getDownloadable(trackId, quality) {
        if (quality < 0) throw new Error(`No streamable format for track ${trackId}`);

        const requestedQuality = QUALITY_MAP[quality] || QUALITY_MAP[2];
        const params = {
            audioquality: requestedQuality,
            playbackmode: 'STREAM',
            assetpresentation: 'FULL',
        };
        const resp = await this._apiGet(`tracks/${trackId}/playbackinfopostpaywall`, params);
        if (resp.status === 401 || resp.status === 403) {
            if (quality > 0) return this._getDownloadable(trackId, quality - 1);
            throw new Error(`Tidal: no accessible quality for track ${trackId}`);
        }
        if (resp.status === 404) throw new Error(`Tidal: track ${trackId} not found`);
        const json = JSON.parse(resp.text);

        const manifestMime = json.manifestMimeType || '';
        const rawManifest = json.manifest ? Buffer.from(json.manifest, 'base64').toString('utf-8') : '';

        if (manifestMime === 'application/dash+xml') {
            return { ...this._parseDashManifest(rawManifest), actualQuality: json.audioQuality };
        }

        let manifest;
        try {
            manifest = JSON.parse(rawManifest);
        } catch {
            if (quality > 0) return this._getDownloadable(trackId, quality - 1);
            throw new Error(`Tidal: failed to parse manifest for ${trackId}`);
        }

        const encKey = manifest.encryptionType === 'NONE' ? null : (manifest.keyId || null);
        const codec = (manifest.codecs || '').toLowerCase();
        const ext = (codec === 'flac' || codec === 'mqa') ? 'flac' : 'm4a';

        if (!manifest.urls?.[0]) {
            if (quality > 0) return this._getDownloadable(trackId, quality - 1);
            throw new Error(`Tidal: no URL in manifest for ${trackId}`);
        }

        return { type: 'direct', url: manifest.urls[0], codec, ext, encKey, actualQuality: json.audioQuality };
    }

    _parseDashManifest(mpd) {
        const getAttr = (str, tag, attr) => {
            const re = new RegExp(`<${tag}[^>]+${attr}="([^"]+)"`);
            const m = str.match(re);
            return m ? m[1] : null;
        };

        const codec = getAttr(mpd, 'Representation', 'codecs') || 'flac';
        const initUrl = getAttr(mpd, 'SegmentTemplate', 'initialization') || '';
        const mediaTemplate = getAttr(mpd, 'SegmentTemplate', 'media') || '';
        const startNumber = parseInt(getAttr(mpd, 'SegmentTemplate', 'startNumber') || '1', 10);

        let totalSegments = 0;
        const sRe = /<S\s[^>]*>/g;
        let sm;
        while ((sm = sRe.exec(mpd)) !== null) {
            const r = parseInt((sm[0].match(/r="(\d+)"/) || ['', '0'])[1], 10);
            totalSegments += r + 1;
        }

        const segmentUrls = [];
        for (let i = 0; i < totalSegments; i++) {
            segmentUrls.push(mediaTemplate.replace('$Number$', String(startNumber + i)));
        }

        const ext = codec.toLowerCase() === 'flac' ? 'flac' : 'm4a';
        return { type: 'dash', initUrl, segmentUrls, codec, ext, needsRemux: codec.toLowerCase() === 'flac' };
    }

    async _downloadDash({ initUrl, segmentUrls, needsRemux }, destPath, onProgress) {
        const allUrls = [initUrl, ...segmentUrls];
        const tmp = destPath + '.tmp.m4a';

        await downloadSegments(allUrls, tmp, {
            Authorization: `Bearer ${this.accessToken}`,
        }, onProgress);

        if (needsRemux) {
            await this._remuxFlac(tmp, destPath);
            fs.unlinkSync(tmp);
        } else {
            fs.renameSync(tmp, destPath);
        }
    }

    async _streamDash({ initUrl, segmentUrls }, output) {
        const allUrls = [initUrl, ...segmentUrls];
        for (const url of allUrls) {
            await new Promise((resolve, reject) => {
                const parsed = new URL(url);
                const transport = parsed.protocol === 'https:' ? https : http;
                const req = transport.get(url, {
                    headers: { Authorization: `Bearer ${this.accessToken}`, 'User-Agent': 'TIDAL_ANDROID/1039 okhttp/3.14.9' },
                }, (res) => {
                    res.on('data', c => output.write(c));
                    res.on('end', resolve);
                    res.on('error', reject);
                });
                req.on('error', reject);
            });
        }
        output.end();
    }

    async _resolveHlsSegmentUrls(url) {
        const resp = await httpRequest(url);
        if (resp.status !== 200) throw new Error(`Tidal HLS: failed to fetch manifest (HTTP ${resp.status})`);
        const content = resp.text;
        const base = url.substring(0, url.lastIndexOf('/') + 1);

        // If this is a master playlist, pick the best stream and recurse
        if (content.includes('#EXT-X-STREAM-INF:')) {
            const bestUrl = this._parseBestM3U8Stream(content);
            if (!bestUrl) throw new Error('Tidal HLS: no streams found in master playlist');
            const resolved = bestUrl.startsWith('http') ? bestUrl : base + bestUrl;
            return this._resolveHlsSegmentUrls(resolved);
        }

        // Media playlist — extract segment URLs
        return content.split('\n')
            .map(l => l.trim())
            .filter(l => l && !l.startsWith('#'))
            .map(l => (l.startsWith('http') ? l : base + l));
    }

    async _streamHls(hlsUrl, output) {
        const segmentUrls = await this._resolveHlsSegmentUrls(hlsUrl);

        for (const url of segmentUrls) {
            await new Promise((resolve, reject) => {
                const parsed = new URL(url);
                const transport = parsed.protocol === 'https:' ? https : http;
                // Tidal CloudFront-signed URLs don't need the Authorization header
                const req = transport.get(url, {
                    headers: { 'User-Agent': 'Mozilla/5.0' },
                }, (res) => {
                    res.on('data', c => output.write(c));
                    res.on('end', resolve);
                    res.on('error', reject);
                });
                req.on('error', reject);
            });
        }
        output.end();
    }

    async getVideoStream(videoId, settings) {
        await this.login(settings);
        let durationMs = 0;
        try {
            const metaResp = await this._apiGet(`videos/${videoId}`);
            if (metaResp.status === 200) durationMs = (JSON.parse(metaResp.text).duration || 0) * 1000;
        } catch {}

        const hlsUrl = await this.getVideoStreamUrl(videoId, settings);
        const { PassThrough } = require('stream');
        const output = new PassThrough();

        // Prefer system ffmpeg; ffmpeg-static segfaults on HLS with some builds
        let ffmpegPath = 'ffmpeg';
        if (require('child_process').spawnSync('ffmpeg', ['-version'], { timeout: 2000 }).status !== 0) {
            try {
                const staticPath = require('ffmpeg-static');
                if (staticPath && require('fs').existsSync(staticPath)) ffmpegPath = staticPath;
            } catch {}
        }

        // Stream HLS → fragmented MP4 so Chromium can play it while downloading.
        const proc = spawn(ffmpegPath, [
            '-loglevel', 'error',
            '-user_agent', 'Mozilla/5.0',
            '-i', hlsUrl,
            '-c', 'copy',
            '-bsf:a', 'aac_adtstoasc',
            '-f', 'mp4',
            '-movflags', 'frag_keyframe+empty_moov+default_base_moof',
            '-frag_duration', '1000000',
            'pipe:1',
        ], { windowsHide: true });
        let stderr = '';
        proc.stderr.on('data', d => { stderr += d.toString(); });
        proc.on('error', e => output.destroy(e));
        proc.on('close', code => {
            if (code !== 0) output.destroy(new Error(`ffmpeg video remux failed: ${stderr.slice(0, 300)}`));
        });
        proc.stdout.pipe(output);

        return { stream: output, contentType: 'video/mp4', durationMs };
    }

    async _remuxFlac(src, dst) {
        let ffmpeg = 'ffmpeg';
        try { ffmpeg = require('ffmpeg-static'); } catch {}
        try { if (!ffmpeg || ffmpeg === 'ffmpeg') ffmpeg = require('@ffmpeg-installer/ffmpeg').path; } catch {}

        await new Promise((resolve, reject) => {
            const proc = spawn(ffmpeg, [
                '-y', '-loglevel', 'error',
                '-i', src, '-vn', '-c:a', 'flac', dst,
            ], { windowsHide: true });
            let stderr = '';
            proc.stderr.on('data', d => { stderr += d; });
            proc.on('close', code => {
                if (code !== 0) reject(new Error(`ffmpeg remux failed: ${stderr.slice(0, 200)}`));
                else resolve();
            });
            proc.on('error', reject);
        });
    }

    async _verifyToken() {
        const resp = await httpRequest('https://api.tidal.com/v1/sessions', {
            headers: { Authorization: `Bearer ${this.accessToken}` },
        });
        if (resp.status !== 200) throw new Error('Tidal: access token invalid');
        const json = JSON.parse(resp.text);
        this.userId = json.userId;
        this.countryCode = json.countryCode || this.countryCode;
    }

    async _refreshAccessToken() {
        const body = buildQs({
            client_id: CLIENT_ID,
            refresh_token: this.refreshToken,
            grant_type: 'refresh_token',
            scope: 'r_usr+w_usr+w_sub',
        });
        const auth = Buffer.from(`${CLIENT_ID}:${CLIENT_SECRET}`).toString('base64');
        const resp = await httpRequest(`${AUTH_URL}/token`, {
            method: 'POST',
            headers: {
                Authorization: `Basic ${auth}`,
                'Content-Type': 'application/x-www-form-urlencoded',
                'Content-Length': Buffer.byteLength(body),
            },
        }, body);

        if (resp.status !== 200) throw new Error('Tidal: token refresh failed');
        const json = JSON.parse(resp.text);
        this.accessToken = json.access_token;
        this.tokenExpiry = Math.floor(Date.now() / 1000) + json.expires_in;
    }

    async _apiGet(path, params = {}) {
        const p = { ...params, countryCode: this.countryCode, limit: 100 };
        const qs = buildQs(p);
        const url = `${BASE}/${path}${qs ? '?' + qs : ''}`;
        return httpRequest(url, {
            headers: { Authorization: `Bearer ${this.accessToken}` },
        });
    }

    async _fetchLyrics(trackId) {
        const qs = buildQs({ countryCode: this.countryCode });
        const hosts = [
            `https://api.tidal.com/v1/tracks/${trackId}/lyrics?${qs}`,
            `https://api.tidalhifi.com/v1/tracks/${trackId}/lyrics?${qs}`,
        ];
        for (const url of hosts) {
            const resp = await httpRequest(url, {
                headers: {
                    Authorization: `Bearer ${this.accessToken}`,
                    'X-Tidal-Token': CLIENT_ID,
                },
            });
            if (resp.status === 200) return JSON.parse(resp.text);
        }
        return null;
    }

    _getAlbumArt(albumMeta) {
        if (!albumMeta?.cover) return null;
        const uuid = albumMeta.cover.replace(/-/g, '/');
        return `https://resources.tidal.com/images/${uuid}/1280x1280.jpg`;
    }

    async getAlbumTracks(albumId) {
        this._requireLogin();
        const [tracksResp, albumResp] = await Promise.all([
            this._apiGet(`albums/${albumId}/tracks`, { countryCode: this.countryCode, limit: 100 }),
            this._apiGet(`albums/${albumId}`).catch(() => ({ text: '{}' })),
        ]);
        const tracks = JSON.parse(tracksResp.text);
        const album = JSON.parse(albumResp.text);
        const cover = album.cover ? `https://resources.tidal.com/images/${album.cover.replace(/-/g, '/')}/320x320.jpg` : '';
        const items = tracks.items || [];
        return {
            trackIds: items.map(t => String(t.id)),
            trackDiscNumbers: items.map(t => t.volumeNumber || 1),
            numberOfVolumes: album.numberOfVolumes || 1,
            title: album.title || `Album ${albumId}`,
            artist: album.artist?.name || '',
            thumbnail: cover,
            year: album.releaseDate?.split('-')[0] || '',
            genre: album.genre || '',
            label: album.label?.name || '',
        };
    }

    async getPlaylistTracks(playlistId) {
        this._requireLogin();
        const resp = await this._apiGet(`playlists/${playlistId}/tracks`, { countryCode: this.countryCode, limit: 200 });
        const json = JSON.parse(resp.text);
        return {
            trackIds: (json.items || []).map(t => String(t.id)),
            title: `Playlist ${playlistId}`,
            artist: '',
            thumbnail: '',
        };
    }

    async getArtistAlbums(artistId) {
        this._requireLogin();
        const [albumsResp, epsResp] = await Promise.all([
            this._apiGet(`artists/${artistId}/albums`, { countryCode: this.countryCode, limit: 500 }),
            this._apiGet(`artists/${artistId}/albums`, { countryCode: this.countryCode, filter: 'EPSANDSINGLES', limit: 500 }),
        ]);
        const albums = JSON.parse(albumsResp.text);
        const eps    = JSON.parse(epsResp.text);
        const all = [...(albums.items || []), ...(eps.items || [])];
        return all.map(a => String(a.id));
    }

    async getVideoStreamUrl(videoId, settings) {
        await this.login(settings);
        const streamResp = await this._apiGet(`videos/${videoId}/playbackinfopostpaywall`, {
            videoquality: 'HIGH',
            playbackmode: 'STREAM',
            assetpresentation: 'FULL',
        });
        if (streamResp.status !== 200) throw new Error(`Tidal: could not get video stream for ${videoId} (HTTP ${streamResp.status})`);
        const streamJson = JSON.parse(streamResp.text);
        const rawManifest = streamJson.manifest
            ? Buffer.from(streamJson.manifest, 'base64').toString('utf-8')
            : '';
        let videoUrl = null;
        if (rawManifest.startsWith('#EXTM3U')) {
            videoUrl = this._parseBestM3U8Stream(rawManifest);
        } else {
            try {
                const mf = JSON.parse(rawManifest);
                videoUrl = mf.urls?.[0] || null;
            } catch {}
        }
        if (!videoUrl) throw new Error(`Tidal: no video URL available for ${videoId}`);
        return videoUrl;
    }

    async downloadVideo(videoId, quality, destDir, onProgress = null, onInfo = null, settings = {}) {
        this._requireLogin();

        const metaResp = await this._apiGet(`videos/${videoId}`);
        if (metaResp.status === 404) throw new Error(`Tidal: video ${videoId} not found`);
        const meta = JSON.parse(metaResp.text);

        const title = meta.title || `video_${videoId}`;
        const artist = meta.artists?.map(a => a.name).join(', ') || 'Unknown';

        if (onInfo) {
            const imgUrl = meta.imageId
                ? `https://resources.tidal.com/images/${meta.imageId.replace(/-/g, '/')}/1280x720.jpg`
                : null;
            onInfo({ title, artist, album: '', thumbnail: imgUrl || '' });
        }

        const streamResp = await this._apiGet(`videos/${videoId}/playbackinfopostpaywall`, {
            videoquality: 'HIGH',
            playbackmode: 'STREAM',
            assetpresentation: 'FULL',
        });
        if (streamResp.status !== 200) throw new Error(`Tidal: could not get video stream for ${videoId}`);
        const streamJson = JSON.parse(streamResp.text);

        const rawManifest = streamJson.manifest
            ? Buffer.from(streamJson.manifest, 'base64').toString('utf-8')
            : '';

        let videoUrl = null;
        if (rawManifest.startsWith('#EXTM3U')) {
            videoUrl = this._parseBestM3U8Stream(rawManifest);
        } else {
            try {
                const mf = JSON.parse(rawManifest);
                videoUrl = mf.urls?.[0] || null;
            } catch {}
        }

        if (!videoUrl) throw new Error(`Tidal: no video URL available for ${videoId}`);

        const safe = s => String(s || '').replace(/[<>:"/\\|?*]/g, '_');
        const fileName = safe(title).slice(0, 120) + '.mp4';
        const destPath = path.join(destDir, fileName);
        fs.mkdirSync(destDir, { recursive: true });

        // videoUrl may be an HLS master or sub-playlist m3u8 — use ffmpeg to download HLS properly
        let ffmpeg = 'ffmpeg';
        try { ffmpeg = require('ffmpeg-static'); } catch {}
        try { if (!ffmpeg || ffmpeg === 'ffmpeg') ffmpeg = require('@ffmpeg-installer/ffmpeg').path; } catch {}

        await new Promise((resolve, reject) => {
            const proc = spawn(ffmpeg, [
                '-y', '-loglevel', 'error',
                '-user_agent', 'Mozilla/5.0',
                '-i', videoUrl,
                '-c', 'copy',
                destPath,
            ], { windowsHide: true });
            let stderr = '';
            let totalSeconds = meta.duration || 0;
            proc.stderr.on('data', (d) => {
                stderr += d.toString();
                if (onProgress && totalSeconds > 0) {
                    const m = d.toString().match(/time=(\d+):(\d+):(\d+)/);
                    if (m) {
                        const elapsed = parseInt(m[1]) * 3600 + parseInt(m[2]) * 60 + parseInt(m[3]);
                        onProgress(Math.min(elapsed / totalSeconds, 0.99));
                    }
                }
            });
            proc.on('error', err => reject(new Error(`ffmpeg spawn failed: ${err.message}`)));
            proc.on('close', code => {
                if (code !== 0) return reject(new Error(`ffmpeg failed (exit ${code}): ${stderr.substring(0, 300)}`));
                if (onProgress) onProgress(1);
                resolve();
            });
        });

        if (settings.save_cover && meta.imageId) {
            const imgUrl = `https://resources.tidal.com/images/${meta.imageId.replace(/-/g, '/')}/1280x720.jpg`;
            const { downloadCoverArt } = require('./tagger');
            const tmp = await downloadCoverArt(imgUrl);
            if (tmp) {
                try { fs.copyFileSync(tmp, path.join(destDir, 'thumbnail.jpg')); } catch {}
                fs.unlink(tmp, () => {});
            }
        }

        return destPath;
    }

    _parseBestM3U8Stream(m3u8) {
        const lines = m3u8.split('\n');
        let bestBandwidth = -1;
        let bestUrl = null;
        for (let i = 0; i < lines.length; i++) {
            const line = lines[i].trim();
            if (line.startsWith('#EXT-X-STREAM-INF:')) {
                const bwMatch = line.match(/BANDWIDTH=(\d+)/);
                const bw = bwMatch ? parseInt(bwMatch[1], 10) : 0;
                const nextLine = (lines[i + 1] || '').trim();
                if (nextLine && !nextLine.startsWith('#') && bw > bestBandwidth) {
                    bestBandwidth = bw;
                    bestUrl = nextLine;
                }
            }
        }
        return bestUrl;
    }

    _requireLogin() {
        if (!this.loggedIn) throw new Error('TidalClient: not logged in. Call login(settings) first.');
    }
}

function extractTidalTrackId(input) {
    if (!input) return null;
    if (/^\d+$/.test(input)) return input;
    const m = input.match(/tidal\.com\/(?:[a-z]{2}\/)?(?:browse\/)?track\/(\d+)/i);
    return m ? m[1] : null;
}

module.exports = { TidalClient, extractTidalTrackId };

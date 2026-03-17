'use strict';

const https = require('https');
const http = require('http');
const crypto = require('crypto');
const path = require('path');
const fs = require('fs');
const { downloadFile } = require('./downloader');
const { tagFile } = require('./tagger');
const logger = require('../logger');

const BASE_URL = 'https://www.qobuz.com/api.json/0.2';

function buildFileName(template, vars, restrictChars = true, truncateTo = 120) {
    let tpl = template.replace(/[\[(][^\])]*/g, (seg, offset, str) => {
        const close = seg[0] === '[' ? ']' : ')';
        const full = seg + (str[offset + seg.length] === close ? close : '');
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
    1: 5,
    2: 6,
    3: 7,
    4: 27,
    5: 5,
    6: 6,
    7: 7,
    27: 27,
};

function httpGet(url, headers = {}) {
    return new Promise((resolve, reject) => {
        const parsed = new URL(url);
        const transport = parsed.protocol === 'https:' ? https : http;
        const opts = {
            hostname: parsed.hostname,
            port: parsed.port || (parsed.protocol === 'https:' ? 443 : 80),
            path: parsed.pathname + parsed.search,
            method: 'GET',
            headers: { 'User-Agent': 'Mozilla/5.0', ...headers },
        };
        const req = transport.request(opts, (res) => {
            if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                return httpGet(res.headers.location, headers).then(resolve).catch(reject);
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
        req.end();
    });
}

function buildQs(params) {
    return Object.entries(params)
        .filter(([, v]) => v != null)
        .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(v)}`)
        .join('&');
}

class QobuzSpoofer {
    async getAppIdAndSecrets() {
        const loginPage = await httpGet('https://play.qobuz.com/login');
        const bundleMatch = loginPage.text.match(
            /<script src="(\/resources\/[\d.]+-[a-z]\d+\/bundle\.js)"><\/script>/
        );
        if (!bundleMatch) throw new Error('Qobuz: Could not find bundle.js URL');
        const bundleUrl = 'https://play.qobuz.com' + bundleMatch[1];

        const bundleResp = await httpGet(bundleUrl);
        const bundle = bundleResp.text;

        const appIdMatch = bundle.match(/production:\{api:\{appId:"(?<app_id>\d{9})"/);
        if (!appIdMatch) throw new Error('Qobuz: Could not extract app_id from bundle');
        const appId = appIdMatch.groups.app_id;

        const seedTzRegex = /[a-z]\.initialSeed\("(?<seed>[\w=]+)",window\.utimezone\.(?<tz>[a-z]+)\)/g;
        const secrets = {};
        let m;
        while ((m = seedTzRegex.exec(bundle)) !== null) {
            secrets[m.groups.tz] = [m.groups.seed];
        }

        const keys = Object.keys(secrets);
        if (keys.length >= 2) {
            const [first, second, ...rest] = keys;
            const reordered = [second, first, ...rest];
            const tzPat = reordered.map(tz => tz.charAt(0).toUpperCase() + tz.slice(1)).join('|');
            const infoRegex = new RegExp(
                `name:"\\w+\\/(?<tz>${tzPat})",info:"(?<info>[\\w=]+)",extras:"(?<extras>[\\w=]+)"`, 'g'
            );
            while ((m = infoRegex.exec(bundle)) !== null) {
                const tz = m.groups.tz.toLowerCase();
                if (secrets[tz]) {
                    secrets[tz].push(m.groups.info, m.groups.extras);
                }
            }
            for (const tz of Object.keys(secrets)) {
                try {
                    secrets[tz] = Buffer.from(secrets[tz].join('').slice(0, -44), 'base64').toString('utf-8');
                } catch {
                    secrets[tz] = '';
                }
            }
        }

        const secretList = Object.values(secrets).filter(Boolean);
        return { appId, secrets: secretList };
    }
}

class QobuzClient {
    constructor() {
        this.appId = null;
        this.secret = null;
        this.authToken = null;
        this.loggedIn = false;
        this._secrets = [];
    }

    async login(settings) {
        const emailOrUserId    = settings.qobuz_email_or_userid    ?? settings.emailOrUserId    ?? settings.email   ?? '';
        const passwordOrToken  = settings.qobuz_password_or_token  ?? settings.passwordOrToken  ?? settings.password ?? '';
        const useToken         = settings.qobuz_token_or_email     ?? settings.useToken          ?? false;
        const cachedAppId      = settings.qobuz_app_id             ?? settings.appId             ?? '';
        const cachedSecretsRaw = settings.qobuz_secrets            ?? settings.secrets           ?? '';

        if (!emailOrUserId || !passwordOrToken) {
            throw new Error('Qobuz credentials not set. Go to Settings → Qobuz and enter your email + password.');
        }

        let cachedSecrets = [];
        if (Array.isArray(cachedSecretsRaw)) {
            cachedSecrets = cachedSecretsRaw;
        } else if (typeof cachedSecretsRaw === 'string' && cachedSecretsRaw.startsWith('[')) {
            try { cachedSecrets = JSON.parse(cachedSecretsRaw); } catch {}
        } else if (typeof cachedSecretsRaw === 'string' && cachedSecretsRaw.length > 0) {
            cachedSecrets = cachedSecretsRaw.split(',').map(s => s.trim()).filter(Boolean);
        }

        const spoofer = new QobuzSpoofer();
        const { appId, secrets } = await spoofer.getAppIdAndSecrets();
        this.appId = appId;
        this._secrets = secrets;

        let params;
        if (useToken) {
            params = {
                user_id: emailOrUserId,
                user_auth_token: passwordOrToken,
                app_id: this.appId,
            };
        } else {
            params = {
                email: emailOrUserId,
                password: passwordOrToken,
                app_id: this.appId,
            };
        }

        const resp = await this._apiGet('user/login', params);
        if (resp.status === 401) throw new Error('Qobuz: Invalid credentials');
        if (resp.status === 400) throw new Error('Qobuz: Invalid app_id');
        const json = JSON.parse(resp.text);

        if (!json?.user?.credential?.parameters) {
            throw new Error('Qobuz: Free accounts are not eligible to download tracks.');
        }

        this.authToken = json.user_auth_token;

        this.secret = await this._getValidSecret(this._secrets);
        this.loggedIn = true;

        return {
            qobuz_app_id: this.appId,
            qobuz_secrets: JSON.stringify(this._secrets),
        };
    }

    async downloadTrack(trackId, quality, destDir, onProgress = null, onInfo = null, settings = {}, createAlbumSubfolder = false) {
        this._requireLogin();

        const resp = await this._requestFileUrl(String(trackId), quality);
        const json = JSON.parse(resp.text);

        if (!json.url) {
            const restrictions = json.restrictions;
            if (restrictions?.length) {
                const words = restrictions[0].code.split(/(?=[A-Z])/).join(' ').toLowerCase();
                throw new Error(`Qobuz: ${words}`);
            }
            throw new Error('Qobuz: Could not get download URL');
        }

        const streamUrl = json.url;
        const formatId = QUALITY_MAP[quality] || QUALITY_MAP[2];
        const ext = formatId === 5 ? 'mp3' : 'flac';

        const meta = await this.getTrackMetadata(String(trackId));
        const title = meta?.title || `track_${trackId}`;
        const artist = meta?.performer?.name || meta?.album?.artist?.name || 'Unknown';
        const albumartist = meta?.album?.artist?.name || artist;
        const album = meta?.album?.title || '';
        const trackNum = meta?.track_number || 0;
        const discNum = meta?.media_number || 1;
        const tracktotal = meta?.album?.tracks_count || '';
        const disctotal = meta?.album?.media_count || '';
        const year = meta?.album?.release_date_original?.split('-')[0] || '';
        const genre = meta?.album?.genre?.name || '';

        if (createAlbumSubfolder) {
            const safe = (s) => String(s || '').replace(/[<>:"/\\|?*]/g, '_');
            const folderTemplate = settings.filepaths_folder_format || '{albumartist} - {album} ({year})';
            const container = formatId === 5 ? 'MP3' : 'FLAC';
            const bit_depth = formatId === 5 ? '' : formatId === 6 ? '16' : '24';
            const sampling_rate = formatId === 5 ? '' : formatId === 6 ? '44.1' : formatId === 7 ? '96' : '192';
            const label = safe(meta?.album?.label?.name || '');
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
        const fileName = buildFileName(trackTemplate, { title, artist, albumartist, album, tracknumber: effectiveTrackNum, discnumber: discNum, tracktotal: effectiveTrackTotal, disctotal, year, genre, explicit: meta?.parental_warning ? ' (Explicit)' : '' }, settings.filepaths_restrict_characters !== false, settings.filepaths_truncate_to || 120) + '.' + ext;
        const destPath = path.join(destDir, fileName);

        if (onInfo) {
            onInfo({
                title,
                artist,
                album,
                thumbnail: meta?.album?.image?.small || '',
            });
        }

        const coverUrl = meta?.album?.image?.large;
        const embedCover = settings.embed_cover !== false;
        const saveCover = !!settings.save_cover;
        const parsePerformers = (str) => {
            const map = {};
            if (!str) return map;
            str.split(/\s*[\n,]\s*/).forEach(part => {
                const m = part.match(/^(.+?)\s*-\s*(.+)$/);
                if (m) {
                    const role = m[1].trim().toLowerCase();
                    const name = m[2].trim();
                    if (!map[role]) map[role] = [];
                    map[role].push(name);
                }
            });
            return map;
        };
        const perfMap = parsePerformers(meta.performers);
        const getRole = (role) => perfMap[role.toLowerCase()]?.join(', ') || undefined;

        const { downloadCoverArt } = require('./tagger');
        let coverLocalPath = null;
        if (coverUrl && (embedCover || saveCover)) {
            coverLocalPath = await downloadCoverArt(coverUrl);
        }

        await downloadFile(streamUrl, destPath, {
            'X-User-Auth-Token': this.authToken,
            'X-App-Id': String(this.appId),
        }, onProgress);

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
                title: meta.title,
                artist: meta.performer?.name,
                album: meta.album?.title,
                albumartist: meta.album?.artist?.name,
                date: meta.album?.release_date_original,
                tracknumber: effectiveTrackNum,
                tracktotal: effectiveTrackTotal || undefined,
                discnumber: discNum,
                disctotal: meta.album?.media_count || undefined,
                genre,
                isrc: meta.isrc,
                composer: meta.composer?.name || getRole('composer'),
                lyricist: getRole('lyricist'),
                producer: getRole('producer'),
                engineer: getRole('engineer'),
                mixer: getRole('mixer'),
                copyright: meta.copyright,
                label: meta.album?.label?.name,
                upc: meta.album?.upc,
                description: meta.description || undefined,
                purchase_date: meta.purchasable_at ? new Date(meta.purchasable_at * 1000).toISOString().slice(0, 10) : undefined,
                grouping: meta.album?.genre?.name || undefined,
            }, embedCover ? coverLocalPath : null, settings.metadata_exclude || []);
        } catch (e) { logger.warn('qobuz', `Failed to tag file ${destPath}: ${e.message || e}`); }

        if (coverLocalPath) fs.unlink(coverLocalPath, () => {});

        return destPath;
    }

    async getAlbumTracks(albumId) {
        this._requireLogin();
        const resp = await this._apiGet('album/get', { album_id: albumId, app_id: this.appId });
        const json = JSON.parse(resp.text);
        const items = json.tracks?.items || [];
        return {
            trackIds: items.map(t => String(t.id)),
            trackDiscNumbers: items.map(t => t.media_number || 1),
            numberOfVolumes: json.media_count || 1,
            title: json.title || `Album ${albumId}`,
            artist: json.artist?.name || '',
            thumbnail: json.image?.large || json.image?.small || '',
            year: json.release_date_original?.split('-')[0] || '',
            genre: json.genre?.name || '',
            label: json.label?.name || '',
            bit_depth: json.maximum_bit_depth || null,
            sampling_rate: json.maximum_sampling_rate || null,
        };
    }

    async getPlaylistTracks(playlistId) {
        this._requireLogin();
        const resp = await this._apiGet('playlist/get', { playlist_id: playlistId, extra: 'tracks', app_id: this.appId });
        const json = JSON.parse(resp.text);
        return {
            trackIds: (json.tracks?.items || []).map(t => String(t.id)),
            title: json.name || `Playlist ${playlistId}`,
            artist: '',
            thumbnail: '',
        };
    }

    async getArtistAlbums(artistId, filters = {}) {
        this._requireLogin();
        const resp = await this._apiGet('artist/get', {
            artist_id: artistId,
            extra: 'albums',
            limit: 500,
            app_id: this.appId,
        });
        const json = JSON.parse(resp.text);
        const artistName = json.name || '';
        let albums = (json.albums?.items || []).map(a => ({
            id: String(a.id),
            title: a.title || '',
            albumartist: a.artist?.name || artistName,
            sampling_rate: a.maximum_sampling_rate || 0,
            bit_depth: a.maximum_bit_depth || 0,
            explicit: a.parental_warning || false,
            nb_tracks: a.tracks_count || 0,
        }));

        albums = this._applyArtistFilters(albums, artistName, filters);
        return albums.map(a => a.id);
    }

    _applyArtistFilters(albums, artistName, filters) {
        let result = albums;

        if (filters.non_albums) {
            result = result.filter(a => a.nb_tracks > 1);
        }
        if (filters.extras) {
            result = result.filter(a => this._isNotExtra(a));
        }
        if (filters.features) {
            result = result.filter(a => a.albumartist === artistName);
        }
        if (filters.non_studio_albums) {
            result = result.filter(a => a.albumartist !== 'Various Artists' && this._isNotExtra(a));
        }
        if (filters.non_remaster) {
            result = result.filter(a => this._isRemaster(a));
        }
        if (filters.repeats) {
            result = this._filterRepeats(result);
        }

        return result;
    }

    _extraRe = /anniversary|deluxe|live|collector|demo|expanded|remix/i;
    _remasterRe = /re.?master(ed)?/i;
    _essenceRe = /^([^([]+)/;

    _isNotExtra(album) {
        return !this._extraRe.test(album.title);
    }

    _isRemaster(album) {
        return this._remasterRe.test(album.title);
    }

    _filterRepeats(albums) {
        const groups = {};
        for (const a of albums) {
            const m = this._essenceRe.exec(a.title);
            const key = (m ? m[1] : a.title).trim().toLowerCase();
            if (!groups[key]) groups[key] = [];
            groups[key].push(a);
        }
        const result = [];
        for (const group of Object.values(groups)) {
            const sorted = group.slice().sort((x, y) => {
                if (y.explicit !== x.explicit) return y.explicit - x.explicit;
                if (y.sampling_rate !== x.sampling_rate) return y.sampling_rate - x.sampling_rate;
                return (y.bit_depth || 0) - (x.bit_depth || 0);
            });
            result.push(sorted[0]);
        }
        return result;
    }

    async getLabelAlbums(labelId) {
        this._requireLogin();
        const resp = await this._apiGet('label/get', {
            label_id: labelId,
            extra: 'albums',
            limit: 500,
            app_id: this.appId,
        });
        const json = JSON.parse(resp.text);
        return (json.albums?.items || []).map(a => String(a.id));
    }

    async downloadBooklet(albumId, destDir) {
        this._requireLogin();
        try {
            const resp = await this._apiGet('album/get', { album_id: albumId, app_id: this.appId });
            const json = JSON.parse(resp.text);
            const goodies = json.goodies || [];
            const pdfs = goodies.filter(g =>
                g.file_format_id === 21 ||
                (g.url || '').toLowerCase().endsWith('.pdf') ||
                (g.original_url || '').toLowerCase().endsWith('.pdf')
            );
            if (pdfs.length === 0) return null;

            const pdfUrl = pdfs[0].original_url || pdfs[0].url;
            if (!pdfUrl) return null;

            const destPath = path.join(destDir, 'booklet.pdf');
            await downloadFile(pdfUrl, destPath, {
                'X-User-Auth-Token': this.authToken,
                'X-App-Id': String(this.appId),
            });
            return destPath;
        } catch (e) {
            return null;
        }
    }

    async getTrackMetadata(trackId) {
        const resp = await this._apiGet('track/get', {
            track_id: trackId,
            app_id: this.appId,
        });
        return JSON.parse(resp.text);
    }

    _requireLogin() {
        if (!this.loggedIn) throw new Error('QobuzClient: not logged in. Call login() first.');
    }

    _headers() {
        const h = { 'X-App-Id': String(this.appId) };
        if (this.authToken) h['X-User-Auth-Token'] = this.authToken;
        return h;
    }

    async _apiGet(endpoint, params = {}) {
        const qs = buildQs(params);
        const url = `${BASE_URL}/${endpoint}${qs ? '?' + qs : ''}`;
        return httpGet(url, this._headers());
    }

    async _requestFileUrl(trackId, quality) {
        const formatId = QUALITY_MAP[quality] || QUALITY_MAP[2];
        const unixTs = Date.now() / 1000;
        const rSig = `trackgetFileUrlformat_id${formatId}intentstreamtrack_id${trackId}${unixTs}${this.secret}`;
        const rSigHashed = crypto.createHash('md5').update(rSig, 'utf-8').digest('hex');

        const params = {
            request_ts: unixTs,
            request_sig: rSigHashed,
            track_id: trackId,
            format_id: formatId,
            intent: 'stream',
            app_id: this.appId,
        };
        return this._apiGet('track/getFileUrl', params);
    }

    async _testSecret(secret) {
        const unixTs = Date.now() / 1000;
        const rSig = `trackgetFileUrlformat_id27intentstreamtrack_id19512574${unixTs}${secret}`;
        const rSigHashed = crypto.createHash('md5').update(rSig, 'utf-8').digest('hex');
        const params = {
            request_ts: unixTs,
            request_sig: rSigHashed,
            track_id: '19512574',
            format_id: 27,
            intent: 'stream',
            app_id: this.appId,
        };
        const resp = await this._apiGet('track/getFileUrl', params);
        if (resp.status === 400) return null;
        return secret;
    }

    async _getValidSecret(secrets) {
        for (const s of secrets) {
            const result = await this._testSecret(s).catch(() => null);
            if (result) return result;
        }
        if (secrets.length > 0) {
            return secrets[0];
        }
        throw new Error('Qobuz: No app secrets available. Save your Qobuz credentials in Settings.');
    }
}

module.exports = { QobuzClient };

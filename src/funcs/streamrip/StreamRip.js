'use strict';

const fs = require('fs');
const path = require('path');
const { getNextDownloadOrder } = require('../downloadorder');
const { DeezerClient } = require('./DeezerClient');
const { QobuzClient } = require('./QobuzClient');
const { TidalClient } = require('./TidalClient');
const { convertAudio } = require('./converter');
const logger = require('../logger');

function extractId(url, patterns) {
    for (const re of patterns) {
        const m = url.match(re);
        if (m) return m[1];
    }
    return null;
}

const PARSERS = {
    deezer: {
        track:    u => extractId(u, [/deezer\.com\/(?:[a-z]{2}\/)?track\/(\d+)/, /^(\d+)$/]),
        album:    u => extractId(u, [/deezer\.com\/(?:[a-z]{2}\/)?album\/(\d+)/]),
        playlist: u => extractId(u, [/deezer\.com\/(?:[a-z]{2}\/)?playlist\/(\d+)/]),
        artist:   u => extractId(u, [/deezer\.com\/(?:[a-z]{2}\/)?artist\/(\d+)/]),
    },
    qobuz: {
        track:    u => extractId(u, [/play\.qobuz\.com\/track\/(\w+)/, /qobuz\.com\/[a-z-]+\/album\/[^/]+\/(\d+)/, /^(\d+)$/]),
        album:    u => extractId(u, [/play\.qobuz\.com\/album\/(\w+)/, /qobuz\.com\/[a-z-]+\/album\/[^/]+\/(\w+)/i, /^(\w+)$/i]),
        playlist: u => extractId(u, [/play\.qobuz\.com\/playlist\/(\d+)/, /qobuz\.com\/[a-z-]+\/playlist\/[^/]+\/(\d+)/i, /^(\d+)$/]),
        artist:   u => extractId(u, [/qobuz\.com\/[a-z-]+\/interpreter\/[^/]+\/(\d+)/i, /qobuz\.com\/[a-z-]+\/artist\/(\d+)/i]),
        label:    u => extractId(u, [/qobuz\.com\/[a-z-]+\/label\/[^/]+\/(\d+)/i]),
    },
    tidal: {
        track:    u => extractId(u, [/tidal\.com\/(?:[a-z]{2}\/)?(?:browse\/)?track\/(\d+)/i, /^(\d+)$/]),
        album:    u => extractId(u, [/tidal\.com\/(?:[a-z]{2}\/)?(?:browse\/)?album\/(\d+)/i]),
        playlist: u => extractId(u, [/tidal\.com\/(?:[a-z]{2}\/)?(?:browse\/)?playlist\/([a-z0-9-]+)/i]),
        artist:   u => extractId(u, [/tidal\.com\/(?:[a-z]{2}\/)?(?:browse\/)?artist\/(\d+)/i]),
        video:    u => extractId(u, [/tidal\.com\/(?:[a-z]{2}\/)?(?:browse\/)?video\/(\d+)/i]),
    },
};

function detectUrlType(url) {
    if (/\/video\//.test(url)) return 'video';
    if (/\/artist\//.test(url) || /\/interpreter\//.test(url)) return 'artist';
    if (/\/label\//.test(url)) return 'label';
    if (/\/track\//.test(url) || /^[\d]+$/.test(url)) return 'track';
    if (/\/album\//.test(url)) return 'album';
    if (/\/playlist\//.test(url)) return 'playlist';
    return 'track';
}

function makeProgressHandler(event, downloadOrder) {
    return (bytesDownloaded, totalBytes) => {
        if (totalBytes > 0) {
            const pct = Math.min(100, (bytesDownloaded / totalBytes) * 100);
            event.reply('download-update', { progress: pct, order: downloadOrder });
        }
    };
}

class StreamRip {
    constructor(settingsFilePath, app) {
        this.settingsFilePath = settingsFilePath;
        this.app = app;
    }

    log() {}

    _loadSettings() {
        try {
            return JSON.parse(fs.readFileSync(this.settingsFilePath, 'utf8'));
        } catch {
            return {};
        }
    }

    _getDownloadDir(settings, serviceName = null) {
        let dir = settings.downloadLocation || this.app.getPath('downloads');
        if (serviceName && settings.createPlatformSubfolders) {
            dir = path.join(dir, this._serviceLabel(serviceName));
        }
        return dir;
    }

    async _makeDeezerClient(settings) {
        const arl = settings.deezer_arl;
        if (!arl) throw new Error('Deezer ARL token not set. Add it in Settings → Deezer.');
        const client = new DeezerClient();
        await client.login(arl);
        return client;
    }

    async _makeQobuzClient(settings) {
        const client = new QobuzClient();
        const updated = await client.login(settings);
        this._patchSettings(updated);
        return client;
    }

    async _makeTidalClient(settings) {
        const client = new TidalClient();
        await client.login(settings);
        return client;
    }

    _patchSettings(patch) {
        try {
            const settings = this._loadSettings();
            Object.assign(settings, patch);
            fs.writeFileSync(this.settingsFilePath, JSON.stringify(settings, null, 2), 'utf8');
        } catch (e) {
            this.log('Failed to patch settings:', e.message);
        }
    }

    _qobuzFilters(settings) {
        return {
            extras:            !!settings.qobuz_filter_extras,
            repeats:           !!settings.qobuz_filter_repeats,
            non_albums:        !!settings.qobuz_filter_non_albums,
            features:          !!settings.qobuz_filter_features,
            non_studio_albums: !!settings.qobuz_filter_non_studio_albums,
            non_remaster:      !!settings.qobuz_filter_non_remaster,
        };
    }

    async handleDownload(event, data, serviceName) {
        const { url, quality } = data;
        const settings = this._loadSettings();
        const qualityNum = parseInt(quality, 10) || (serviceName === 'qobuz' ? 27 : serviceName === 'tidal' ? 3 : 1);
        const downloadOrder = getNextDownloadOrder();
        const baseDir = this._getDownloadDir(settings, serviceName);

        logger.info('download', `Starting ${this._serviceLabel(serviceName)} download: ${url} (quality=${qualityNum})`);

        event.reply('download-info', {
            title: `${this._serviceLabel(serviceName)} Download`,
            platform: serviceName,
            quality: String(qualityNum),
            order: downloadOrder,
        });

        const onInfo = (info) => {
            event.reply('download-info', { ...info, platform: serviceName, quality: String(qualityNum), order: downloadOrder });
        };

        const urlType = detectUrlType(url);

        try {
            let client;
            switch (serviceName) {
                case 'deezer': client = await this._makeDeezerClient(settings); break;
                case 'qobuz':  client = await this._makeQobuzClient(settings);  break;
                case 'tidal':  client = await this._makeTidalClient(settings);  break;
                default: throw new Error(`StreamRip: unsupported service "${serviceName}"`);
            }

            if (urlType === 'artist') {
                const artistId = PARSERS[serviceName]?.artist?.(url);
                if (!artistId) throw new Error(`Could not extract ${this._serviceLabel(serviceName)} artist ID from: ${url}`);
                await this._downloadArtist(event, client, serviceName, artistId, qualityNum, baseDir, settings, downloadOrder);

            } else if (urlType === 'label') {
                const labelId = PARSERS[serviceName]?.label?.(url);
                if (!labelId) throw new Error(`Could not extract label ID from: ${url}. Note: only Qobuz supports label downloads.`);
                await this._downloadLabel(event, client, serviceName, labelId, qualityNum, baseDir, settings, downloadOrder);

            } else if (urlType === 'video') {
                if (serviceName !== 'tidal') throw new Error('Video downloads are only supported for Tidal.');
                const videoId = PARSERS.tidal.video?.(url);
                if (!videoId) throw new Error(`Could not extract Tidal video ID from: ${url}`);
                await this._downloadVideo(event, client, videoId, qualityNum, baseDir, settings, downloadOrder);

            } else if (urlType === 'album' || urlType === 'playlist') {
                const collId = PARSERS[serviceName][urlType]?.(url);
                if (!collId) throw new Error(`Could not extract ${this._serviceLabel(serviceName)} ${urlType} ID from: ${url}`);
                await this._downloadCollection(event, client, urlType, collId, qualityNum, baseDir, settings, downloadOrder, serviceName);

            } else {
                const trackId = PARSERS[serviceName].track(url);
                if (!trackId) throw new Error(`Could not extract ${this._serviceLabel(serviceName)} track ID from: ${url}`);
                fs.mkdirSync(baseDir, { recursive: true });
                const onProgress = makeProgressHandler(event, downloadOrder);
                const destPath = await client.downloadTrack(trackId, qualityNum, baseDir, onProgress, onInfo, settings, true);
                if (typeof destPath === 'string') {
                    await convertAudio(destPath, settings).catch(() => {});
                }
                event.reply('download-complete', { order: downloadOrder });
                logger.info('download', `Track download complete: ${serviceName} trackId=${trackId}`);
                this.log('Download complete:', trackId);
            }
        } catch (err) {
            logger.error('download', `Download failed for ${serviceName} url=${url}: ${err.stack || err.message}`);
            this.log('Download error:', err.message);
            event.reply('download-error', {
                order: downloadOrder,
                error: err.message,
                fullLog: err.stack || err.message,
            });
        }
    }

    async _downloadArtist(event, client, serviceName, artistId, quality, baseDir, settings, downloadOrder) {
        event.reply('download-info', {
            title: `${this._serviceLabel(serviceName)} Artist`,
            order: downloadOrder,
        });

        const filters = serviceName === 'qobuz' ? this._qobuzFilters(settings) : {};
        let albumIds;
        if (serviceName === 'qobuz') {
            albumIds = await client.getArtistAlbums(artistId, filters);
        } else {
            albumIds = await client.getArtistAlbums(artistId);
        }

        logger.info('download', `Artist ${artistId} has ${albumIds.length} albums after filtering`);

        let completedAlbums = 0;
        for (const albumId of albumIds) {
            try {
                await this._downloadCollection(event, client, 'album', albumId, quality, baseDir, settings, downloadOrder, serviceName);
                completedAlbums++;
            } catch (err) {
                logger.warn('download', `Artist album ${albumId} failed: ${err.message}`);
            }
        }

        event.reply('download-complete', { order: downloadOrder });
        logger.info('download', `Artist download complete: ${completedAlbums}/${albumIds.length} albums`);
    }

    async _downloadLabel(event, client, serviceName, labelId, quality, baseDir, settings, downloadOrder) {
        event.reply('download-info', {
            title: `${this._serviceLabel(serviceName)} Label`,
            order: downloadOrder,
        });

        const albumIds = await client.getLabelAlbums(labelId);
        logger.info('download', `Label ${labelId} has ${albumIds.length} albums`);

        let completedAlbums = 0;
        for (const albumId of albumIds) {
            try {
                await this._downloadCollection(event, client, 'album', albumId, quality, baseDir, settings, downloadOrder, serviceName);
                completedAlbums++;
            } catch (err) {
                logger.warn('download', `Label album ${albumId} failed: ${err.message}`);
            }
        }

        event.reply('download-complete', { order: downloadOrder });
        logger.info('download', `Label download complete: ${completedAlbums}/${albumIds.length} albums`);
    }

    async _downloadVideo(event, client, videoId, quality, baseDir, settings, downloadOrder) {
        fs.mkdirSync(baseDir, { recursive: true });
        const onProgress = makeProgressHandler(event, downloadOrder);
        const onInfo = (info) => {
            event.reply('download-info', { ...info, order: downloadOrder });
        };

        await client.downloadVideo(videoId, quality, baseDir, onProgress, onInfo, settings);
        event.reply('download-complete', { order: downloadOrder });
        logger.info('download', `Video download complete: tidal videoId=${videoId}`);
    }

    _qualityTechVars(serviceName, quality) {
        const q = parseInt(quality, 10);
        if (serviceName === 'tidal') {
            if (q <= 1) return { container: 'AAC',  bit_depth: 16, sampling_rate: 44.1 };
            if (q === 2) return { container: 'FLAC', bit_depth: 16, sampling_rate: 44.1 };
            if (q === 3) return { container: 'FLAC', bit_depth: 24, sampling_rate: 96  };
            return             { container: 'FLAC', bit_depth: 24, sampling_rate: 192  };
        }
        if (serviceName === 'deezer') {
            if (q >= 9) return { container: 'FLAC', bit_depth: 16, sampling_rate: 44.1 };
            return             { container: 'MP3',  bit_depth: 16, sampling_rate: 44.1 };
        }
        if (serviceName === 'qobuz') {
            if (q === 1) return { container: 'MP3',  bit_depth: 16, sampling_rate: 44.1 };
            if (q === 2) return { container: 'FLAC', bit_depth: 16, sampling_rate: 44.1 };
            if (q === 3) return { container: 'FLAC', bit_depth: 24, sampling_rate: 96  };
            return              { container: 'FLAC', bit_depth: 24, sampling_rate: 192  };
        }
        return { container: '', bit_depth: '', sampling_rate: '' };
    }

    async _downloadCollection(event, client, type, id, quality, baseDir, settings, downloadOrder, serviceName) {
        let result;
        if (type === 'album') {
            result = await client.getAlbumTracks(id);
        } else {
            result = await client.getPlaylistTracks(id);
        }

        const { trackIds, trackDiscNumbers, numberOfVolumes, title: collTitle = type, artist = '', thumbnail = null } = result;

        event.reply('download-info', { title: collTitle, artist, thumbnail, order: downloadOrder });

        const techDefaults = this._qualityTechVars(serviceName, quality);
        const bitDepth     = result.bit_depth      ?? techDefaults.bit_depth;
        const samplingRate = result.sampling_rate   ?? techDefaults.sampling_rate;
        const container    = result.container       ?? techDefaults.container;

        const fmtRate = (r) => {
            if (!r && r !== 0) return '';
            const n = parseFloat(r);
            const khz = n > 1000 ? n / 1000 : n;
            return khz % 1 === 0 ? String(khz) : String(khz);
        };

        const safe = (s) => String(s || '').replace(/[<>:"/\\|?*]/g, '_');
        const folderTemplate = settings.filepaths_folder_format || '{albumartist} - {album} ({year})';
        const vars = {
            albumartist:   safe(artist),
            album:         safe(collTitle),
            title:         safe(collTitle),
            artist:        safe(artist),
            year:          result.year || '',
            genre:         safe(result.genre),
            label:         safe(result.label),
            container:     container || '',
            bit_depth:     bitDepth  != null ? String(bitDepth)      : '',
            sampling_rate: samplingRate != null ? fmtRate(samplingRate) : '',
        };
        const safeFolder = folderTemplate
            .replace(/\{(\w+)(?::[^}]*)?\}/g, (_, k) => vars[k] ?? '')
            .replace(/[<>:"/\\|?*]/g, '_')
            .replace(/\s+/g, ' ')
            .trim()
            .slice(0, 150) || safe(collTitle).slice(0, 100);
        const destDir = path.join(baseDir, safeFolder);
        fs.mkdirSync(destDir, { recursive: true });

        const totalTracks = trackIds.length;
        let completedTracks = 0;

        const renumber = type === 'playlist' && !!settings.renumber_playlist_tracks;

        const useDiscDirs = type === 'album'
            && settings.disc_subdirectories !== false
            && (numberOfVolumes > 1 || (trackDiscNumbers && Math.max(...trackDiscNumbers) > 1));

        for (let i = 0; i < trackIds.length; i++) {
            const trackId = trackIds[i];
            const discNum = trackDiscNumbers?.[i] || 1;
            let trackDir = destDir;
            if (useDiscDirs) {
                trackDir = path.join(destDir, `Disc ${discNum}`);
                fs.mkdirSync(trackDir, { recursive: true });
            }

            const trackSettings = renumber
                ? { ...settings, _renumber_override: i + 1, _renumber_total: totalTracks }
                : settings;

            const trackOnProgress = (bytes, total) => {
                const trackPct = total > 0 ? bytes / total : 0;
                const overall = ((completedTracks + trackPct) / totalTracks) * 100;
                event.reply('download-update', { progress: overall, order: downloadOrder });
            };
            const trackOnInfo = (info) => {
                event.reply('download-info', {
                    ...info,
                    title: `[${completedTracks + 1}/${totalTracks}] ${info.title || ''}`,
                    order: downloadOrder,
                });
            };
            try {
                const destPath = await client.downloadTrack(trackId, quality, trackDir, trackOnProgress, trackOnInfo, trackSettings);
                if (typeof destPath === 'string') {
                    await convertAudio(destPath, settings).catch(() => {});
                }
                completedTracks++;
            } catch (err) {
                logger.warn('download', `Collection track ${trackId} failed (${completedTracks + 1}/${totalTracks}): ${err.message}`);
                this.log(`Collection track ${trackId} error:`, err.message);
            }
        }

        if (type === 'album' && serviceName === 'qobuz' && settings.qobuz_download_booklets) {
            try {
                await client.downloadBooklet(id, destDir);
            } catch (err) {
                logger.warn('download', `Booklet download failed for album ${id}: ${err.message}`);
            }
        }

        event.reply('download-complete', { order: downloadOrder });
        logger.info('download', `Collection download complete: ${completedTracks}/${totalTracks} tracks`);
        this.log(`Collection download complete: ${completedTracks}/${totalTracks} tracks`);
    }

    async handleBatchDownload(event, data, serviceName) {
        const { filePath, quality } = data;
        const settings = this._loadSettings();
        const qualityNum = parseInt(quality, 10) || (serviceName === 'qobuz' ? 27 : serviceName === 'tidal' ? 3 : 1);
        const downloadOrder = getNextDownloadOrder();
        const destDir = this._getDownloadDir(settings, serviceName);
        fs.mkdirSync(destDir, { recursive: true });

        logger.info('download', `Starting ${this._serviceLabel(serviceName)} batch download from file: ${filePath} (quality=${qualityNum})`);

        let lines;
        try {
            lines = fs.readFileSync(filePath, 'utf8')
                .split(/\r?\n/)
                .map(l => l.trim())
                .filter(l => l && !l.startsWith('#'));
        } catch (e) {
            logger.error('download', `Could not read batch file ${filePath}: ${e.message}`);
            event.reply('download-error', {
                order: downloadOrder,
                error: `Could not read batch file: ${e.message}`,
            });
            return;
        }

        const totalTracks = lines.length;
        event.reply('download-info', {
            title: `Batch Download #${downloadOrder}`,
            downloadArtistOrUploader: this._serviceLabel(serviceName),
            order: downloadOrder,
            isBatch: true,
            totalTracks,
        });

        let completedTracks = 0;
        let client;
        try {
            switch (serviceName) {
                case 'deezer': client = await this._makeDeezerClient(settings); break;
                case 'qobuz':  client = await this._makeQobuzClient(settings);  break;
                case 'tidal':  client = await this._makeTidalClient(settings);  break;
                default: throw new Error(`StreamRip: unsupported service "${serviceName}"`);
            }
        } catch (err) {
            logger.error('download', `${this._serviceLabel(serviceName)} login failed for batch download: ${err.stack || err.message}`);
            event.reply('download-error', {
                order: downloadOrder,
                error: `Login failed: ${err.message}`,
            });
            return;
        }

        const trackProgressMap = {};

        for (const url of lines) {
            const trackId = this._extractIdForService(serviceName, url);
            if (!trackId) {
                this.log('Skipping unrecognized URL:', url);
                continue;
            }

            const onProgress = (bytes, total) => {
                const pct = total > 0 ? (bytes / total) * 100 : 0;
                trackProgressMap[trackId] = { trackId, progress: pct };
                event.reply('download-update', {
                    tracksProgress: Object.values(trackProgressMap),
                    order: downloadOrder,
                    completedTracks,
                    totalTracks,
                    isBatch: true,
                });
            };

            const onInfo = (info) => {
                trackProgressMap[trackId] = { ...info, trackId, progress: 0 };
            };

            try {
                const destPath = await client.downloadTrack(trackId, qualityNum, destDir, onProgress, onInfo, settings);
                if (typeof destPath === 'string') {
                    await convertAudio(destPath, settings).catch(() => {});
                }
                completedTracks++;
                delete trackProgressMap[trackId];
                event.reply('download-complete', { order: downloadOrder, completedTracks, totalTracks });
                logger.info('download', `Batch track complete: ${serviceName} trackId=${trackId} (${completedTracks}/${totalTracks})`);
            } catch (err) {
                logger.warn('download', `Batch track ${trackId} failed: ${err.message}`);
                this.log('Batch track error:', trackId, err.message);
                event.reply('download-error', {
                    order: downloadOrder,
                    error: `Track ${trackId}: ${err.message}`,
                });
            }
        }

        event.reply('download-complete', { order: downloadOrder, completedTracks, totalTracks });
        logger.info('download', `Batch download complete: ${completedTracks}/${totalTracks} tracks from ${this._serviceLabel(serviceName)}`);
    }

    _serviceLabel(service) {
        return { deezer: 'Deezer', qobuz: 'Qobuz', tidal: 'Tidal' }[service] || service;
    }

    _extractIdForService(service, url) {
        const p = PARSERS[service];
        if (!p) return null;
        const type = detectUrlType(url);
        return p[type] ? p[type](url) : (p.track ? p.track(url) : null);
    }
}

module.exports = StreamRip;

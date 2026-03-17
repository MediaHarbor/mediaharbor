const YTMusic = require('ytmusic-api');

const ytmusic = new YTMusic();
let _initialized = false;

ytmusic.client.interceptors.request.use((req) => {
    if (req.url) {
        const [path, qs] = req.url.split('?');
        if (qs) {
            const params = new URLSearchParams(qs);
            const key = params.get('key');
            if (!key || key === 'undefined') {
                params.delete('key');
            }
            const cleaned = params.toString();
            req.url = cleaned ? `${path}?${cleaned}` : path;
        }
    }
    req.headers = req.headers || {};
    if (!req.headers['Origin'])  req.headers['Origin']  = 'https://music.youtube.com';
    if (!req.headers['Referer']) req.headers['Referer'] = 'https://music.youtube.com/';
    return req;
});

async function ensureInitialized() {
    if (_initialized) return;

    try {
        await ytmusic.initialize();
        if (ytmusic.config?.INNERTUBE_CLIENT_VERSION) {
            _initialized = true;
            ytmusic.initialize = async () => {};
            return;
        }
    } catch (_) {}

    const fallback = {
        INNERTUBE_API_VERSION: 'v1',
        INNERTUBE_CLIENT_NAME: 'WEB_REMIX',
        INNERTUBE_CLIENT_VERSION: '1.20240101.01.00',
        INNERTUBE_CONTEXT_CLIENT_NAME: 67,
        GL: 'US',
        HL: 'en',
    };

    try {
        const res = await fetch('https://music.youtube.com/', {
            headers: {
                'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36',
                'Accept-Language': 'en-US,en;q=0.9',
            },
        });
        const html = await res.text();
        const pick = (key) => {
            const m = html.match(new RegExp('"' + key + '"\\s*:\\s*"([^"]+)"'));
            return m ? m[1] : null;
        };
        ytmusic.config = {
            ...ytmusic.config,
            ...fallback,
            INNERTUBE_API_KEY:      pick('INNERTUBE_API_KEY')      || '',
            INNERTUBE_API_VERSION:  pick('INNERTUBE_API_VERSION')  || fallback.INNERTUBE_API_VERSION,
            INNERTUBE_CLIENT_NAME:  pick('INNERTUBE_CLIENT_NAME')  || fallback.INNERTUBE_CLIENT_NAME,
            INNERTUBE_CLIENT_VERSION: pick('INNERTUBE_CLIENT_VERSION') || fallback.INNERTUBE_CLIENT_VERSION,
            GL: pick('GL') || fallback.GL,
            HL: pick('HL') || fallback.HL,
        };
    } catch (_) {
        ytmusic.config = { ...ytmusic.config, ...fallback, INNERTUBE_API_KEY: '' };
    }

    _initialized = true;
    ytmusic.initialize = async () => {};
}

function parseDuration(durationStr) {
    if (!durationStr) return 0;
    const parts = durationStr.split(':').reverse();
    let seconds = 0;
    for (let i = 0; i < parts.length; i++) {
        seconds += parseInt(parts[i]) * Math.pow(60, i);
    }
    return seconds;
}

async function getTrackList(contentId, contentType) {
    await ensureInitialized();
    try {
        let content;
        if (contentType === 'album') {
            content = await ytmusic.getAlbum(contentId);
            const totalDuration = content.songs.reduce((sum, track) => sum + (track.duration || 0), 0);
            const albumInfo = {
                title: content.name || 'Unknown Album',
                artist: content.artist?.name || 'Unknown Artist',
                coverUrl: content.thumbnails?.[content.thumbnails.length - 1]?.url || '',
                description: content.description || '',
                duration: totalDuration
            };
            const tracks = content.songs.map((track, index) => ({
                id: track.videoId || '',
                number: index + 1,
                title: track.name || 'Unknown Title',
                duration: track.duration || 0,
                quality: '256Kbps',
                playUrl: track.videoId ? `https://music.youtube.com/watch?v=${track.videoId}` : null
            }));
            return { album: albumInfo, tracks };
        } else if (contentType === 'playlist') {
            const browseId = contentId.startsWith('VL') ? contentId : 'VL' + contentId;
            const rawData = await ytmusic.constructRequest('browse', { browseId });

            const headerItems = [];
            _findAll(rawData, 'musicResponsiveHeaderRenderer', headerItems, 0);
            const header = headerItems[0] || {};
            const headerTitle = header.title?.runs?.[0]?.text || 'Unknown Playlist';
            const subtitleRuns = header.subtitle?.runs || [];
            const headerYear = subtitleRuns.map(r => r.text).find(t => /^\d{4}$/.test(t?.trim())) || '';
            const headerThumbUrl = (() => {
                const thumbs = header.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails;
                return thumbs ? thumbs[thumbs.length - 1]?.url : '';
            })();

            const renderers = [];
            _findAll(rawData, 'musicResponsiveListItemRenderer', renderers, 0);

            const tracks = renderers.map((item, index) => {
                const videoId = item.playlistItemData?.videoId || null;
                if (!videoId) return null;

                const col0runs = item.flexColumns?.[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
                const col1runs = item.flexColumns?.[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || [];
                const fixedRuns = item.fixedColumns?.[0]?.musicResponsiveListItemFixedColumnRenderer?.text?.runs || [];

                const title   = col0runs.map(r => r.text).join('') || 'Unknown Title';
                const artist  = col1runs.map(r => r.text).join('').trim() || '';
                const durStr  = fixedRuns.map(r => r.text).join('').trim() || '';

                return {
                    id: videoId,
                    number: index + 1,
                    title,
                    artist,
                    duration: parseDuration(durStr),
                    quality: 'HIGH',
                    playUrl: `https://music.youtube.com/watch?v=${videoId}`
                };
            }).filter(Boolean);

            const totalDuration = tracks.reduce((sum, t) => sum + t.duration, 0);
            const playlistInfo = {
                title: headerTitle,
                artist: 'YouTube Music',
                releaseDate: headerYear,
                coverUrl: headerThumbUrl || '',
                description: '',
                duration: totalDuration
            };
            return { album: playlistInfo, tracks };
        } else if (contentType === 'podcast') {
            const rawData = await ytmusic.constructRequest('browse', { browseId: contentId });

            const mf = rawData.microformat?.microformatDataRenderer;
            const thumbs = mf?.thumbnail?.thumbnails;
            const podcastInfo = {
                title: mf?.title || 'Unknown Podcast',
                artist: 'Podcast',
                coverUrl: thumbs ? thumbs[thumbs.length - 1]?.url : '',
                description: mf?.description || '',
                duration: 0
            };

            // Try multiple renderer types — YouTube Music uses different renderers over time
            let episodeItems = [];
            _findAll(rawData, 'musicMultiRowListItemRenderer', episodeItems, 0);

            if (episodeItems.length === 0) {
                _findAll(rawData, 'musicResponsiveListItemRenderer', episodeItems, 0);
            }
            if (episodeItems.length === 0) {
                _findAll(rawData, 'musicTwoRowItemRenderer', episodeItems, 0);
            }

            const tracks = episodeItems.map((item, index) => {
                // Handle musicMultiRowListItemRenderer
                const title = item.title?.runs?.[0]?.text
                    // Handle musicResponsiveListItemRenderer
                    || item.flexColumns?.[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text
                    // Handle musicTwoRowItemRenderer
                    || item.title?.runs?.[0]?.text
                    || 'Unknown Episode';

                const episodeThumbs = item.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails;
                const thumbUrl = episodeThumbs ? episodeThumbs[episodeThumbs.length - 1]?.url : null;

                const videoIds = [];
                _collectIds(item, { browseIds: [], videoIds }, 0);
                const videoId = videoIds[0] || null;

                return {
                    id: videoId || '',
                    number: index + 1,
                    title,
                    duration: 0,
                    quality: 'HIGH',
                    thumbnail: thumbUrl,
                    playUrl: videoId ? `https://music.youtube.com/watch?v=${videoId}` : null
                };
            }).filter(ep => ep.playUrl);

            return { album: podcastInfo, tracks };
        } else {
            throw new Error("Invalid content type. Must be 'album', 'playlist', or 'podcast'");
        }
    } catch (e) {
        throw new Error(`Error fetching ${contentType} tracks: ${e.message}`);
    }
}

const SEARCH_PARAMS = {
    song:     decodeURIComponent('EgWKAQIIAWoKEAkQAxAEEAoQBQ%3D%3D'),
    album:    decodeURIComponent('EgWKAQIYAWoKEAkQAxAEEAoQBQ%3D%3D'),
    playlist: decodeURIComponent('EgeKAQQoAEABagoQCRADEAQQChAF'),
    artist:   decodeURIComponent('EgWKAQIgAWoKEAkQAxAEEAoQBQ%3D%3D'),
    podcast:  decodeURIComponent('Eg%2BKAQwIABAAGAAgACgAMAFqChAEEAMQCRAFEAo%3D'),
};

function _findAll(obj, key, results, depth) {
    if (depth > 12 || !obj || typeof obj !== 'object') return;
    if (obj[key]) results.push(obj[key]);
    for (const v of Object.values(obj)) _findAll(v, key, results, depth + 1);
}

function _collectIds(obj, ids, depth) {
    if (depth > 12 || !obj || typeof obj !== 'object') return;
    if (obj.browseId) ids.browseIds.push(obj.browseId);
    if (obj.videoId)  ids.videoIds.push(obj.videoId);
    for (const v of Object.values(obj)) _collectIds(v, ids, depth + 1);
}

function _extractTexts(obj, texts, depth) {
    if (depth > 12 || !obj || typeof obj !== 'object') return;
    if (Array.isArray(obj.runs)) obj.runs.forEach(r => r.text && texts.push(r.text));
    for (const v of Object.values(obj)) _extractTexts(v, texts, depth + 1);
}

function _findThumbnails(obj, depth) {
    if (depth > 12 || !obj || typeof obj !== 'object') return null;
    if (obj.thumbnails && Array.isArray(obj.thumbnails)) return obj.thumbnails;
    for (const v of Object.values(obj)) {
        const found = _findThumbnails(v, depth + 1);
        if (found) return found;
    }
    return null;
}

function _parsePodcastItems(rawData) {
    const renderers = [];
    _findAll(rawData, 'musicResponsiveListItemRenderer', renderers, 0);

    return renderers.map(item => {
        const texts = [];
        _extractTexts(item, texts, 0);
        const typeLabel = texts[1] || '';
        if (typeLabel !== 'Podcast') return null;

        const ids = { browseIds: [], videoIds: [] };
        _collectIds(item, ids, 0);

        const podcastBrowseId = ids.browseIds.find(id => id.startsWith('MPSP'))
            || ids.browseIds[0]
            || null;

        const thumbs = _findThumbnails(item, 0);
        const thumbnail = thumbs ? thumbs[thumbs.length - 1]?.url : undefined;

        if (!podcastBrowseId) return null;
        return {
            id: podcastBrowseId,
            title: texts[0] || 'Unknown Podcast',
            thumbnail,
            url: `https://music.youtube.com/browse/${podcastBrowseId}`,
        };
    }).filter(Boolean);
}

async function searchYouTubeMusic(query, searchType = 'song') {
    await ensureInitialized();

    const validTypes = ['song', 'album', 'playlist', 'artist', 'podcast'];
    if (!validTypes.includes(searchType)) {
        throw new Error(`Invalid search type: ${searchType}. Valid types are ${validTypes.join(', ')}.`);
    }

    if (searchType === 'podcast') {
        const rawData = await ytmusic.constructRequest('search', {
            query,
            params: SEARCH_PARAMS.podcast,
        });
        return _parsePodcastItems(rawData);
    }

    if (searchType === 'song') {
        const results = await ytmusic.searchSongs(query);
        return results.map(r => ({
            id: r.videoId,
            title: r.name,
            artist: r.artist?.name,
            album: r.album?.name,
            thumbnail: r.thumbnails?.[r.thumbnails.length - 1]?.url,
            url: r.videoId ? `https://music.youtube.com/watch?v=${r.videoId}` : undefined,
            explicit: false,
            duration: typeof r.duration === 'number' ? r.duration : parseDuration(r.duration),
        })).filter(r => r.id);
    }

    if (searchType === 'artist') {
        const results = await ytmusic.searchArtists(query);
        return results.map(r => ({
            id: r.artistId,
            name: r.name,
            title: r.name,
            thumbnail: r.thumbnails?.[r.thumbnails.length - 1]?.url,
            browseId: r.artistId,
            url: r.artistId ? `https://music.youtube.com/browse/${r.artistId}` : undefined,
        })).filter(r => r.id);
    }

    if (searchType === 'album') {
        const results = await ytmusic.searchAlbums(query);
        return results.map(r => ({
            id: r.albumId,
            title: r.name,
            artist: r.artist?.name,
            thumbnail: r.thumbnails?.[r.thumbnails.length - 1]?.url,
            browseId: r.albumId,
            url: r.albumId ? `https://music.youtube.com/browse/${r.albumId}` : undefined,
            explicit: false,
        })).filter(r => r.id);
    }

    if (searchType === 'playlist') {
        const results = await ytmusic.searchPlaylists(query);
        return results.map(r => ({
            id: r.playlistId,
            title: r.name,
            owner: r.artist?.name,
            thumbnail: r.thumbnails?.[r.thumbnails.length - 1]?.url,
            browseId: r.playlistId ? `VL${r.playlistId}` : undefined,
            url: r.playlistId ? `https://music.youtube.com/browse/VL${r.playlistId}` : undefined,
        })).filter(r => r.id);
    }

    return [];
}


async function getArtistAlbums(browseId) {
    await ensureInitialized();
    try {
        const items = await ytmusic.getArtistAlbums(browseId);
        return Array.isArray(items) ? items : [];
    } catch (_) {
        return [];
    }
}

module.exports = { getTrackList, searchYouTubeMusic, getArtistAlbums };

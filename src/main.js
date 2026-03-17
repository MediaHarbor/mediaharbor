const {
    downloadAndInstallBento4: downloadAndInstallBento4
} = require("./funcs/installers/bento4installer");
const {
    downloadAndInstallFFmpeg: downloadAndInstallFFmpeg
} = require("./funcs/installers/ffmpegInstaller");
const {
    downloadAndInstallGit: downloadAndInstallGit
} = require("./funcs/installers/gitInstaller");
const {
    downloadAndInstallPython: downloadAndInstallPython
} = require("./funcs/installers/pythonInstaller");
const logger = require("./funcs/logger");
const {
    clipboard: clipboard,
    Menu: Menu,
    MenuItem: MenuItem,
    app: app,
    BrowserWindow: BrowserWindow,
    ipcMain: ipcMain,
    dialog: dialog,
    shell: shell,
    protocol: protocol,
    net: net
} = require("electron");

protocol.registerSchemesAsPrivileged([
    {
        scheme: 'mhfile',
        privileges: { secure: true, standard: true, supportFetchAPI: true, stream: true, bypassCSP: true }
    }
]);
const {
    exec: exec,
    spawn: spawn
} = require("child_process");
const util = require("util");
const execPromise = util.promisify(exec);
const os = require("os");
const path = require("path");
const fs = require("fs");
const http = require("http");
const {
    MediaScanner: MediaScanner
} = require("./funcs/mediaScanner");
const mediaScanner = new MediaScanner;
const { FileWatcherService } = require("./funcs/fileWatcher");
const fileWatcher = new FileWatcherService(mediaScanner);
const UpdateChecker = require("./funcs/updatechecker");
const {
    getDefaultSettings: getDefaultSettings
} = require("./funcs/defaults.js");
const {
    handleYtDlpDownload: handleYtDlpDownload,
    handleYtDlpMusicDownload: handleYtDlpMusicDownload,
    cancelDownload: cancelDownload
} = require("./funcs/yt_dlp_downloaders");
const GamRip = require("./funcs/gamRip");
const StreamRip = require("./funcs/streamrip/StreamRip");
const {
    setupSettingsHandlers: setupSettingsHandlers,
    loadSettings: loadSettingsFull,
    saveSettings: saveSettingsFull
} = require("./funcs/settings");
const { ensureVenv, getVenvPython, isVenvReady } = require("./funcs/venvManager");
const { registerDownloadHandlers } = require("./funcs/downloadsHandlers");
const settingsFilePath = path.join(app.getPath("userData"), "mh-settings.json");
let settings = loadTheSettings();
const downloadsDatabasePath = settings.downloads_database;
const failedDownloadsDatabasePath = settings.failed_downloads_database;
const axios = require("axios");
const YTMusic = require('ytmusic-api');
const ytmusic = new YTMusic();
const SpotifyAPI = require('./funcs/apis/spotifyapi.js');
const librespotService = require('./funcs/apis/librespotService.js');
const TidalAPI = require('./funcs/apis/tidalapi.js');
const { searchQobuz } = require('./funcs/apis/qobuzapi.js');
const { searchAppleMusic } = require('./funcs/apis/applemusicapi.js');
const { searchYouTubeMusic } = require('./funcs/apis/ytmusicsearchapi.js');
const { searchYouTube } = require('./funcs/apis/ytsearchapi.js');
const { searchTracks: searchDeezerTracks, searchAlbums: searchDeezerAlbums, searchArtists: searchDeezerArtists, searchPlaylists: searchDeezerPlaylists, searchPodcasts: searchDeezerPodcasts, searchEpisodes: searchDeezerEpisodes } = require('./funcs/apis/deezerapi.js');
const { preResolve: ytPreResolve } = require('./funcs/apis/ytaudiostream.js');
const tidalStreamService = require('./funcs/streamrip/TidalClient.js');
const deezerStreamService = require('./funcs/streamrip/DeezerClient.js');
const appleMusicService = require('./funcs/apis/appleMusicService.js');
if (process.platform === "darwin" || process.platform === "linux") {
    import("fix-path").then(module => {
        module.default()
    }).catch((err) => {
        logger.warn('system', `fix-path failed: ${err.message || err}`);
    })
}

process.on('uncaughtException', (err) => {
    logger.error('system', `Uncaught exception: ${err.stack || err.message || err}`);
});
process.on('unhandledRejection', (reason) => {
    logger.error('system', `Unhandled rejection: ${reason instanceof Error ? reason.stack || reason.message : reason}`);
});

const _audioStreams = new Map(); // id → { stream, contentType }
let _audioStreamPort = 0;
const _audioStreamServer = http.createServer((req, res) => {
    const id = req.url?.replace(/^\//, '');

    const entry = _audioStreams.get(id);
    if (!entry) {
        res.writeHead(404);
        res.end('Not found');
        return;
    }
    res.writeHead(200, {
        'Content-Type': entry.contentType || 'audio/mp4',
        'Accept-Ranges': 'none',
        'Access-Control-Allow-Origin': '*',
    });
    entry.stream.pipe(res);
    entry.stream.on('end', () => _audioStreams.delete(id));
    entry.stream.on('error', () => _audioStreams.delete(id));
});
_audioStreamServer.listen(0, '127.0.0.1', () => {
    _audioStreamPort = _audioStreamServer.address().port;
});

function setupContextMenu(win) {
    win.webContents.on("context-menu", (event, params) => {
        if (params.isEditable) {
            const hasSelection = params.selectionText.trim().length > 0;
            const clipboardHasText = clipboard.availableFormats().includes("text/plain");
            const contextMenu = new Menu;
            contextMenu.append(new MenuItem({
                label: "Cut",
                role: "cut",
                enabled: hasSelection
            }));
            contextMenu.append(new MenuItem({
                label: "Copy",
                role: "copy",
                enabled: hasSelection
            }));
            contextMenu.append(new MenuItem({
                label: "Paste",
                role: "paste",
                enabled: clipboardHasText
            }));
            contextMenu.append(new MenuItem({
                type: "separator"
            }));
            contextMenu.append(new MenuItem({
                label: "Select All",
                role: "selectall"
            }));
            contextMenu.popup({
                window: win
            })
        }
    })
}

function loadTheSettings() {
    try {
        const settingsData = fs.readFileSync(settingsFilePath, "utf8");
        return JSON.parse(settingsData)
    } catch (err) {
        logger.warn('settings', `Could not load settings file, using defaults: ${err.message || err}`);
        return getDefaultSettings()
    }
}

function getSpotifyCredentials() {
    return { clientId: settings.spotify_client_id || '', clientSecret: settings.spotify_client_secret || '' };
}
function getTidalCredentials() {
    return { clientId: settings.tidal_client_id || '', clientSecret: settings.tidal_client_secret || '' };
}
async function getTidalUserApiToken() {
    if (!settings.tidal_access_token) return null;
    const { TidalClient } = tidalStreamService;
    const client = new TidalClient();
    try {
        await client.login(settings);
        if (client.accessToken !== settings.tidal_access_token) {
            settings.tidal_access_token = client.accessToken;
            settings.tidal_token_expiry = String(client.tokenExpiry);
            try { fs.writeFileSync(settingsFilePath, JSON.stringify(settings, null, 2)); } catch (e) { logger.error('settings', `Failed to save tidal token: ${e.message || e}`); }
        }
        return { token: client.accessToken, countryCode: settings.tidal_country_code || 'US' };
    } catch { return null; }
}
function getYouTubeApiKey() {
    return settings.youtube_api_key || '';
}

ipcMain.handle("scan-directory", async (event, directory, options = {}) => {
    const { force = false } = options;
    return await mediaScanner.scanDirectory(directory, event, { force });
});
ipcMain.handle("get-settings", async () => {
    try {
        return await loadSettingsFull();
    } catch {
        return loadTheSettings();
    }
});
ipcMain.handle("set-settings", async (event, newSettings) => {
    if (!newSettings || typeof newSettings !== 'object' || Array.isArray(newSettings)) {
        return { success: false, error: 'Invalid settings payload' };
    }
    const validKeys = new Set(Object.keys(getDefaultSettings()));
    const filteredSettings = Object.fromEntries(
        Object.entries(newSettings).filter(([key]) => validKeys.has(key))
    );
    try {
        await saveSettingsFull(filteredSettings);
        settings = { ...settings, ...filteredSettings };
        if (filteredSettings.downloadLocation) {
            fileWatcher.changeDirectory(filteredSettings.downloadLocation);
        }
        if (settings.spotify_cookies_path && !librespotService.isLoggedIn()) {
            librespotService.loginFromCookies(settings.spotify_cookies_path).catch((e) => {
                logger.warn('settings', `Spotify cookie login failed: ${e.message || e}`);
            });
        }
        return { success: true };
    } catch (err) {
        return { success: false, error: String(err) };
    }
});
try {
    const settingsData = fs.readFileSync(settingsFilePath, "utf8");
    settings = JSON.parse(settingsData)
} catch (error) {
    logger.warn('settings', `Failed to reload settings: ${error.message || error}`);
}
ipcMain.handle("dialog:openFile", async event => {
    const currentWindow = BrowserWindow.fromWebContents(event.sender);
    const {
        canceled: canceled,
        filePaths: filePaths
    } = await dialog.showOpenDialog(currentWindow, {
        properties: ["openFile"],
        filters: [
            { name: "Supported files", extensions: ["txt", "wvd"] },
            { name: "All files", extensions: ["*"] }
        ],
        title: "Select File"
    });
    if (canceled) {
        return null
    } else {
        return filePaths[0]
    }
});
ipcMain.handle("dialog:openFolder", async event => {
    const currentWindow = BrowserWindow.fromWebContents(event.sender);
    const {
        canceled: canceled,
        filePaths: filePaths
    } = await dialog.showOpenDialog(currentWindow, {
        properties: ["openDirectory", "createDirectory"],
        title: "Select Folder Location"
    });
    if (canceled) {
        return null
    } else {
        return filePaths[0]
    }
});
ipcMain.handle("perform-search", async (event, {
    platform: platform,
    query: query,
    type: type
}) => {
    try {
        let results;
        switch (platform?.toLowerCase()) {
            case "spotify":
                if (librespotService.isLoggedIn() && type !== 'episode' && type !== 'audiobook') {
                    results = await librespotService.search(query, type || 'track');
                    break;
                }
                const spotifyApi = new SpotifyAPI(getSpotifyCredentials());
                switch (type) {
                    case "track":
                        results = await spotifyApi.searchTracks(query);
                        break;
                    case "album":
                        results = await spotifyApi.searchAlbums(query);
                        break;
                    case "artist":
                        results = await spotifyApi.searchArtists(query);
                        break;
                    case "playlist":
                        results = await spotifyApi.searchPlaylists(query);
                        break;
                    case "episode":
                        results = await spotifyApi.searchEpisodes(query);
                        break;
                    case "podcast":
                    case "show":
                        results = await spotifyApi.searchPodcasts(query);
                        break;
                    case "audiobook":
                        results = await spotifyApi.searchAudiobooks(query);
                        break;
                    default:
                        results = await spotifyApi.searchTracks(query);
                }
                break;
            case "tidal": {
                const tidalApi = new TidalAPI(getTidalCredentials());
                const tidalAuth = await getTidalUserApiToken();
                if (tidalAuth) {
                    results = await tidalApi.searchV1(query, type, tidalAuth.countryCode, tidalAuth.token);
                } else {
                    switch (type) {
                        case "track":    results = await tidalApi.searchTracks(query, 'US'); break;
                        case "album":    results = await tidalApi.searchAlbums(query, 'US'); break;
                        case "artist":   results = await tidalApi.searchArtists(query, 'US'); break;
                        case "playlist": results = await tidalApi.searchPlaylists(query, 'US'); break;
                        default:         results = await tidalApi.searchTracks(query, 'US');
                    }
                }
                break;
            }
            case "deezer":
                switch (type) {
                    case "track":
                        results = await searchDeezerTracks(query);
                        break;
                    case "album":
                        results = await searchDeezerAlbums(query);
                        break;
                    case "artist":
                        results = await searchDeezerArtists(query);
                        break;
                    case "playlist":
                        results = await searchDeezerPlaylists(query);
                        break;
                    case "podcast":
                        results = await searchDeezerPodcasts(query);
                        break;
                    case "episode":
                        results = await searchDeezerEpisodes(query);
                        break;
                    default:
                        results = await searchDeezerTracks(query);
                }
                break;
            case "qobuz":
                results = await searchQobuz(query, type || 'track');
                break;
            case "applemusic":
                results = await searchAppleMusic(query, type || 'track');
                break;
            case "youtubemusic":
                results = await searchYouTubeMusic(query, type || 'song');
                break;
            case "youtube":
                results = await searchYouTube(query, type || 'video', getYouTubeApiKey());
                break;
            default:
                throw new Error(`Invalid platform: "${platform}". Supported platforms: spotify, tidal, deezer, qobuz, applemusic, youtubemusic, youtube`);
        }
        return { results, platform };
    } catch (error) {
        throw error;
    }
});
ipcMain.handle("play-media", async (event, {
    url: url,
    platform: platform
}) => {
    if (!url) throw new Error("URL cannot be null");
    try {

    if (platform === "youtube") {
        const { getYouTubeVideoStream } = require('./funcs/apis/ytaudiostream.js');
        const { stream: videoStream, contentType } = await getYouTubeVideoStream(url);
        const streamId = Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
        _audioStreams.set(streamId, { stream: videoStream, contentType: contentType || 'video/mp4' });
        const streamUrl = `http://127.0.0.1:${_audioStreamPort}/${streamId}`;
        event.sender.send("stream-ready", { streamUrl, platform: 'youtube' });
        return { streamUrl, platform: 'youtube' };
    }

    if (platform === "youtubeMusic") {
        const { getYTMusicAudioStream } = require('./funcs/apis/ytaudiostream.js');
        const { stream: audioStream, contentType } = await getYTMusicAudioStream(url);
        const streamId = Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
        _audioStreams.set(streamId, { stream: audioStream, contentType: contentType || 'audio/webm' });
        const streamUrl = `http://127.0.0.1:${_audioStreamPort}/${streamId}`;
        event.sender.send("stream-ready", { streamUrl, platform: 'youtubeMusic' });
        return { streamUrl, platform: 'youtubeMusic' };
    }

    if (platform === "spotify" && librespotService.isLoggedIn()) {
        const { extractMediaInfo } = require('./funcs/apis/librespotService.js');
        const mediaInfo = extractMediaInfo(url);
        if (!mediaInfo) throw new Error("Could not extract Spotify track ID from URL");
        const { stream: audioStream, contentType, durationMs } = await librespotService.getTrackStream(mediaInfo.id, mediaInfo.type);
        const streamId = Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
        _audioStreams.set(streamId, { stream: audioStream, contentType: contentType || 'audio/mp4' });
        const streamUrl = `http://127.0.0.1:${_audioStreamPort}/${streamId}`;
        const durationSec = durationMs ? durationMs / 1000 : 0;
        event.sender.send("stream-ready", { streamUrl, platform: 'spotify', durationSec });
        return { streamUrl, platform: 'spotify' };
    }

    if (platform === "tidal") {
        if (url && /tidal\.com\/(browse\/)?video\//i.test(url)) {
            const videoId = url.match(/\/video\/(\d+)/)?.[1];
            if (!videoId) throw new Error("Could not extract Tidal video ID from URL");
            const { TidalClient } = tidalStreamService;
            const videoClient = new TidalClient();
            const { stream: videoStream, contentType: videoContentType, durationMs: videoDurationMs } = await videoClient.getVideoStream(videoId, settings);
            if (videoClient.accessToken !== settings.tidal_access_token) {
                settings.tidal_access_token = videoClient.accessToken;
                try { fs.writeFileSync(settingsFilePath, JSON.stringify(settings, null, 2)); } catch (e) { logger.error('settings', `Failed to save tidal video token: ${e.message || e}`); }
            }
            const streamId = Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
            _audioStreams.set(streamId, { stream: videoStream, contentType: videoContentType || 'video/mp4' });
            const streamUrl = `http://127.0.0.1:${_audioStreamPort}/${streamId}`;
            const durationSec = videoDurationMs ? videoDurationMs / 1000 : 0;
            event.sender.send("stream-ready", { streamUrl, platform: 'tidal', mediaType: 'video', durationSec });
            return { streamUrl, platform: 'tidal' };
        }
        const { TidalClient, extractTidalTrackId } = tidalStreamService;
        const trackId = extractTidalTrackId(url);
        if (!trackId) throw new Error("Could not extract Tidal track ID from URL");
        const tidalClient = new TidalClient();
        const { stream: audioStream, contentType, durationMs } = await tidalClient.getTrackStream(trackId, settings);
        if (tidalClient.accessToken !== settings.tidal_access_token) {
            settings.tidal_access_token = tidalClient.accessToken;
            settings.tidal_token_expiry = tidalClient.tokenExpiry;
            try { fs.writeFileSync(settingsFilePath, JSON.stringify(settings, null, 2)); } catch (e) { logger.error('settings', `Failed to save tidal playback token: ${e.message || e}`); }
        }
        const streamId = Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
        _audioStreams.set(streamId, { stream: audioStream, contentType: contentType || 'audio/mp4' });
        const streamUrl = `http://127.0.0.1:${_audioStreamPort}/${streamId}`;
        const durationSec = durationMs ? durationMs / 1000 : 0;
        event.sender.send("stream-ready", { streamUrl, platform: 'tidal', durationSec });
        return { streamUrl, platform: 'tidal' };
    }

    if (platform === "deezer") {
        const { stream: audioStream, contentType, durationMs } = await deezerStreamService.getTrackStream(url, settings);
        const streamId = Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
        _audioStreams.set(streamId, { stream: audioStream, contentType: contentType || 'audio/mpeg' });
        const streamUrl = `http://127.0.0.1:${_audioStreamPort}/${streamId}`;
        const durationSec = durationMs ? durationMs / 1000 : 0;
        event.sender.send("stream-ready", { streamUrl, platform: 'deezer', durationSec });
        return { streamUrl, platform: 'deezer' };
    }

    if (platform === "qobuz") {
        const { QobuzClient } = require('./funcs/streamrip/QobuzClient');
        const qobuzClient = new QobuzClient();
        const updatedCreds = await qobuzClient.login(settings);
        Object.assign(settings, updatedCreds);
        try { fs.writeFileSync(settingsFilePath, JSON.stringify(settings, null, 2)); } catch (e) { logger.error('settings', `Failed to save qobuz credentials: ${e.message || e}`); }
        const qobuzTrackIdMatch = url.match(/play\.qobuz\.com\/track\/(\d+)/) ||
                                  url.match(/qobuz\.com\/[a-z-]+\/album\/[^/]+\/(\d+)/);
        if (!qobuzTrackIdMatch) throw new Error("Could not extract Qobuz track ID from URL");
        const trackId = qobuzTrackIdMatch[1];
        let streamUrl = null;
        let lastApiError = null;
        const qualityOrder = [settings.qobuz_quality || 2, 2, 1];
        const triedQualities = new Set();
        for (const q of qualityOrder) {
            if (triedQualities.has(q)) continue;
            triedQualities.add(q);
            const fileUrlResp = await qobuzClient._requestFileUrl(trackId, q);
            if (fileUrlResp.status === 401) throw new Error("Qobuz: Authentication failed — re-enter credentials in Settings");
            if (fileUrlResp.status === 400) throw new Error("Qobuz: Bad request — credentials may be invalid");
            if (fileUrlResp.status >= 500) throw new Error(`Qobuz: Server error (HTTP ${fileUrlResp.status})`);
            let fileUrlJson;
            try { fileUrlJson = JSON.parse(fileUrlResp.text); } catch { throw new Error(`Qobuz: Unexpected response (HTTP ${fileUrlResp.status}): ${fileUrlResp.text.slice(0, 200)}`); }
            if (fileUrlJson.url) { streamUrl = fileUrlJson.url; break; }
            if (fileUrlJson.code || fileUrlJson.message) lastApiError = `Qobuz: ${fileUrlJson.message || fileUrlJson.code}`;
            const restriction = fileUrlJson.restrictions?.[0];
            if (restriction) lastApiError = `Qobuz restriction: ${restriction.message || restriction.code || JSON.stringify(restriction)}`;
        }
        if (!streamUrl) throw new Error(lastApiError || "Qobuz: No stream URL — track may not be in your region or subscription tier");
        event.sender.send("stream-ready", { streamUrl, platform: 'qobuz' });
        return { streamUrl, platform: 'qobuz' };
    }

    if (platform === "applemusic") {
        if (!appleMusicService.isConfigured(settings)) {
            throw new Error("Apple Music requires cookies. Go to Settings → Apple → set Cookies Path.");
        }
        const { stream: audioStream, contentType, durationMs } = await appleMusicService.getTrackStream(url, settings);
        const streamId = Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
        _audioStreams.set(streamId, { stream: audioStream, contentType: contentType || 'audio/aac' });
        const streamUrl = `http://127.0.0.1:${_audioStreamPort}/${streamId}`;
        const durationSec = durationMs ? durationMs / 1000 : 0;
        event.sender.send("stream-ready", { streamUrl, platform: 'applemusic', durationSec });
        return { streamUrl, platform: 'applemusic' };
    }

    if (platform === "spotify") {
        throw new Error("Spotify streaming requires login. Go to Settings → Spotify → Spotify Account to log in.");
    }

    if (url && url !== "null") {
        event.sender.send("stream-ready", { streamUrl: url, platform });
        return { streamUrl: url, platform };
    }
    throw new Error("No stream found for this URL");

    } catch (err) {
        const needsAuth = (platform === 'tidal' && err.message?.includes('access token not set')) ? 'tidal' : undefined;
        logger.error('playback', `[${platform}] ${err.message || err}`);
        event.sender.send("app-error", { message: err.message, context: 'playback', ...(needsAuth ? { needsAuth } : {}) });
        throw err;
    }
});
ipcMain.handle("pause-media", async () => {
    for (const [id, entry] of _audioStreams.entries()) {
        try { entry.stream?.destroy?.(); } catch {}
        _audioStreams.delete(id);
    }
});

ipcMain.handle("spotify-oauth-login", async () => {
    try {
        const cookiesPath = settings.spotify_cookies_path;
        if (!cookiesPath) throw new Error('Spotify cookies file not set. Go to Settings → Spotify → Authentication and set your cookies path.');
        const profile = await librespotService.loginFromCookies(cookiesPath);
        return profile;
    } catch (err) {
        throw err;
    }
});

ipcMain.handle("spotify-oauth-logout", async () => {
    librespotService.logout();
    return { success: true };
});

ipcMain.handle("spotify-oauth-status", async () => {
    return {
        loggedIn: librespotService.isLoggedIn(),
        profile: librespotService.getProfile(),
    };
});

ipcMain.handle("spotify-get-token", async () => {
    if (!librespotService.isLoggedIn()) return null;
    return await librespotService._getValidToken();
});

const TIDAL_CLIENT_ID = '6BDSRdpK9hqEBTgU';
const TIDAL_REDIRECT_URI = 'https://tidal.com/android/login/auth';

ipcMain.handle('tidal:start-auth', async () => {
    const codeVerifier = require('crypto').randomBytes(32).toString('base64url');
    const codeChallenge = require('crypto')
        .createHash('sha256').update(codeVerifier).digest('base64url');
    const params = new URLSearchParams({
        response_type: 'code',
        redirect_uri: TIDAL_REDIRECT_URI,
        client_id: TIDAL_CLIENT_ID,
        scope: 'r_usr+w_usr+w_sub',
        code_challenge_method: 'S256',
        code_challenge: codeChallenge,
        appMode: 'android',
        lang: 'en_US',
    });
    const authUrl = `https://login.tidal.com/authorize?${params}`;
    shell.openExternal(authUrl);
    return { codeVerifier, authUrl };
});

ipcMain.handle('tidal:exchange-code', async (_event, { redirectUrl, codeVerifier }) => {
    const url = new URL(redirectUrl);
    const code = url.searchParams.get('code');
    if (!code) throw new Error('No auth code found in redirect URL');

    const body = new URLSearchParams({
        code,
        client_id: TIDAL_CLIENT_ID,
        grant_type: 'authorization_code',
        redirect_uri: TIDAL_REDIRECT_URI,
        scope: 'r_usr+w_usr+w_sub',
        code_verifier: codeVerifier,
    }).toString();

    const resp = await new Promise((resolve, reject) => {
        const req = require('https').request({
            hostname: 'auth.tidal.com',
            path: '/v1/oauth2/token',
            method: 'POST',
            headers: {
                'Content-Type': 'application/x-www-form-urlencoded',
                'Content-Length': Buffer.byteLength(body),
            },
        }, (res) => {
            const chunks = [];
            res.on('data', c => chunks.push(c));
            res.on('end', () => resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString() }));
        });
        req.on('error', reject);
        req.write(body);
        req.end();
    });

    if (resp.status !== 200) throw new Error(`Tidal token exchange failed (${resp.status}): ${resp.body}`);
    const json = JSON.parse(resp.body);

    let userId = '', countryCode = 'US';
    try {
        const userResp = await new Promise((resolve, reject) => {
            const req = require('https').request({
                hostname: 'api.tidalhifi.com',
                path: '/v1/sessions',
                headers: { Authorization: `Bearer ${json.access_token}`, 'X-Tidal-Token': TIDAL_CLIENT_ID },
            }, (res) => {
                const chunks = [];
                res.on('data', c => chunks.push(c));
                res.on('end', () => resolve(JSON.parse(Buffer.concat(chunks).toString())));
            });
            req.on('error', reject);
            req.end();
        });
        userId = String(userResp.userId || '');
        countryCode = userResp.countryCode || 'US';
    } catch (e) {
        logger.warn('settings', `Tidal session fetch failed: ${e.message || e}`);
    }

    const tokens = {
        tidal_access_token: json.access_token,
        tidal_refresh_token: json.refresh_token || '',
        tidal_token_expiry: String(Math.floor(Date.now() / 1000) + (json.expires_in || 86400)),
        tidal_user_id: userId,
        tidal_country_code: countryCode,
    };

    Object.assign(settings, tokens);

    try {
        const current = JSON.parse(fs.readFileSync(settingsFilePath, 'utf8'));
        Object.assign(current, tokens);
        fs.writeFileSync(settingsFilePath, JSON.stringify(current, null, 2));
    } catch (e) {
        logger.warn('settings', `Failed to merge tidal tokens into settings file: ${e.message || e}`);
        try { fs.writeFileSync(settingsFilePath, JSON.stringify({ ...settings }, null, 2)); } catch (e2) { logger.error('settings', `Failed to write settings file: ${e2.message || e2}`); }
    }

    return tokens;
});
async function handleGetAlbumDetails(platform, albumId) {
    try {
        if (platform === 'youtubemusic') platform = 'youtubeMusic';
        let result;
        switch (platform) {
            case "spotify":
                const spotifyApi = new SpotifyAPI(getSpotifyCredentials());
                if (albumId.startsWith('audiobook::')) {
                    const abId = albumId.slice(11);
                    result = await spotifyApi.getAudiobookChapters(abId);
                } else {
                    result = await spotifyApi.getAlbumTracks(albumId);
                }
                break;
            case "tidal": {
                const auth = await getTidalUserApiToken();
                if (auth) {
                    const headers = { Authorization: `Bearer ${auth.token}` };
                    const BASE_V1 = 'https://api.tidalhifi.com/v1';
                    const cc = auth.countryCode;
                    const [albumResp, tracksResp] = await Promise.all([
                        fetch(`${BASE_V1}/albums/${albumId}?countryCode=${cc}`, { headers }).then(r => r.json()),
                        fetch(`${BASE_V1}/albums/${albumId}/tracks?countryCode=${cc}&limit=100`, { headers }).then(r => r.json()),
                    ]);
                    result = { _v1: true, album: albumResp, tracks: tracksResp.items || [] };
                } else {
                    const tidalApi = new TidalAPI(getTidalCredentials());
                    result = await tidalApi.getAlbum(albumId, 'US');
                }
                break;
            }
            case "deezer":
                const { getTrackList: getDeezerTrackList } = require('./funcs/apis/deezerapi.js');
                result = await getDeezerTrackList(`album/${albumId}`);
                break;
            case "qobuz":
                const { getTrackList: getQobuzTrackList } = require('./funcs/apis/qobuzapi.js');
                result = await getQobuzTrackList(albumId, 'album');
                break;
            case "youtubeMusic":
                const { getTrackList: getYTMusicTrackList } = require('./funcs/apis/ytmusicsearchapi.js');
                result = await getYTMusicTrackList(albumId, 'album');
                break;
            case "applemusic": {
                const { getAlbumTracks: getAppleMusicAlbumTracks } = require('./funcs/apis/applemusicapi.js');
                result = await getAppleMusicAlbumTracks(albumId);
                break;
            }
            default:
                throw new Error(`Unsupported platform: ${platform}`);
        }
        const formattedResult = formatPlatformResponseAlbum(platform, result);
        if (platform === 'youtubeMusic' || platform === 'youtube') {
            const urls = (formattedResult.tracks || [])
                .map(t => t.playUrl)
                .filter(Boolean);
            if (urls.length) ytPreResolve(urls);
        }
        return formattedResult;
    } catch (error) {
        throw error;
    }
}

function formatPlatformResponseAlbum(platform, result) {
    const formatters = {
        qobuz: data => {
            const trackItems = data.tracks?.items || [];
            const albumInfo = {
                title: data.title || "Unknown Album",
                artist: data.artist?.name || trackItems[0]?.performer?.name || "Unknown Artist",
                releaseDate: data.release_date_original || data.release_date_download || "Unknown Date",
                coverUrl: data.image?.large || data.image?.small || "",
                description: "",
                duration: data.duration || trackItems.reduce((sum, track) => sum + (track.duration || 0), 0),
                genre: data.genre?.name || ""
            };
            const tracks = trackItems.map((track, index) => ({
                id: track.id,
                number: track.track_number || index + 1,
                title: track.title,
                duration: track.duration,
                quality: `${track.maximum_bit_depth||"16"}bit / ${track.maximum_sampling_rate||"44.1"}kHz`,
                playUrl: track.id ? `https://play.qobuz.com/track/${track.id}` : null,
                artist: track.performer?.name || albumInfo.artist
            }));
            return {
                album: albumInfo,
                tracks: tracks
            }
        },
        tidal: data => {
            if (data._v1) {
                const album = data.album;
                const coverUrl = album.cover
                    ? `https://resources.tidal.com/images/${album.cover.replace(/-/g, '/')}/640x640.jpg`
                    : '';
                const albumInfo = {
                    title: album.title || 'Unknown Album',
                    artist: album.artists?.[0]?.name || album.artist?.name || 'Unknown Artist',
                    releaseDate: album.releaseDate || 'Unknown Date',
                    coverUrl,
                    description: '',
                    duration: album.duration || 0,
                    genre: album.genre || '',
                };
                const tracks = data.tracks.map((track, index) => ({
                    id: String(track.id),
                    number: track.trackNumber || index + 1,
                    title: track.title,
                    duration: track.duration,
                    quality: track.audioQuality || '',
                    playUrl: `https://tidal.com/browse/track/${track.id}`,
                    artist: track.artists?.[0]?.name || track.artist?.name || albumInfo.artist,
                }));
                return { album: albumInfo, tracks };
            }
            const albumData = data.data.attributes;
            const trackList = data.included;
            const trackOrderMap = new Map(data.data.relationships.items.data.map((item, index) => [item.id, index + 1]));
            const albumInfo = {
                title: albumData.title || "Unknown Album",
                artist: "Unknown Artist",
                releaseDate: albumData.releaseDate || "Unknown Date",
                coverUrl: albumData.imageLinks?.[0]?.href || "",
                description: "",
                duration: parseDuration(albumData.duration),
                genre: ""
            };
            const tracks = trackList.filter(track => track.type === "tracks").sort((a, b) => {
                const aOrder = trackOrderMap.get(a.id) || 0;
                const bOrder = trackOrderMap.get(b.id) || 0;
                return aOrder - bOrder
            }).map(track => ({
                id: track.id,
                number: trackOrderMap.get(track.id) || 0,
                title: track.attributes.title,
                duration: parseDuration(track.attributes.duration),
                quality: track.attributes.mediaTags.includes("LOSSLESS") ? "16bit / 44.1kHz" : "AAC",
                playUrl: track.attributes.externalLinks?.[0]?.href || null,
                artist: "Unknown Artist"
            }));
            return {
                album: albumInfo,
                tracks: tracks
            }
        },
        deezer: data => {
            const albumInfo = {
                title: data.name || "Unknown Album",
                artist: data.artist || "Unknown Artist",
                releaseDate: data.release_date || "Unknown Date",
                coverUrl: data.cover_xl || (data.md5_image ? `https://e-cdns-images.dzcdn.net/images/cover/${data.md5_image}/1000x1000.jpg` : ""),
                description: "",
                duration: data.tracks.reduce((sum, track) => sum + (track.duration || 0), 0),
                genre: ""
            };
            const tracks = data.tracks.map(track => ({
                id: track.id,
                number: track.track_position || 0,
                title: track.title,
                duration: track.duration,
                quality: "",
                playUrl: track.link || (track.id ? `https://www.deezer.com/track/${track.id}` : null),
                artist: track.artist?.name || albumInfo.artist
            }));
            return {
                album: albumInfo,
                tracks: tracks
            }
        },
        youtubeMusic: data => {
            const albumInfo = {
                title: data.album.title || "Unknown Album",
                artist: data.album.artist || "Unknown Artist",
                releaseDate: data.album.releaseDate || "Unknown Date",
                coverUrl: data.album.coverUrl || "",
                description: data.album.description || "",
                duration: data.album.duration || 0,
                genre: data.album.genre || ""
            };
            const tracks = data.tracks.map(track => ({
                id: track.id || "",
                number: track.number || 0,
                title: track.title || "Unknown Title",
                duration: track.duration || 0,
                quality: track.quality || "256Kbps",
                playUrl: track.playUrl || null,
                artist: albumInfo.artist
            }));
            return {
                album: albumInfo,
                tracks: tracks
            }
        },
        spotify: data => {
            if (data.chapters) {
                const albumInfo = {
                    title: data.book_name || 'Unknown Audiobook',
                    artist: data.author || 'Unknown Author',
                    releaseDate: '',
                    coverUrl: data.cover_url || '',
                    description: '',
                    duration: '',
                    genre: '',
                };
                const tracks = (data.chapters || []).map((ch, index) => ({
                    id: ch.id,
                    number: ch.chapter_number || index + 1,
                    title: ch.name,
                    duration: Math.floor((ch.duration_ms || 0) / 1e3),
                    quality: '',
                    playUrl: ch.external_urls?.spotify || ch.uri || null,
                    artist: data.author || '',
                }));
                return { album: albumInfo, tracks };
            }
            const albumInfo = {
                title: data.album_name || "Unknown Album",
                artist: data.artist_name || "Unknown Artist",
                releaseDate: data.release_date || "Unknown Date",
                coverUrl: data.cover_url || "",
                description: "",
                duration: "",
                genre: ""
            };
            const tracks = data.tracks.map(track => ({
                id: track.id,
                number: track.track_number || 0,
                title: track.name,
                duration: Math.floor(track.duration_ms / 1e3),
                quality: "",
                playUrl: track.external_urls?.spotify || track.uri || null,
                artist: track.artists?.[0]?.name || albumInfo.artist
            }));
            return {
                album: albumInfo,
                tracks: tracks
            }
        },
        applemusic: data => {
            const a = data.album || {};
            const albumInfo = {
                title: a.collectionName || "Unknown Album",
                artist: a.artistName || "Unknown Artist",
                releaseDate: a.releaseDate ? a.releaseDate.slice(0, 4) : "Unknown Date",
                coverUrl: a.artworkUrl100?.replace('100x100', '640x640') || "",
                description: "",
                duration: 0,
                genre: a.primaryGenreName || ""
            };
            const tracks = (data.tracks || []).map((track, index) => ({
                id: String(track.trackId),
                number: track.trackNumber || index + 1,
                title: track.trackName || "Unknown Track",
                duration: track.trackTimeMillis ? Math.floor(track.trackTimeMillis / 1000) : 0,
                quality: "",
                playUrl: track.trackViewUrl || null,
                artist: track.artistName || albumInfo.artist
            }));
            return { album: albumInfo, tracks };
        }
    };
    const formatter = formatters[platform];
    if (!formatter) {
        throw new Error(`No formatter available for platform: ${platform}`)
    }
    return formatter(result)
}
async function handleGetPlaylistDetails(platform, playlistId) {
    try {
        if (platform === 'youtubemusic') platform = 'youtubeMusic';
        let result;
        const isPodcast = platform === 'youtubeMusic' && playlistId.startsWith('MPSP');
        switch (platform) {
            case "spotify":
                const spotifyApi = new SpotifyAPI(getSpotifyCredentials());
                if (playlistId.startsWith('show::')) {
                    const showId = playlistId.slice(6);
                    result = await spotifyApi.getShowEpisodes(showId);
                } else {
                    result = await spotifyApi.getPlaylistTracks(playlistId);
                }
                break;
            case "tidal": {
                const auth = await getTidalUserApiToken();
                if (!auth) throw new Error('Tidal sign-in required for playlists. Go to Settings → Tidal to authenticate.');
                const headers = { Authorization: `Bearer ${auth.token}` };
                const BASE_V1 = 'https://api.tidalhifi.com/v1';
                const cc = auth.countryCode;
                const [playlistResp, tracksResp] = await Promise.all([
                    fetch(`${BASE_V1}/playlists/${playlistId}?countryCode=${cc}`, { headers }).then(r => r.json()),
                    fetch(`${BASE_V1}/playlists/${playlistId}/tracks?countryCode=${cc}&limit=200`, { headers }).then(r => r.json()),
                ]);
                result = { _v1: true, playlist: playlistResp, tracks: tracksResp.items || [] };
                break;
            }
            case "deezer": {
                const { getTrackList: getDeezerTrackList } = require('./funcs/apis/deezerapi.js');
                if (playlistId.startsWith('podcast::')) {
                    const podcastId = playlistId.slice(9);
                    const resp = await fetch(`https://api.deezer.com/podcast/${podcastId}/episodes?limit=100`);
                    const data = await resp.json();
                    const episodes = Array.isArray(data) ? data : (data.data ?? []);
                    result = {
                        playlist: { title: 'Podcast Episodes', coverUrl: '' },
                        tracks: episodes.map(ep => ({
                            id: String(ep.id),
                            title: ep.title,
                            artist: '',
                            duration: ep.duration,
                            link: ep.link || `https://www.deezer.com/episode/${ep.id}`,
                        }))
                    };
                } else {
                    result = await getDeezerTrackList(`playlist/${playlistId}`);
                }
                break;
            }
            case "qobuz":
                const { getTrackList: getQobuzTrackList } = require('./funcs/apis/qobuzapi.js');
                result = await getQobuzTrackList(playlistId, 'playlist');
                break;
            case "youtubeMusic": {
                const { getTrackList: getYTMusicTrackList } = require('./funcs/apis/ytmusicsearchapi.js');
                let ytPlaylistId = playlistId;
                let isYTPodcast = false;
                if (ytPlaylistId.startsWith('podcast::')) {
                    ytPlaylistId = ytPlaylistId.slice(9);
                    isYTPodcast = true;
                } else {
                    isYTPodcast = ytPlaylistId.startsWith('MPSP');
                }
                result = await getYTMusicTrackList(ytPlaylistId, isYTPodcast ? 'podcast' : 'playlist');
                break;
            }
            case "youtube":
                throw new Error("YouTube playlist not implemented in JS yet");
            default:
                throw new Error(`Unsupported platform: ${platform}`);
        }
        const formattedResult = formatPlatformResponsePlaylist(platform, result);
        if (platform === 'youtubeMusic' || platform === 'youtube') {
            const urls = (formattedResult.tracks || [])
                .map(t => t.playUrl)
                .filter(Boolean);
            if (urls.length) ytPreResolve(urls);
        }
        return formattedResult;
    } catch (error) {
        throw error;
    }
}

function formatPlatformResponsePlaylist(platform, result) {
    const formatters = {
        qobuz: data => {
            const trackItems = data.tracks?.items || [];
            const playlistInfo = {
                title: data.name || data.title || "Unknown Playlist",
                creator: data.owner?.name || "",
                creationDate: data.created_at || "Unknown Date",
                coverUrl: (typeof data.image === 'string' ? data.image : data.image?.large) || data.images?.[0] || "",
                description: data.description || "",
                duration: data.duration || trackItems.reduce((sum, track) => sum + (track.duration || 0), 0),
                totalTracks: data.tracks_count || trackItems.length
            };
            const tracks = trackItems.map((track, index) => ({
                id: track.id,
                number: track.position || index + 1,
                title: track.title,
                cover: track.album?.image?.small,
                albumTitle: track.album?.title,
                albumArtist: track.album?.artist?.name,
                explicit: track.parental_warning,
                duration: track.duration,
                quality: `${track.maximum_bit_depth||"16"}bit / ${track.maximum_sampling_rate||"44.1"}kHz`,
                playUrl: track.id ? `https://play.qobuz.com/track/${track.id}` : null,
                artist: track.performer?.name || "Unknown Artist"
            }));
            return {
                playlist: playlistInfo,
                tracks: tracks
            }
        },
        deezer: data => {
            const playlistInfo = {
                title: data.name || "Unknown Playlist",
                creator: data.artist || "Unknown Creator",
                creationDate: data.release_date || "Unknown Date",
                coverUrl: data.cover_xl || (data.md5_image ? `https://e-cdns-images.dzcdn.net/images/cover/${data.md5_image}/1000x1000.jpg` : ""),
                description: "",
                duration: data.tracks.reduce((sum, track) => sum + (track.duration || 0), 0),
                totalTracks: data.total_tracks || data.tracks.length
            };
            const tracks = data.tracks.map((track, index) => ({
                id: track.id,
                number: index + 1,
                title: track.title,
                cover: track.album?.cover_small,
                albumTitle: track.album?.title,
                explicit: track.explicit_lyrics,
                duration: track.duration,
                quality: "MP3 320kbps",
                playUrl: track.link || (track.id ? `https://www.deezer.com/track/${track.id}` : null),
                link: track.link || null,
                albumArtist: track.artist?.name || "Unknown Artist"
            }));
            return {
                playlist: playlistInfo,
                tracks: tracks
            }
        },
        youtubeMusic: data => {
            const playlistInfo = {
                title: data.album.title || "Unknown Album",
                artist: data.album.artist || "Unknown Artist",
                releaseDate: data.album.releaseDate || "Unknown Date",
                coverUrl: data.album.coverUrl || "",
                description: data.album.description || "",
                duration: data.album.duration || 0,
                genre: data.album.genre || ""
            };
            const tracks = data.tracks.map(track => ({
                id: track.id || "",
                number: track.number || 0,
                title: track.title || "Unknown Title",
                duration: track.duration || 0,
                quality: track.quality || "256Kbps",
                playUrl: track.playUrl || null,
                artist: playlistInfo.artist
            }));
            return {
                playlist: playlistInfo,
                tracks: tracks
            }
        },
        spotify: data => {
            if (data.episodes) {
                const playlistInfo = {
                    title: data.show_name || 'Unknown Show',
                    creator: data.publisher || 'Unknown',
                    releaseDate: '',
                    coverUrl: data.cover_url || '',
                    description: '',
                    duration: '',
                    genre: '',
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
                title: data.playlist_name || "Unknown Playlist",
                creator: data.owner_name || "Unknown",
                releaseDate: "",
                coverUrl: data.cover_url || "",
                description: "",
                duration: "",
                genre: ""
            };
            const tracks = data.tracks
                .filter(item => item?.track)
                .map((item, index) => {
                    const track = item.track;
                    return {
                        id: track.id,
                        number: index + 1,
                        title: track.name,
                        duration: Math.floor((track.duration_ms || 0) / 1e3),
                        quality: "",
                        playUrl: track.external_urls?.spotify || track.uri || null,
                        artist: track.artists?.[0]?.name || "Unknown Artist",
                        cover: track.album?.images?.[0]?.url || "",
                        albumTitle: track.album?.name || ""
                    };
                });
            return {
                playlist: playlistInfo,
                tracks: tracks
            }
        },
        youtube: data => {
            const playlistInfo = {
                title: data.Playlist.title,
                artist: data.Playlist.artist,
                releaseDate: data.Playlist.releaseDate,
                coverUrl: data.Playlist.coverUrl,
                description: data.Playlist.description,
                duration: data.Playlist.duration
            };
            const tracks = data.Tracks.map(track => ({
                id: track.id,
                number: track.number,
                title: track.title,
                cover: track.coverUrl,
                duration: track.duration
            }));
            return {
                playlist: playlistInfo,
                tracks: tracks
            }
        },
        tidal: data => {
            const pl = data.playlist;
            const imageId = pl.squareImage || pl.image;
            const coverUrl = imageId
                ? `https://resources.tidal.com/images/${imageId.replace(/-/g, '/')}/640x640.jpg`
                : '';
            const playlistInfo = {
                title: pl.title || 'Unknown Playlist',
                creator: pl.creator?.name || 'Unknown Creator',
                creationDate: '',
                coverUrl,
                description: pl.description || '',
                duration: pl.duration || 0,
                totalTracks: pl.numberOfTracks || data.tracks.length,
            };
            const tracks = data.tracks.map((track, index) => ({
                id: String(track.id),
                number: index + 1,
                title: track.title,
                duration: track.duration,
                quality: track.audioQuality || '',
                playUrl: `https://tidal.com/browse/track/${track.id}`,
                artist: track.artists?.[0]?.name || track.artist?.name || 'Unknown Artist',
            }));
            return { playlist: playlistInfo, tracks };
        }
    };
    const formatter = formatters[platform];
    if (!formatter) {
        throw new Error(`No formatter available for platform: ${platform}`)
    }
    return formatter(result)
}

function parseDuration(duration) {
    if (!duration) return 0;
    const matches = duration.match(/PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?/);
    if (!matches) return 0;
    const [_, hours, minutes, seconds] = matches;
    return parseInt(hours || 0) * 3600 + parseInt(minutes || 0) * 60 + parseInt(seconds || 0)
}
async function handleGetArtistDetails(platform, artistId) {
    if (platform === 'youtubemusic') platform = 'youtubeMusic';
    switch (platform) {
        case 'spotify': {
            const spotifyApi = new SpotifyAPI(getSpotifyCredentials());
            const data = await spotifyApi.getArtistAlbums(artistId);
            const items = data.items || [];
            return {
                albums: items.map(album => ({
                    id: album.id,
                    title: album.name,
                    thumbnail: album.images?.[0]?.url,
                    releaseDate: album.release_date,
                    trackCount: album.total_tracks,
                    url: album.external_urls?.spotify || album.uri,
                    explicit: album.explicit ?? false,
                }))
            };
        }
        case 'tidal': {
            const auth = await getTidalUserApiToken();
            if (auth) {
                const headers = { Authorization: `Bearer ${auth.token}` };
                const BASE_V1 = 'https://api.tidalhifi.com/v1';
                const cc = auth.countryCode;
                const resp = await fetch(`${BASE_V1}/artists/${artistId}/albums?countryCode=${cc}&limit=50`, { headers });
                const data = await resp.json();
                const items = data.items || [];
                return {
                    albums: items.map(album => ({
                        id: String(album.id),
                        title: album.title,
                        thumbnail: album.cover ? `https://resources.tidal.com/images/${album.cover.replace(/-/g, '/')}/640x640.jpg` : undefined,
                        releaseDate: album.releaseDate,
                        trackCount: album.numberOfTracks,
                        url: `https://tidal.com/browse/album/${album.id}`,
                        explicit: album.explicit ?? false,
                    }))
                };
            } else {
                const tidalApi = new TidalAPI(getTidalCredentials());
                const data = await tidalApi.getArtistAlbums(artistId, 'US');
                const items = data.data || [];
                return {
                    albums: items.map(album => {
                        const attr = album.attributes || album;
                        const imgArr = attr.imageCover;
                        const thumbnail = Array.isArray(imgArr) ? (imgArr.find(i => i.width >= 640) ?? imgArr[imgArr.length - 1])?.href : undefined;
                        return {
                            id: album.id,
                            title: attr.title || album.title,
                            thumbnail,
                            releaseDate: attr.releaseDate || album.releaseDate,
                            trackCount: attr.numberOfItems || attr.numberOfTracks,
                            url: attr.url || `https://tidal.com/browse/album/${album.id}`,
                            explicit: attr.explicit ?? false,
                        };
                    })
                };
            }
        }
        case 'deezer': {
            const { getArtistAlbums: getDeezerArtistAlbums } = require('./funcs/apis/deezerapi.js');
            const items = await getDeezerArtistAlbums(artistId);
            return {
                albums: items.map(album => ({
                    id: String(album.id),
                    title: album.title,
                    thumbnail: album.cover_xl || album.cover_big,
                    releaseDate: album.release_date,
                    trackCount: album.nb_tracks,
                    url: album.link || `https://www.deezer.com/album/${album.id}`,
                    explicit: album.explicit_lyrics === 1,
                }))
            };
        }
        case 'qobuz': {
            const { getAlbumList: getQobuzArtistAlbums } = require('./funcs/apis/qobuzapi.js');
            const data = await getQobuzArtistAlbums(artistId);
            const items = data.items || [];
            return {
                albums: items.map(album => ({
                    id: String(album.id),
                    title: album.title,
                    thumbnail: album.image?.large,
                    releaseDate: album.released_at ? new Date(album.released_at * 1000).toISOString().slice(0, 10) : album.release_date_original,
                    trackCount: album.tracks_count,
                    url: `https://play.qobuz.com/album/${album.id}`,
                    explicit: album.parental_warning ?? false,
                    hires: album.hires_streamable ?? false,
                }))
            };
        }
        case 'youtubeMusic': {
            const { getArtistAlbums: getYTMArtistAlbums } = require('./funcs/apis/ytmusicsearchapi.js');
            const items = await getYTMArtistAlbums(artistId);
            return {
                albums: items.map(album => {
                    const thumb = Array.isArray(album.thumbnails) ? album.thumbnails[album.thumbnails.length - 1]?.url : album.thumbnail;
                    return {
                        id: album.albumId || album.browseId,
                        title: album.name || album.title,
                        thumbnail: thumb,
                        releaseDate: album.year ? String(album.year) : undefined,
                        url: album.browseId ? `https://music.youtube.com/browse/${album.browseId}` : undefined,
                    };
                }).filter(a => a.id)
            };
        }
        case 'youtube': {
            const { YouTubeSearch } = require('./funcs/apis/ytsearchapi.js');
            const ytSearch = new YouTubeSearch(getYouTubeApiKey());
            const playlists = await ytSearch.getChannelPlaylists(artistId);
            return {
                albums: playlists.map(p => ({
                    id: p.id,
                    title: p.title,
                    thumbnail: p.thumbnail,
                    trackCount: p.trackCount,
                    url: p.url,
                }))
            };
        }
        case 'applemusic': {
            const { getArtistAlbums: getAppleMusicArtistAlbums } = require('./funcs/apis/applemusicapi.js');
            const items = await getAppleMusicArtistAlbums(artistId);
            return {
                albums: items.map(album => ({
                    id: String(album.collectionId),
                    title: album.collectionName,
                    thumbnail: album.artworkUrl100?.replace('100x100', '640x640'),
                    releaseDate: album.releaseDate,
                    trackCount: album.trackCount,
                    url: album.collectionViewUrl,
                    explicit: album.collectionExplicitness === 'explicit',
                }))
            };
        }
        default:
            throw new Error(`Unsupported platform for artist details: ${platform}`);
    }
}
ipcMain.handle("get-artist-details", async (event, platform, artistId) => {
    try {
        const result = await handleGetArtistDetails(platform, artistId);
        return { success: true, data: result };
    } catch (error) {
        return { success: false, error: error.message };
    }
});
ipcMain.handle("get-album-details", async (event, platform, albumId) => {
    try {
        const result = await handleGetAlbumDetails(platform, albumId);
        return {
            success: true,
            data: result
        }
    } catch (error) {
        return {
            success: false,
            error: error.message
        }
    }
});
ipcMain.handle("get-playlist-details", async (event, platform, playlistId) => {
    try {
        const result = await handleGetPlaylistDetails(platform, playlistId);
        return {
            success: true,
            data: result
        }
    } catch (error) {
        return {
            success: false,
            error: error.message
        }
    }
});
ipcMain.handle("showItemInFolder",async(event,filePath)=>{
    try{
        const normalizedPath=path.normalize(filePath);
        if(fs.existsSync(normalizedPath)){
            if(fs.statSync(normalizedPath).isDirectory()){
                await shell.openPath(normalizedPath);
            }else{
                shell.showItemInFolder(normalizedPath);
            }
            return true;
        }else{
            throw new Error("File or folder not found");
        }
    }catch(error){
        throw error;
    }
});

ipcMain.handle('updates:get-version', () => app.getVersion());

ipcMain.handle('updates:check', async () => {
    try {
        const checker = new UpdateChecker('MediaHarbor', 'mediaharbor', app.getVersion());
        const latestRelease = await checker.getLatestRelease();
        const currentVersion = app.getVersion();
        if (!latestRelease) {
            return { hasUpdate: false, currentVersion, latestVersion: currentVersion, releaseNotes: '', releaseUrl: '', publishedAt: '' };
        }
        const latestVersion = latestRelease.tag_name.replace('v', '');
        const hasUpdate = checker.compareVersions(latestVersion, currentVersion.replace('v', '')) > 0;
        return {
            hasUpdate,
            currentVersion,
            latestVersion: latestRelease.tag_name,
            releaseNotes: latestRelease.body || '',
            releaseUrl: latestRelease.html_url,
            publishedAt: latestRelease.published_at,
        };
    } catch (error) {
        throw error;
    }
});

ipcMain.handle('updates:open-release', async (_event, url) => {
    await shell.openExternal(url);
});

ipcMain.handle('updates:check-deps', async () => {
    const status = { python: false, git: false, ffmpeg: false, ytdlp: false, ytmusic: true, qobuz: false, deezer: false, tidal: false, apple: false, spotify: false, googleapi: true, pyapplemusicapi: true };
    status.python  = await checkPythonVersion();
    status.git     = await checkGit();
    status.ffmpeg  = await checkFFmpeg();
    if (status.python) {
        try {
            const venvPython = getVenvPython();
            const { stdout } = await execPromise(`"${venvPython}" -m pip list`);
            if (typeof stdout === 'string') {
                status.ytdlp          = stdout.includes('yt-dlp');
                status.qobuz          = true;
                status.deezer         = true;
                status.tidal          = true;
                status.apple          = stdout.includes('gamdl');
                status.spotify        = stdout.includes('votify');
            }
        } catch (e) {
            logger.warn('system', `Failed to check pip packages: ${e.message || e}`);
        }
    }
    return status;
});

ipcMain.handle('updates:get-dependency-versions', async (_event, packages) => {
    const result = {};
    try {
        const venvPython = getVenvPython();
        for (const pkg of packages) {
            try {
                const { stdout } = await execPromise(`"${venvPython}" -m pip show ${pkg}`);
                const m = stdout.match(/^Version:\s+(.+)$/m);
                if (m) result[pkg.toLowerCase()] = m[1].trim();
            } catch (e) {
                    logger.info('system', `Package ${pkg} not installed or not found`);
                }
        }
    } catch (e) {
        logger.warn('system', `Failed to get dependency versions: ${e.message || e}`);
    }
    return result;
});

ipcMain.handle('updates:get-binary-versions', async () => {
    const https    = require('https');
    const platform = require('os').platform();
    const result   = { python: '', git: '', ffmpeg: '' };

    function httpsGet(url, hops = 0) {
        return new Promise((resolve, reject) => {
            if (hops > 5) return reject(new Error('too many redirects'));
            const req = https.get(url, { headers: { 'User-Agent': 'MediaHarbor/1.0' } }, (res) => {
                if ([301, 302, 307, 308].includes(res.statusCode))
                    return httpsGet(res.headers.location, hops + 1).then(resolve).catch(reject);
                let raw = '';
                res.on('data', c => raw += c);
                res.on('end', () => resolve(raw));
            });
            req.on('error', reject);
            req.setTimeout(8000, () => { req.destroy(); reject(new Error('timeout')); });
        });
    }
    const getJson = async (url) => JSON.parse(await httpsGet(url));

    try {
        const data   = await getJson('https://endoflife.date/api/python.json');
        result.python = data[0]?.latest ?? '';
    } catch (e) {
        logger.warn('system', `Failed to fetch latest Python version: ${e.message || e}`);
    }

    try {
        if (platform === 'win32') {
            const data = await getJson('https://api.github.com/repos/git-for-windows/git/releases/latest');
            const m    = (data.tag_name ?? '').match(/v(\d+\.\d+\.\d+)/);
            if (m) result.git = m[1];
        } else {
            const tags   = await getJson('https://api.github.com/repos/git/git/tags?per_page=20');
            const stable = tags.find(t => /^v\d+\.\d+\.\d+$/.test(t.name));
            if (stable) {
                const m = stable.name.match(/v(\d+\.\d+\.\d+)/);
                if (m) result.git = m[1];
            }
        }
    } catch (e) {
        logger.warn('system', `Failed to fetch latest Git version: ${e.message || e}`);
    }

    try {
        if (platform === 'win32') {
            const text = await httpsGet('https://www.gyan.dev/ffmpeg/builds/release-version');
            const m    = text.trim().match(/^(\d+[\d.]+)/);
            if (m) result.ffmpeg = m[1];
        } else if (platform === 'darwin') {
            const data  = await getJson('https://evermeet.cx/ffmpeg/info/ffmpeg/release');
            result.ffmpeg = data.version ?? '';
        } else {
            const tags   = await getJson('https://api.github.com/repos/FFmpeg/FFmpeg/tags?per_page=20');
            const stable = tags.find(t => /^n\d+\.\d+(\.\d+)?$/.test(t.name));
            if (stable) {
                const m = stable.name.match(/^n([\d.]+)/);
                if (m) result.ffmpeg = m[1];
            }
        }
    } catch (e) {
        logger.warn('system', `Failed to fetch latest FFmpeg version: ${e.message || e}`);
    }

    return result;
});

ipcMain.handle('updates:install-dep', async (event, dep) => {
    const realWin = BrowserWindow.fromWebContents(event.sender);
    const senderWin = realWin || { webContents: { send: (ch, ...args) => { if (!event.sender.isDestroyed()) event.sender.send(ch, ...args); } } };
    try {
        logger.info('install', `Starting installation of dependency: ${dep}`);
        switch (dep) {
            case 'git':    await downloadAndInstallGit(senderWin);    break;
            case 'python': await ensureVenv(senderWin); break;
            case 'ffmpeg': await downloadAndInstallFFmpeg(senderWin); break;
            case 'ytdlp':
                await ensureVenv(senderWin);
                await installWithProgress(`"${getVenvPython()}" -m pip install --upgrade yt-dlp isodate`, senderWin, dep);
                break;
            case 'ytmusic':
                senderWin?.webContents?.send('install-progress', { dep, progress: 100, message: 'ytmusic is built-in (native Node.js)' });
                break;
            case 'qobuz': case 'deezer': case 'tidal':
                senderWin?.webContents?.send('install-progress', { dep, progress: 100, message: `${dep} is built-in (native Node.js)` });
                break;
            case 'apple':
                await ensureVenv(senderWin);
                await downloadAndInstallBento4(senderWin);
                await installWithProgress(`"${getVenvPython()}" -m pip install --upgrade gamdl`, senderWin, 'apple', { startPercent: 30, endPercent: 100, prefix: 'Installing gamdl' });
                break;
            case 'spotify':
                await ensureVenv(senderWin);
                await downloadAndInstallBento4(senderWin);
                await installWithProgress(`"${getVenvPython()}" -m pip install --upgrade votify`, senderWin, dep, { startPercent: 30, endPercent: 100 });
                break;
            case 'googleapi':
                senderWin?.webContents?.send('install-progress', { dep, progress: 100, message: 'googleapi is built-in (native Node.js)' });
                break;
            case 'pyapplemusicapi':
                senderWin?.webContents?.send('install-progress', { dep, progress: 100, message: 'pyapplemusicapi is built-in (native Node.js)' });
                break;
            default: throw new Error(`Unknown dependency: ${dep}`);
        }
        logger.info('install', `Successfully installed dependency: ${dep}`);
        return { success: true };
    } catch (error) {
        logger.error('install', `Failed to install dependency ${dep}: ${error.message || error}`);
        throw error;
    }
});

function createWindow() {
    const win = new BrowserWindow({
        width: 1200,
        height: 800,
        minHeight: 600,
        minWidth: 915,
        frame: true,
        autoHideMenuBar: true,
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            preload: path.join(__dirname, "preload.js")
        }
    });

    logger.init(app.getPath('userData'), win);
    logger.info('system', `MediaHarbor v${app.getVersion()} starting`);
    const gotTheLock = app.requestSingleInstanceLock();
    if (gotTheLock) {
        const argv = process.argv;
        if (process.platform === "win32" && argv.length >= 2) {
            handleProtocolUrl(argv[1])
        }
    }
    const isDev = true;

    if (isDev) {
        win.loadURL('http://localhost:5173').catch((e) => {
            logger.error('system', `Failed to load dev URL: ${e.message || e}`);
        });
    } else {
        win.loadFile(path.join(__dirname, '../dist-react/index.html'));
    }
    setupContextMenu(win);
    setupSettingsHandlers(ipcMain);

    fileWatcher.setMainWindow(win);
    try {
        const settingsRaw = fs.readFileSync(settingsFilePath, 'utf8');
        const s = JSON.parse(settingsRaw);
        if (s.downloadLocation) fileWatcher.start(s.downloadLocation);
    } catch (e) {
        logger.warn('system', `Failed to start file watcher: ${e.message || e}`);
    }

    registerDownloadHandlers({
        settingsFilePath,
        getDefaultSettings,
        handleYtDlpDownload,
        handleYtDlpMusicDownload,
        cancelDownload,
        GamRip,
        StreamRip,
        app,
        dialog,
        failedDownloadsDatabasePath,
        downloadsDatabasePath,
    });
}
async function checkForUpdates() {
    try {
        const updateChecker = new UpdateChecker("MediaHarbor", "mediaharbor", app.getVersion());
        await updateChecker.checkForUpdates()
    } catch (error) {
        logger.warn('system', `Update check failed: ${error.message || error}`);
    }
}
app.setAsDefaultProtocolClient("mediaharbor");
app.on("open-url", (event, url) => {
    event.preventDefault();
    handleProtocolUrl(url)
});
if (process.platform === "win32") {
    const gotTheLock = app.requestSingleInstanceLock();
    if (!gotTheLock) {
        app.quit()
    } else {
        app.on("second-instance", (event, argv) => {
            if (process.platform === "win32") {
                const protocolUrl = argv.find(arg => arg.startsWith("mediaharbor://"));
                if (protocolUrl) {
                    handleProtocolUrl(protocolUrl)
                }
            }
            const mainWindow = BrowserWindow.getAllWindows()[0];
            if (mainWindow) {
                if (mainWindow.isMinimized()) mainWindow.restore();
                mainWindow.focus()
            }
        })
    }
}
app.whenReady().then(async () => {
    protocol.handle('mhfile', (request) => {
        try {
            const urlObj = new URL(request.url);
            let decodedPath = decodeURIComponent(urlObj.pathname);

            if (/^\/[A-Za-z]:/.test(decodedPath)) {
                decodedPath = decodedPath.slice(1);
            }

            const normalizedPath = path.normalize(decodedPath);
            const { pathToFileURL } = require('url');
            const fileUrl = pathToFileURL(normalizedPath).href;

            return net.fetch(fileUrl, { headers: request.headers }).catch((fetchErr) => {
                logger.error('playback', `File not accessible: ${normalizedPath} — ${fetchErr.message}`);
                return new Response(fetchErr.message, { status: 404 });
            });
        } catch (err) {
            logger.error('playback', `mhfile protocol error: ${request.url} — ${err.message}`);
            return new Response(err.message, { status: 404 });
        }
    });

    await checkForUpdates();
    createWindow();

    if (settings.spotify_cookies_path) {
        librespotService.loginFromCookies(settings.spotify_cookies_path).catch((e) => {
            logger.warn('system', `Spotify cookie login failed at startup: ${e.message || e}`);
        });
    }
});
app.on("window-all-closed", () => {
    fileWatcher.stop();
    _audioStreamServer.close();
    if (process.platform !== "darwin") {
        app.quit()
    }
});
app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) {
        createWindow()
    }
});


async function getPythonVersionOutput(command) {
    try {
        const {
            stdout: stdout,
            stderr: stderr
        } = await execPromise(`${command} --version`);
        const output = stdout.trim() || stderr.trim();
        const versionMatch = output.match(/Python (\d+\.\d+\.\d+)/);
        return versionMatch ? versionMatch[1] : null
    } catch (error) {
        return null
    }
}
async function checkPythonVersion() {
    if (isVenvReady()) {
        const version = await getPythonVersionOutput(`"${getVenvPython()}"`);
        if (version) {
            const [major, minor] = version.split(".").map(Number);
            if (major === 3 && minor >= 10) return true;
        }
    }
    const commandsToTry = ["python", "python3", "py"];
    for (const command of commandsToTry) {
        const version = await getPythonVersionOutput(command);
        if (version) {
            const [major, minor] = version.split(".").map(Number);
            if (major === 3 && minor >= 10) {
                return true
            }
        }
    }
    return false
}
async function checkGit() {
    try {
        await execPromise("git --version");
        return true
    } catch (error) {
        return false
    }
}

function checkFFmpeg() {
    return new Promise(resolve => {
        const process = spawn("ffmpeg", ["-version"]);
        process.on("close", code => {
            resolve(code === 0)
        });
        process.on("error", () => {
            resolve(false)
        })
    })
}


function installWithProgress(command, win, dep, options = {}) {
    const {
        startPercent: startPercent = 0,
        endPercent: endPercent = 100,
        prefix: prefix = `Installing ${dep}`
    } = options;
    logger.info('install', `Running install command: ${command}`);
    return new Promise((resolve, reject) => {
        const executeCommand = cmd => {
            const process = spawn(cmd, {
                shell: true
            });
            let hasErrors = false;
            let lastOutput = "";
            const sendProgress = (progress, status) => {
                const scaledProgress = Math.floor(startPercent + progress / 100 * (endPercent - startPercent));
                win.webContents.send("installation-progress", JSON.stringify({
                    dependency: dep,
                    percent: scaledProgress,
                    status: status || `${prefix}: ${progress}%`
                }))
            };
            process.stdout.on("data", data => {
                const output = data.toString().trim();
                lastOutput = output;
                if (output.includes("Cloning into")) {
                    sendProgress(10, `${prefix}: Cloning repository...`)
                } else if (output.includes("Receiving objects:")) {
                    const match = output.match(/Receiving objects:\s+(\d+)%/);
                    if (match) {
                        const progress = parseInt(match[1]);
                        sendProgress(10 + progress * .3, `${prefix}: Cloning repository ${progress}%`)
                    }
                } else if (output.includes("Building wheels")) {
                    sendProgress(50, `${prefix}: Building package...`)
                } else if (output.includes("Successfully built")) {
                    sendProgress(75, `${prefix}: Package built successfully`)
                } else {
                    const downloadMatch = output.match(/(?<=\()\d{1,3}(?=%\))/);
                    if (downloadMatch) {
                        const progress = parseInt(downloadMatch[0]);
                        sendProgress(progress)
                    }
                }
                if (output.includes("Successfully installed")) {
                    sendProgress(100, `${prefix}: Installation completed`)
                }
            });
            process.stderr.on("data", data => {
                const errorOutput = data.toString().trim();
                if (errorOutput.includes("is installed in") && errorOutput.includes("which is not on PATH")) {
                    return
                }
                const isWarning = /warn|warning|notice|deprecated/i.test(errorOutput) && !/error|fail|exception|fatal/i.test(errorOutput);
                if (!isWarning) {
                    hasErrors = true
                }
                win.webContents.send("installation-message", {
                    dependency: dep,
                    message: errorOutput,
                    type: isWarning ? "warning" : "error"
                })
            });
            process.on("close", code => {
                if (code === 0 || !hasErrors && code !== null) {
                    sendProgress(100, `${prefix}: Installation completed successfully`);
                    logger.info('install', `${dep} installation completed successfully`);
                    resolve()
                } else {
                    const errorMessage = `${dep} installation failed with exit code ${code}. Last output: ${lastOutput}`;
                    logger.error('install', errorMessage);
                    win.webContents.send("installation-message", {
                        dependency: dep,
                        message: errorMessage,
                        type: "error"
                    });
                    reject(new Error(errorMessage))
                }
            });
            process.on("error", error => {
                const errorMessage = `Failed to start ${dep} installation: ${error.message}`;
                logger.error('install', errorMessage);
                win.webContents.send("installation-message", {
                    dependency: dep,
                    message: errorMessage,
                    type: "error"
                });
                reject(new Error(errorMessage))
            })
        };
        executeCommand(command)
    })
}

function handleProtocolUrl(url) {
    try {
        const urlObj = new URL(url);
        if (urlObj.protocol !== "mediaharbor:") return;
        const action = urlObj.pathname.slice(1);
        const params = urlObj.searchParams;
        const mediaUrl = params.get("url");
        let platform = params.get("platform");
        let title = params.get("title") || "Unknown Title";
        if (!mediaUrl) {
            throw new Error("No media URL provided.")
        }
        try {
            new URL(mediaUrl)
        } catch (e) {
            throw new Error("The media URL is not a valid URL.")
        }
        if (!platform) {
            platform = inferPlatformFromUrl(mediaUrl);
            if (!platform) {
                throw new Error("Unable to determine platform from the URL.")
            }
        }
        const mainWindow = BrowserWindow.getAllWindows()[0];
        if (!mainWindow) return;
        mainWindow.webContents.send("protocol-action", {
            url: mediaUrl,
            platform: platform,
            title: title
        })
    } catch (error) {
        dialog.showErrorBox("Protocol Error", `Failed to handle protocol URL: ${error.message}`)
    }
}

function inferPlatformFromUrl(url) {
    const parsedUrl = new URL(url);
    const hostname = parsedUrl.hostname.toLowerCase();
    if (hostname.includes("youtube.com") || hostname.includes("youtu.be")) {
        return "youtube"
    } else if (hostname.includes("music.youtube.com")) {
        return "youtubeMusic"
    } else if (hostname.includes("spotify.com")) {
        return "spotify"
    } else if (hostname.includes("tidal.com")) {
        return "tidal"
    } else if (hostname.includes("deezer.com")) {
        return "deezer"
    } else if (hostname.includes("qobuz.com")) {
        return "qobuz"
    } else if (hostname.includes("apple.com") || hostname.includes("itunes.apple.com") || hostname.includes("music.apple.com") || hostname.includes("music.*.apple.com")) {
        return "applemusic"
    } else {
        return "generic"
    }
}

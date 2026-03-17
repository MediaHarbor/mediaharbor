"use strict";
var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function (resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator["throw"](value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
    return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (g && (g = 0, op[0] && (_ = 0)), _) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
};
Object.defineProperty(exports, "__esModule", { value: true });
var electron_1 = require("electron");
function makeOnListener(channel) {
    return function (callback) {
        var listener = function (_event, data) { return callback(data); };
        electron_1.ipcRenderer.on(channel, listener);
        return function () { return electron_1.ipcRenderer.removeListener(channel, listener); };
    };
}
function makeOnListenerJSON(channel) {
    return function (callback) {
        var listener = function (_event, raw) {
            try {
                callback(typeof raw === 'string' ? JSON.parse(raw) : raw);
            }
            catch (_a) { }
        };
        electron_1.ipcRenderer.on(channel, listener);
        return function () { return electron_1.ipcRenderer.removeListener(channel, listener); };
    };
}
var electronAPI = {
    updates: {
        getVersion: function () { return electron_1.ipcRenderer.invoke('updates:get-version'); },
        check: function () { return electron_1.ipcRenderer.invoke('updates:check'); },
        openRelease: function (url) { return electron_1.ipcRenderer.invoke('updates:open-release', url); },
        checkDeps: function () { return electron_1.ipcRenderer.invoke('updates:check-deps'); },
        getDependencyVersions: function (packages) { return electron_1.ipcRenderer.invoke('updates:get-dependency-versions', packages); },
        getBinaryVersions: function () { return electron_1.ipcRenderer.invoke('updates:get-binary-versions'); },
        installDep: function (dep) { return electron_1.ipcRenderer.invoke('updates:install-dep', dep); },
        onInstallProgress: makeOnListenerJSON('installation-progress'),
    },
    search: {
        perform: function (params) { return electron_1.ipcRenderer.invoke('perform-search', params); },
        getAlbumDetails: function (platform, albumId) { return electron_1.ipcRenderer.invoke('get-album-details', platform, albumId); },
        getPlaylistDetails: function (platform, playlistId) { return electron_1.ipcRenderer.invoke('get-playlist-details', platform, playlistId); },
        getArtistDetails: function (platform, artistId) { return electron_1.ipcRenderer.invoke('get-artist-details', platform, artistId); },
    },
    downloads: {
        startYouTubeMusic: function (data, playlist) { return electron_1.ipcRenderer.send('start-yt-music-download', data, playlist); },
        startYouTubeVideo: function (data) { return electron_1.ipcRenderer.send('start-yt-video-download', data); },
        startSpotify: function (command) { return electron_1.ipcRenderer.send('start-spotify-download', command); },
        startAppleMusic: function (command) { return electron_1.ipcRenderer.send('start-apple-download', command); },
        startQobuz: function (data) { return electron_1.ipcRenderer.send('start-qobuz-download', data); },
        startDeezer: function (data) { return electron_1.ipcRenderer.send('start-deezer-download', data); },
        startTidal: function (data) { return electron_1.ipcRenderer.send('start-tidal-download', data); },
        startGenericVideo: function (data) { return electron_1.ipcRenderer.send('start-generic-video-download', data); },
        onProgress: makeOnListener('download-update'),
        onInfo: makeOnListener('download-info'),
        onComplete: makeOnListener('download-complete'),
        onError: makeOnListener('download-error'),
        cancel: function (order) { return electron_1.ipcRenderer.send('cancel-download', order); },
        showItemInFolder: function (filePath) { return electron_1.ipcRenderer.invoke('showItemInFolder', filePath); },
    },
    settings: {
        get: function () { return electron_1.ipcRenderer.invoke('get-settings'); },
        set: function (settings) { return electron_1.ipcRenderer.invoke('set-settings', settings); },
        openFolder: function () { return __awaiter(void 0, void 0, void 0, function () {
            var result;
            return __generator(this, function (_a) {
                switch (_a.label) {
                    case 0: return [4 /*yield*/, electron_1.ipcRenderer.invoke('dialog:openFolder')];
                    case 1:
                        result = _a.sent();
                        return [2 /*return*/, result !== null && result !== void 0 ? result : null];
                }
            });
        }); },
        openFile: function () { return __awaiter(void 0, void 0, void 0, function () {
            var result;
            return __generator(this, function (_a) {
                switch (_a.label) {
                    case 0: return [4 /*yield*/, electron_1.ipcRenderer.invoke('dialog:openFile')];
                    case 1:
                        result = _a.sent();
                        return [2 /*return*/, result !== null && result !== void 0 ? result : null];
                }
            });
        }); },
    },
    library: {
        scan: function (directory, force) {
            if (force === void 0) { force = false; }
            return electron_1.ipcRenderer.invoke('scan-directory', directory, { force: force });
        },
        onScanProgress: makeOnListener('scan-progress'),
        onFilesChanged: makeOnListener('library:filesChanged'),
        showItemInFolder: function (filePath) { return electron_1.ipcRenderer.invoke('showItemInFolder', filePath); },
    },
    player: {
        playMedia: function (params) { return electron_1.ipcRenderer.invoke('play-media', params); },
        pause: function () { return electron_1.ipcRenderer.invoke('pause-media'); },
        onStreamReady: makeOnListener('stream-ready'),
    },
    spotifyAccount: {
        login: function () { return electron_1.ipcRenderer.invoke('spotify-oauth-login'); },
        logout: function () { return electron_1.ipcRenderer.invoke('spotify-oauth-logout'); },
        getStatus: function () { return electron_1.ipcRenderer.invoke('spotify-oauth-status'); },
        getToken: function () { return electron_1.ipcRenderer.invoke('spotify-get-token'); },
    },
    tidalAuth: {
        startAuth: function () { return electron_1.ipcRenderer.invoke('tidal:start-auth'); },
        exchangeCode: function (data) { return electron_1.ipcRenderer.invoke('tidal:exchange-code', data); },
    },
    app: {
        onError: makeOnListener('app-error'),
        onBackendLog: makeOnListener('backend-log'),
    },
};
electron_1.contextBridge.exposeInMainWorld('electron', electronAPI);

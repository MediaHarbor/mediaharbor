const { contextBridge, ipcRenderer } = require('electron');

// Combine all electronAPI methods into a single object
contextBridge.exposeInMainWorld('electronAPI', {
    // Download-related methods
    getDownloads: () => ipcRenderer.invoke('load-downloads'),

    // Existing channel handlers
    send: (channel, data) => {
        const validChannels = [
            'start-yt-music-download',
            'start-yt-video-download',
            'start-generic-video-download',
            'minimize-window',
            'maximize-window',
            'close-window',
            'start-streamrip',
            'start-download',
            'start-qobuz-download',
            'start-deezer-download',
            'start-tidal-download',
            'save-settings',
            'load-settings',
            'get-default-settings',
            'download-complete',
            'download-error',
            'start-qobuz-batch-download',
            'start-tidal-batch-download',
            'start-deezer-batch-download',
            'start-apple-download',
            'start-spotify-download',
            'clear-database',
            'start-apple-batch-download',
            'start-spotify-batch-download',
            'install-services',
            'spawn-tidal-config',
            'updateDep',
        ];
        if (validChannels.includes(channel)) {
            ipcRenderer.send(channel, data);
        }
    },

    receive: (channel, func) => {
        ipcRenderer.on(channel, (event, ...args) => func(...args));
    },
    showFirstStart: (callback) => ipcRenderer.on('show-first-start', callback),
    completeFirstRun: () => ipcRenderer.send('first-run-complete'),
    deleteDownload: (id) => ipcRenderer.invoke('deleteDownload', id),
    showItemInFolder: (location) => ipcRenderer.invoke('showItemInFolder', location),
    clearDownloadsDatabase: () => ipcRenderer.invoke('clearDownloadsDatabase'),
    fileLocation: () => ipcRenderer.invoke('dialog:saveFile'),
    folderLocation: () => ipcRenderer.invoke('dialog:openFolder'),
    fileSelectLocation: () => ipcRenderer.invoke('dialog:openFile'),
    openWvdLocation: () => ipcRenderer.invoke('dialog:openwvdFile'),

    checkDependencies: () => ipcRenderer.invoke('check-dependencies'),
    handleDependencyStatus: (callback) => {
        ipcRenderer.on('dependency-status', callback);
    },

    // Installation
    installDependency: (dep) => ipcRenderer.invoke('install-dependency', dep),

    // Setup completion
    completeSetup: () => ipcRenderer.invoke('complete-setup'),
    restartApp: () => ipcRenderer.send('restart-app'),

    // Progress updates
    onProgress: (callback) => {
        ipcRenderer.on('installation-progress', callback);
    },

    // Error handling
    onError: (callback) => {
        ipcRenderer.on('installation-error', callback);
    },

    // Settings
    saveSettings: (settings) => ipcRenderer.invoke('save-settings', settings),
    getSettings: () => ipcRenderer.invoke('get-settings'),
});
contextBridge.exposeInMainWorld(
    'api', {
        // First launch
        refreshApp: () => {return ipcRenderer.send('refresh-app')},
        getDefaultSettings: () => ipcRenderer.invoke('get-default-settings'),

        // Search methods
        performSearch: (searchData) => {
            return ipcRenderer.invoke('perform-search', searchData);
        },
        // Listen to events
        onSearchResults: (callback) => {
            ipcRenderer.on('search-results', (event, ...args) => callback(...args));
        },

        playMedia: (args) => {
            // Return the promise directly
            return ipcRenderer.invoke('play-media', args);
        },

        onStreamReady: (callback) => {
            ipcRenderer.removeAllListeners('stream-ready');
            ipcRenderer.on('stream-ready', (event, data) => callback(data));
        },

        onError: (callback) => {
            ipcRenderer.on('error', (event, ...args) => callback(...args));
        }
    }
);
contextBridge.exposeInMainWorld("electron", {
    ipcRenderer: {
        send: (channel, data) => ipcRenderer.send(channel, data),
        on: (channel, func) => ipcRenderer.on(channel, (event, ...args) => func(...args)),
    },

});
contextBridge.exposeInMainWorld('errorNotifier', {
    onError: (callback) => {
        ipcRenderer.on('out-error', (event, message) => {
            callback(message);
        });
    },
});
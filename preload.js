// preload.js
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
            'download-error'
        ];
        if (validChannels.includes(channel)) {
            ipcRenderer.send(channel, data);
        }
    },

    receive: (channel, func) => {
        ipcRenderer.on(channel, (event, ...args) => func(...args));
    },

    // URL processing
    processUrl: (url) => ipcRenderer.invoke('process-url', url),
    deleteDownload: (id) => ipcRenderer.invoke('deleteDownload', id),
    showItemInFolder: (location) => ipcRenderer.invoke('showItemInFolder', location),
    clearDownloadsDatabase: () => ipcRenderer.invoke('clearDownloadsDatabase')
});

// Theme detection
contextBridge.exposeInMainWorld('theme', {
    isDarkMode: () => {
        return window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
    }
});
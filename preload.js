const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electronAPI', {
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
            'start-tidal-download'
        ];
        if (validChannels.includes(channel)) {
            ipcRenderer.send(channel, data);
        }
    },
    receive: (channel, func) => {
        ipcRenderer.on(channel, (event, ...args) => func(...args));
    }
});

contextBridge.exposeInMainWorld('theme', {
    isDarkMode: () => {
        return window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
    }
});
contextBridge.exposeInMainWorld('electronAPI', {
    processUrl: (url) => ipcRenderer.invoke('process-url', url)
});
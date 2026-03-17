'use strict';

const { ipcMain } = require('electron');
const fs = require('fs');

function registerDownloadHandlers({
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
}) {
    ipcMain.on('clear-database', (event, { failedDownloads, downloads }) => {
        if (failedDownloads) {
            fs.unlink(failedDownloadsDatabasePath, err => {
                if (err) dialog.showErrorBox('Error', `Failed to delete Failed Downloads Database: ${err.message}`);
            });
        }
        if (downloads) {
            fs.unlink(downloadsDatabasePath, err => {
                if (err) dialog.showErrorBox('Error', `Failed to delete Downloads Database: ${err.message}`);
            });
        }
        event.sender.send('database-clear-status', 'Selected databases have been deleted.');
    });

    ipcMain.on('start-yt-music-download', (event, data, playlist) => {
        fs.readFile(settingsFilePath, 'utf8', (err, settingsData) => {
            const settings = err ? getDefaultSettings() : JSON.parse(settingsData);
            handleYtDlpMusicDownload(event, { ...data, platform: 'youtubemusic' }, settings, playlist);
        });
    });

    ipcMain.on('start-yt-video-download', (event, data) => {
        fs.readFile(settingsFilePath, 'utf8', (err, settingsData) => {
            const settings = err ? getDefaultSettings() : JSON.parse(settingsData);
            handleYtDlpDownload(event, { ...data, platform: 'youtube' }, settings, false);
        });
    });

    ipcMain.on('start-generic-video-download', (event, data) => {
        fs.readFile(settingsFilePath, 'utf8', (err, settingsData) => {
            const settings = err ? getDefaultSettings() : JSON.parse(settingsData);
            handleYtDlpDownload(event, { ...data, platform: 'generic' }, settings, true);
        });
    });

    const gamRip = new GamRip(settingsFilePath, app);

    ipcMain.handle('clear-spotify-credentials', async () => {
        try {
            await gamRip.clearCredentials();
            return { success: true, message: 'Credentials cleared successfully' };
        } catch (error) {
            return { success: false, message: error.message };
        }
    });

    ipcMain.on('start-spotify-download', (event, command) => {
        gamRip.handleDownload(event, command, 'spotify');
    });

    ipcMain.on('start-apple-download', (event, command) => {
        gamRip.handleDownload(event, command, 'applemusic');
    });

    ipcMain.on('start-apple-batch-download', (event, command) => {
        gamRip.handleBatchDownload(event, command, 'applemusic');
    });

    ipcMain.on('start-spotify-batch-download', (event, command) => {
        gamRip.handleBatchDownload(event, command, 'spotify');
    });

    const streamRip = new StreamRip(settingsFilePath, app);

    ipcMain.on('start-qobuz-download', (event, data) => {
        streamRip.handleDownload(event, data, 'qobuz');
    });

    ipcMain.on('start-deezer-download', (event, data) => {
        streamRip.handleDownload(event, data, 'deezer');
    });

    ipcMain.on('start-tidal-download', (event, data) => {
        streamRip.handleDownload(event, data, 'tidal');
    });

    ipcMain.on('start-qobuz-batch-download', (event, data) => {
        streamRip.handleBatchDownload(event, data, 'qobuz');
    });

    ipcMain.on('start-tidal-batch-download', (event, data) => {
        streamRip.handleBatchDownload(event, data, 'tidal');
    });

    ipcMain.on('start-deezer-batch-download', (event, data) => {
        streamRip.handleBatchDownload(event, data, 'deezer');
    });

    ipcMain.on('cancel-download', (_event, order) => {
        cancelDownload(order);
    });
}

module.exports = { registerDownloadHandlers };

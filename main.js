const { app, BrowserWindow, ipcMain,dialog } = require('electron');
const { shell } = require('electron');
const { exec } = require('child_process');
const { execFile } = require('child_process');
const path = require('path');
const fs = require('fs');
const settingsFilePath = path.join(app.getPath('userData'), 'mh-settings.json');
const { saveDownloadToDatabase, loadDownloadsFromDatabase, deleteFromDatabase, deleteDataBase} = require('./funcs/db');
const {getDefaultSettings} = require('./funcs/defaults.js');
const {handleYtDlpDownload, handleYtDlpMusicDownload} = require('./funcs/yt_dlp_downloaders')
const GamRip = require('./funcs/gamRip');
const CustomRip = require('./funcs/customRip');
const { setupSettingsHandlers } = require('./funcs/settings');

let settings = loadTheSettings(); // initialize with defaults
const downloadsDatabasePath = settings.downloads_database;
const failedDownloadsDatabasePath = settings.failed_downloads_database
function loadTheSettings() {
    try {
        const settingsData = fs.readFileSync(settingsFilePath, 'utf8');
        return JSON.parse(settingsData);
    } catch (err) {
        console.log('No user settings found, using default settings.');
        return getDefaultSettings();
    }
}
try {
    const settingsData = fs.readFileSync(settingsFilePath, 'utf8');
    settings = JSON.parse(settingsData);
} catch (error) {
    console.warn('Failed to load settings, using defaults:', error);
}


ipcMain.handle('load-downloads', (event) => {
    return new Promise((resolve, reject) => {
        loadDownloadsFromDatabase((rows) => {
            if (rows) {
                resolve(rows);
            } else {
                reject('No downloads found');
            }
        });
    });
});

ipcMain.handle('deleteDownload', async (event, id) => {
    deleteFromDatabase(event, id)
});

ipcMain.handle('showItemInFolder', async (event, filePath) => {
    try {

        const normalizedPath = path.normalize(filePath);


        if (fs.existsSync(normalizedPath)) {

            if (fs.statSync(normalizedPath).isDirectory()) {

                if (process.platform === 'win32') {
                    require('child_process').exec(`explorer "${normalizedPath}"`);
                } else {
                    shell.openPath(normalizedPath);
                }
            } else {

                shell.showItemInFolder(normalizedPath);
            }
            return true;
        } else {
            throw new Error('File or folder not found');
        }
    } catch (error) {
        console.error('Error showing item in folder:', error);
        throw error;
    }
});

ipcMain.handle('clearDownloadsDatabase', async () => {
    await deleteDataBase()
});
ipcMain.handle('dialog:openwvdFile', async (event) => {
    const currentWindow = BrowserWindow.fromWebContents(event.sender);

    const { canceled, filePaths } = await dialog.showOpenDialog(currentWindow, {
        properties: ['openFile'],
        filters: [{ name: 'Widevine Device Files', extensions: ['wvd'] }],
        title: 'Select File'
    });
    if (canceled) {
        return null;
    } else {
        return filePaths[0];
    }
});
ipcMain.handle('dialog:openFile', async (event) => {
    const currentWindow = BrowserWindow.fromWebContents(event.sender);

    const { canceled, filePaths } = await dialog.showOpenDialog(currentWindow, {
        properties: ['openFile'],
        filters: [{ name: 'Text files', extensions: ['txt'] }],
        title: 'Select File'
    });
    if (canceled) {
        return null;
    } else {
        return filePaths[0];
    }
});

ipcMain.handle('dialog:saveFile', async (event) => {

    const currentWindow = BrowserWindow.fromWebContents(event.sender);

    const { canceled, filePath } = await dialog.showSaveDialog(currentWindow, {
        properties: ['createDirectory'],
        filters: [{ name: 'Database File', extensions: ['db'] }],
        title: 'Select Save Location'
    });

    if (canceled) {
        return null;
    } else {
        return filePath;
    }
});

ipcMain.handle('dialog:openFolder', async (event) => {

    const currentWindow = BrowserWindow.fromWebContents(event.sender);

    const { canceled, filePaths } = await dialog.showOpenDialog(currentWindow, {
        properties: ['openDirectory', 'createDirectory'],
        title: 'Select Folder Location'
    });

    if (canceled) {
        return null;
    } else {
        return filePaths[0];
    }
});
ipcMain.handle('perform-search', async (event, { platform, query, type }) => {
    return new Promise((resolve, reject) => {
        let command;

        switch(platform) {
            case 'youtube':
                command = `py .\\ytsearchapi.py -q "${query}" ${type ? `-t ${type}` : ''}`;
                break;
            case 'youtubeMusic':
                command = `py .\\ytmusicsearchapi.py -q "${query}" ${type ? `-t ${type}` : 'song'}`;
                break;
            case 'spotify':
                command = `py .\\spotifyapi.py --search-${type || 'track'} "${query}"`;
                break;
            case 'tidal':
                command = `py .\\tidalapi.py --search-${type || 'track'} "${query}"`;
                break;
            case 'deezer':
                command = `py .\\deezerapi.py --search-${type || 'track'} "${query}"`;
                break;
            case 'qobuz':
                command = `py .\\qobuzapi.py --search-${type || 'track'} "${query}"`;
                break;
            default:
                reject(new Error('Invalid platform'));
                return;
        }

        exec(command, (error, stdout, stderr) => {
            if (error) {
                reject(error);
                return;
            }

            try {
                const results = JSON.parse(stdout);
                resolve({ results, platform });
            } catch (e) {
                reject(new Error('Failed to parse results'));
            }
        });
    });
});

ipcMain.handle('play-media', async (event, { url, platform }) => {
    return new Promise((resolve, reject) => {
        console.log('Input URL:', url);
        if (!url) {
            reject(new Error('URL cannot be null'));
            return;
        }

        const options = {
            encoding: 'utf8',
            env: { ...process.env, PYTHONIOENCODING: 'utf-8' }
        };

        let command;

        switch(platform) {
            case 'youtube':
                command = `py .\\ytvideostream.py --url "${url}"`;
                break;
            case 'youtubeMusic':
                command = `py .\\ytaudiostream.py --url "${url}"`;
                break;
            case 'qobuz':
                command = `custom_rip -q 4 -ndb streamurl "${url}"`;
                break;
            case 'tidal':
                if (url === 'WIP') {
                    reject(new Error('Work In Progress'));
                    break;
                }
                console.log(settings.tidal_access_token)
                command = `custom_rip -q 1 -ndb streamurl "${url}"`;;
                break;
            default:
                if (url !== "null"){
                    event.sender.send('stream-ready', { streamUrl: url, platform });
                    resolve({ streamUrl: url, platform });
                    return;
                }
                else if (url === 'WIP') {
                    reject(new Error('Work In Progress'));
                }
                reject(new Error('No Stream Found'));
        }

        exec(command, options, (error, stdout, stderr) => {
            if (error) {
                console.error('Execution error:', error);
                reject(error);
                return;
            }

            let streamUrl = '';

            if (platform === 'qobuz') {
                const lines = stdout.split('\n');
                const urlStartMarker = 'https://streaming-qobuz-std.akamaized.net';
                let isCapturing = false;

                for (const line of lines) {
                    if (line.includes(urlStartMarker)) {
                        isCapturing = true;
                    }
                    if (isCapturing) {
                        streamUrl += line.trim();
                    }
                    if (isCapturing && line.trim() === '') {
                        break;
                    }
                }

                if (!streamUrl.startsWith(urlStartMarker)) {
                    console.error('Invalid Qobuz URL format:', streamUrl);
                    reject(new Error('Invalid Qobuz URL format'));
                    return;
                }

                console.log('Found complete Qobuz stream URL:', streamUrl);
            } else if (platform === 'deezer') {
                const match = stdout.match(/Stream URL for the track is:\s*(.*)/);
                streamUrl = match ? match[1].trim() : null;

                if (!streamUrl) {
                    reject(new Error('Could not extract stream URL from output'));
                    return;
                }
            } else {
                streamUrl = stdout.trim();
            }

            if (!streamUrl || streamUrl === '') {
                console.error('Stream URL is empty or invalid');
                reject(new Error('Invalid stream URL'));
                return;
            }

            console.log('Sending stream URL to renderer:', streamUrl);
            event.sender.send('stream-ready', { streamUrl, platform });
            resolve({ streamUrl, platform });
        });

    });
});

ipcMain.handle('stream-ready', async (event, { streamUrl, platform }) => {
    event.sender.send('stream-ready', { streamUrl, platform });
});

function createWindow() {
    const win = new BrowserWindow({
        width: 1200,
        height: 800,
        frame: true,
        autoHideMenuBar: true,
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            preload: path.join(__dirname, 'preload.js')
        }
    });
    win.loadFile('index.html');

    setupSettingsHandlers(ipcMain);
    ipcMain.on("clear-database", (event, { failedDownloads, downloads }) => {
        // Check and delete the databases based on the user’s selection
        if (failedDownloads) {
            fs.unlink(failedDownloadsDatabasePath, (err) => {
                if (err) dialog.showErrorBox("Error", `Failed to delete Failed Downloads Database: ${err.message}`);
            });
        }

        if (downloads) {
            fs.unlink(downloadsDatabasePath, (err) => {
                if (err) dialog.showErrorBox("Error", `Failed to delete Downloads Database: ${err.message}`);
            });
        }

        // Optional: Send feedback to renderer
        event.sender.send("database-clear-status", "Selected databases have been deleted.");
    });


    ipcMain.on('start-yt-music-download', (event, data, playlist) => {
        fs.readFile(settingsFilePath, 'utf8', (err, settingsData) => {
            const settings = err ? getDefaultSettings() : JSON.parse(settingsData);
            handleYtDlpMusicDownload(event, data, settings, playlist);
        });
    });

    ipcMain.on('start-yt-video-download', (event, data) => {
        fs.readFile(settingsFilePath, 'utf8', (err, settingsData) => {
            const settings = err ? getDefaultSettings() : JSON.parse(settingsData);
            handleYtDlpDownload(event, data, settings, false);
        });
    });

    ipcMain.on('start-generic-video-download', (event, data) => {
        fs.readFile(settingsFilePath, 'utf8', (err, settingsData) => {
            const settings = err ? getDefaultSettings() : JSON.parse(settingsData);
            handleYtDlpDownload(event, data, settings, true);
        });
    });

    const gamRip = new GamRip(
        settingsFilePath,
        app,
        {saveDownloadToDatabase}
    );

    ipcMain.on('start-spotify-download', (event, command) => {
        gamRip.handleSpotify(event, command);
    })

    ipcMain.on('start-apple-download', (event, command) => {
        gamRip.handleApple(event, command);
    })
    ipcMain.on('start-apple-batch-download', (event, command) => {
        gamRip.handleAppleMusicBatchDownload(event, command);
    })
    ipcMain.on('start-spotify-batch-download', (event, command) => {
        gamRip.handleSpotifyBatchDownload(event, command);
    })

    const customRip = new CustomRip(
        settingsFilePath,
        app,
        { saveDownloadToDatabase }
    );

    ipcMain.on('start-streamrip', (event, command) => {
        customRip.handleStreamRip(event, command);
    });

    ipcMain.on('start-qobuz-download', (event, data) => {
        customRip.handleQobuzDownload(event, data);
    });

    ipcMain.on('start-deezer-download', (event, data) => {
        customRip.handleDeezerDownload(event, data);
    });

    ipcMain.on('start-tidal-download', (event, data) => {
        customRip.handleTidalDownload(event, data);
    });
    ipcMain.on('start-qobuz-batch-download', (event, data) => {
        customRip.handleQobuzBatchDownload(event, data);
    });

    ipcMain.on('start-tidal-batch-download', (event, data) => {
        customRip.handleTidalBatchDownload(event, data);
    });

    ipcMain.on('start-deezer-batch-download', (event, data) => {
        customRip.handleDeezerBatchDownload(event, data);
    });

}

app.whenReady().then(createWindow);

app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') {
        app.quit();
    }
});

app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
        createWindow();
    }
});


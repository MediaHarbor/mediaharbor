const { app, BrowserWindow, ipcMain,dialog } = require('electron');
const { shell } = require('electron');
const { exec } = require('child_process');
const { spawn } = require('child_process');
const path = require('path');
const UpdateChecker = require('./funcs/updatechecker')
const fs = require('fs');
const settingsFilePath = path.join(app.getPath('userData'), 'mh-settings.json');
const { saveDownloadToDatabase, loadDownloadsFromDatabase, deleteFromDatabase, deleteDataBase} = require('./funcs/db');
const {getDefaultSettings} = require('./funcs/defaults.js');
const {handleYtDlpDownload, handleYtDlpMusicDownload} = require('./funcs/yt_dlp_downloaders')
const GamRip = require('./funcs/gamRip');
const CustomRip = require('./funcs/customRip');
const { setupSettingsHandlers} = require('./funcs/settings');
let settings = loadTheSettings(); // initialize with defaults
const firstTime = settings.firstTime;
const downloadsDatabasePath = settings.downloads_database;
const failedDownloadsDatabasePath = settings.failed_downloads_database

function getPythonCommand() {
    switch (process.platform) {
        case 'win32':
            return 'py';
        case 'darwin':
        case 'linux':
            // fall back to python if python3 not found
            return new Promise((resolve) => {
                exec('python3 --version', (error) => {
                    if (error) {
                        resolve('python');
                    } else {
                        resolve('python3');
                    }
                });
            });
        default:
            return 'python';
    }
}

function loadTheSettings() {
    try {
        const settingsData = fs.readFileSync(settingsFilePath, 'utf8');
        return JSON.parse(settingsData);
    } catch (err) {
        console.log('No user settings found, using default settings.');
        return getDefaultSettings();
    }
}

function getResourcePath(filename) {
    if (app.isPackaged) {
        return path.join(process.resourcesPath, 'app.asar.unpacked', 'src', filename)
            .replace(/\\/g, '/');
    }
    return path.join(__dirname, filename).replace(/\\/g, '/');
}

ipcMain.handle('get-default-settings', async () => {
    return getDefaultSettings();
});
try {
    const settingsData = fs.readFileSync(settingsFilePath, 'utf8');
    settings = JSON.parse(settingsData);
} catch (error) {
    console.warn('Failed to load settings, using defaults:', error);
}

// Add refresh app handler
ipcMain.on('refresh-app', () => {
    app.relaunch();
    app.exit(0);
});

ipcMain.handle('load-downloads', () => {
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
    await deleteFromDatabase(event, id)
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
    const pythonCommand = await getPythonCommand();

    return new Promise((resolve, reject) => {
        let command;
        const scriptPath = getResourcePath(getPythonScript(platform));
        switch(platform) {
            case 'youtube':
                command = `${pythonCommand} "${scriptPath}" -q "${query}" ${type ? `-t ${type}` : ''}`;
                break;
            case 'youtubeMusic':
                command = `${pythonCommand} "${scriptPath}" -q "${query}" ${type ? `-t ${type}` : 'song'}`;
                break;
            case 'spotify':
                command = `${pythonCommand} "${scriptPath}" --search-${type || 'track'} "${query}"`;
                break;
            case 'tidal':
                command = `${pythonCommand} "${scriptPath}" --search-${type || 'track'} "${query}"`;
                break;
            case 'deezer':
                command = `${pythonCommand} "${scriptPath}" --search-${type || 'track'} "${query}"`;
                break;
            case 'qobuz':
                command = `${pythonCommand} "${scriptPath}" --search-${type || 'track'} "${query}"`;
                break;
            default:
                reject(new Error('Invalid platform'));
                return;
        }
        const options = {
            encoding: 'utf8',
            env: {
                ...process.env,
                PYTHONIOENCODING: 'utf-8',
                PATH: process.env.PATH
            }
        };

        exec(command, options, (error, stdout) => {
            if (error) {
                console.error('Search execution error:', error);
                reject(error);
                return;
            }

            try {
                const results = JSON.parse(stdout);
                resolve({ results, platform });
            } catch (e) {
                console.error('JSON parsing error:', e);
                reject(new Error('Failed to parse results'));
            }
        });
    });
});

ipcMain.handle('play-media', async (event, { url, platform }) => {
    const pythonCommand = await getPythonCommand();
    return new Promise((resolve, reject) => {
        console.log('Input URL:', url);
        if (!url) {
            reject(new Error('URL cannot be null'));
            return;
        }

        const options = {
            encoding: 'utf8',
            env: {
                ...process.env,
                PYTHONIOENCODING: 'utf-8',
                PATH: process.env.PATH
            }
        };

        let command;
        const scriptPath = getResourcePath(getPythonStreamScript(platform));
        switch(platform) {
            case 'youtube':
                command = `${pythonCommand} "${scriptPath}" --url "${url}"`;
                break;
            case 'youtubeMusic':
                command = `${pythonCommand} "${scriptPath}" --url "${url}"`;
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
                command = `custom_rip -q 1 -ndb streamurl "${url}"`;
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

        exec(command, options, (error, stdout) => {
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
    win.loadFile(`${__dirname}/index.html`);
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


function createFirstStartWindow() {
    const firstStartWindow = new BrowserWindow({
        width: 780,
        height: 500,
        modal: false,
        frame: true,
        resizable: true,
        autoHideMenuBar: true,
        webPreferences: {
            contextIsolation: true,
            nodeIntegration: false,
            preload: path.join(__dirname, 'preload.js'),
        },
    });

    ipcMain.on('spawn-tidal-config', async () => {
        const customRipCommand = process.platform === 'win32' ? 'custom_rip' : './custom_rip';

        const authProcess = spawn(customRipCommand, ["url", "https://tidal.com/track/1"], {
            env: {
                ...process.env,
                PATH: process.env.PATH
            }
        });

        authProcess.stderr.on('data', (data) => {
            console.error(`Error: ${data}`);
        });

        authProcess.on('close', (code) => {
            console.log(`Process exited with code ${code}`);
        });
    });

    ipcMain.on('install-services', async (event, services) => {
        const pythonCommand = await getPythonCommand();
        const pythonProcess = spawn(pythonCommand, [getResourcePath('start.py'), ...services], {
            env: {
                ...process.env,
                PATH: process.env.PATH
            }
        });
        pythonProcess.stdout.on('data', (data) => {
            // Send the output back to renderer
            event.sender.send('python-output', {
                type: 'output',
                data: data.toString()
            });
        });
        setupSettingsHandlers(ipcMain);
        pythonProcess.stderr.on('data', (data) => {
            // Send error output back to renderer
            event.sender.send('python-output', {
                type: 'error',
                data: data.toString()
            });
        });

        pythonProcess.on('close', (code) => {
            // Send completion status back to renderer
            event.sender.send('python-output', {
                type: 'complete',
                code: code
            });
        });
    });
    firstStartWindow.loadFile(`${__dirname}/firststart.html`);

    // Save settings when window is closed OR when setup is complete
    ipcMain.once('setup-complete', () => {
        settings.firstTime = false;
        fs.writeFileSync('./mh-settings.json', JSON.stringify(settings, null, 2));
        firstStartWindow.close();
    });
}
async function checkForUpdates() {
    try {
        const updateChecker = new UpdateChecker(
            'MediaHarbor',
            'mediaharbor',
            app.getVersion()
        );

        await updateChecker.checkForUpdates();
    } catch (error) {
        console.error('Update check failed:', error);
    }
}

app.whenReady().then(async () => {
    await checkForUpdates();
    if (settings.firstTime) {
        createFirstStartWindow();
    } else {
        createWindow();
    }
});
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
function getPythonScript(platform) {
    const scriptMap = {
        youtube: 'ytsearchapi.py',
        youtubeMusic: 'ytmusicsearchapi.py',
        spotify: 'spotifyapi.py',
        tidal: 'tidalapi.py',
        deezer: 'deezerapi.py',
        qobuz: 'qobuzapi.py'
    };
    return scriptMap[platform] || '';
}

function getPythonStreamScript(platform) {
    const scriptMap = {
        youtube: 'ytvideostream.py',
        youtubeMusic: 'ytaudiostream.py'
    };
    return scriptMap[platform] || '';
}

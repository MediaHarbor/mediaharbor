const {downloadAndInstallBento4} = require( "./funcs/installers/bento4installer");

const {downloadAndInstallFFmpeg} = require("./funcs/installers/ffmpegInstaller");
const {downloadAndInstallGit} = require("./funcs/installers/gitInstaller");
const {downloadAndInstallPython} = require("./funcs/installers/pythonInstaller");
const {clipboard , Menu, MenuItem, app, BrowserWindow, ipcMain,dialog, shell  } = require('electron');
const { exec, spawn } = require('child_process');
const util = require('util');
const execPromise = util.promisify(exec);
const os = require('os');
const path = require('path');
const fs = require('fs');
const UpdateChecker = require('./funcs/updatechecker')
const { saveDownloadToDatabase, loadDownloadsFromDatabase, deleteFromDatabase, deleteDataBase} = require('./funcs/db');
const {getDefaultSettings} = require('./funcs/defaults.js');
const {handleYtDlpDownload, handleYtDlpMusicDownload} = require('./funcs/yt_dlp_downloaders')
const GamRip = require('./funcs/gamRip');
const CustomRip = require('./funcs/customRip');
const { setupSettingsHandlers} = require('./funcs/settings');
const {getPythonCommand} = require('./funcs/spawner');
const settingsFilePath = path.join(app.getPath('userData'), 'mh-settings.json');
let settings = loadTheSettings();
const downloadsDatabasePath = settings.downloads_database;
const failedDownloadsDatabasePath = settings.failed_downloads_database
const sudo = require('sudo-prompt');
const axios = require('axios');
const ProgressBar = require('progress');
const unzipper = require('unzipper');
if (process.platform === 'darwin' || process.platform === 'linux') {
    import('fix-path').then((module) => {
        module.default();
    }).catch(err => console.error('Failed to load fix-path:', err));
}
function setupContextMenu(win) {
    win.webContents.on('context-menu', (event, params) => {
        if (params.isEditable) {
            const hasSelection = params.selectionText.trim().length > 0;
            const clipboardHasText = clipboard.availableFormats().includes('text/plain');
            // Create the context menu
            const contextMenu = new Menu();
            contextMenu.append(new MenuItem({
                label: 'Cut',
                role: 'cut',
                enabled: hasSelection
            }));
            contextMenu.append(new MenuItem({
                label: 'Copy',
                role: 'copy',
                enabled: hasSelection
            }));
            contextMenu.append(new MenuItem({
                label: 'Paste',
                role: 'paste',
                enabled: clipboardHasText
            }));
            contextMenu.append(new MenuItem({ type: 'separator' }));
            contextMenu.append(new MenuItem({
                label: 'Select All',
                role: 'selectall'
            }));
            // Show the context menu
            contextMenu.popup({ window: win });
        }
    });
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

ipcMain.on('updateDep', (event, selectedPackages) => {
    console.log('Received packages:', selectedPackages); // For debugging
    event.reply('toggleLoading', true); // Show loading overlay
    updateDependencies(selectedPackages, event);
});

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
                    await shell.openPath(normalizedPath);
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
            case 'applemusic':
                command = `${pythonCommand} "${scriptPath}" "${query}" --media_type ${type || 'track'}`;
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
                const results = JSON.parse(stdout.toString());
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
    win.loadFile(`${__dirname}/pages/index.html`);
    setupContextMenu(win);
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
        gamRip.handleDownload(event, command, 'spotify')
    })

    ipcMain.on('start-apple-download', (event, command) => {
        gamRip.handleDownload(event, command, 'applemusic')
    })
    ipcMain.on('start-apple-batch-download', (event, command) => {
        gamRipInstance.handleBatchDownload(event, data, 'applemusic');
    })
    ipcMain.on('start-spotify-batch-download', (event, command) => {
        gamRipInstance.handleBatchDownload(event, data, 'spotify');
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
        customRip.handleDownload(event, data, 'qobuz');
    });

    ipcMain.on('start-deezer-download', (event, data) => {
        customRip.handleDownload(event, data, 'deezer');
    });

    ipcMain.on('start-tidal-download', (event, data) => {
        customRip.handleDownload(event, data, 'tidal');
    });
    ipcMain.on('start-qobuz-batch-download', (event, data) => {
        customRip.handleBatchDownload(event, data, 'qobuz');
    });

    ipcMain.on('start-tidal-batch-download', (event, data) => {
        customRip.handleBatchDownload(event, data, 'tidal');
    });

    ipcMain.on('start-deezer-batch-download', (event, data) => {
        customRip.handleBatchDownload(event, data, 'deezer');
    });

}


async function createFirstStartWindow() {
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
    setupFirstStartHandlers(firstStartWindow);
    await firstStartWindow.loadFile(`${__dirname}/pages/firststart.html`);

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
        youtube: './funcs/apis/ytsearchapi.py',
        youtubeMusic: './funcs//apis/ytmusicsearchapi.py',
        spotify: './funcs/apis/spotifyapi.py',
        tidal: './funcs/apis/tidalapi.py',
        deezer: './funcs/apis/deezerapi.py',
        qobuz: './funcs/apis/qobuzapi.py',
        applemusic: './funcs/apis/applemusicapi.py'
    };
    return scriptMap[platform] || '';
}

function getPythonStreamScript(platform) {
    const scriptMap = {
        youtube: './funcs/apis/ytvideostream.py',
        youtubeMusic: './funcs/apis/ytaudiostream.py'
    };
    return scriptMap[platform] || '';
}
function getPipCommand() {
    const platform = os.platform();
    switch (platform) {
        case 'win32':
            return 'py -m pip';  // Use py launcher with -m pip on Windows
        case 'darwin':
        case 'linux':
            return 'python3 -m pip';
        default:
            return 'pip';
    }
}

function sanitizePackageName(packageName) {
    return packageName.replace(/[;&|`$]/g, '');
}

function updateDependencies(packages, event) {
    if (!Array.isArray(packages)) {
        console.error("Expected an array of packages.");
        return;
    }

    console.log('Received packages:', packages);
    const pipCommand = getPipCommand();
    let completedCount = 0;

    packages.forEach(packageName => {
        const sanitizedPackage = sanitizePackageName(packageName);
        let installCommand = `${pipCommand} install --upgrade "${sanitizedPackage}"`;

        // Add --user flag for non-root installations on Unix systems
        if (os.platform() !== 'win32') {
            installCommand += ' --user';
        }

        console.log(`Executing: ${installCommand}`);

        exec(installCommand, { shell: true }, (error, stdout, stderr) => {
            completedCount++;

            if (error) {
                console.error(`Error installing ${sanitizedPackage}:`, error.message);
                event.reply('showNotification', {
                    type: 'error',
                    message: `Failed to install ${sanitizedPackage}: ${error.message}`
                });
            } else {
                event.reply('showNotification', {
                    type: 'success',
                    message: `Successfully installed ${sanitizedPackage}`
                });
            }

            if (stderr) {
                console.warn(`stderr for ${sanitizedPackage}:`, stderr);
            }
            console.log(`stdout for ${sanitizedPackage}:`, stdout);

            if (completedCount === packages.length) {
                event.reply('toggleLoading', false);
            }
        });
    });
}
async function getPythonVersionOutput(command) {
    try {
        const { stdout, stderr } = await execPromise(`${command} --version`);
        const output = stdout.trim() || stderr.trim();
        const versionMatch = output.match(/Python (\d+\.\d+\.\d+)/);
        return versionMatch ? versionMatch[1] : null;
    } catch (error) {
        return null;
    }
}

/**
 * Checks if the installed Python version is between 3.9 and 3.12 inclusive.
 * @returns {Promise<boolean>} - True if the Python version is within the range, else false.
 */
async function checkPythonVersion() {
    const commandsToTry = ['python', 'python3', 'py'];

    for (const command of commandsToTry) {
        const version = await getPythonVersionOutput(command);
        if (version) {
            const [major, minor] = version.split('.').map(Number);
            if (major === 3 && minor >= 9 && minor <= 12) {
                return true;
            }
        }
    }

    return false;
}

async function checkGit() {
    try {
        await execPromise('git --version');
        return true;
    } catch (error) {
        console.error('Git check failed:', error.message);
        return false;
    }
}

function checkFFmpeg() {
    return new Promise((resolve) => {
        const process = spawn('ffmpeg', ['-version']);
        process.on('close', (code) => {
            resolve(code === 0);
        });
        process.on('error', () => {
            resolve(false);
        });
    });
}

function setupFirstStartHandlers(win) {
    ipcMain.handle('check-dependencies', async () => {
        const status = {
            python: await checkPythonVersion(),
            git: await checkGit(),
            ffmpeg: await checkFFmpeg(),
            ytdlp: false,
            ytmusic: false,
            qobuz: false,
            deezer: false,
            tidal: false,
            apple: false,
            spotify: false,
        };

        // If Python is installed, check pip packages
        if (status.python) {
            try {
                const pythonCommand = await getPythonCommand();
                const { stdout, stderr } = await execPromise(`${pythonCommand} -m pip list`);

                console.log('pip list stdout:', stdout);
                console.log('pip list stderr:', stderr);

                if (typeof stdout === 'string') {
                    status.ytdlp = stdout.includes('yt-dlp');
                    status.ytmusic = stdout.includes('ytmusicapi');
                    status.qobuz = stdout.includes('custom_streamrip');
                    status.deezer = stdout.includes('custom_streamrip');
                    status.tidal = stdout.includes('custom_streamrip');
                    status.apple = stdout.includes('custom_gamdl');
                    status.spotify = stdout.includes('custom_votify');
                } else {
                    console.error('Unexpected type for pipList.stdout:', typeof stdout);
                }
            } catch (error) {
                console.error('Failed to check pip packages:', error);
            }
        }

        win.webContents.send('dependency-status', status);
        return status;
    });

    ipcMain.handle('install-dependency', async (event, dep) => {
        try {
            const pythonCommand = await getPythonCommand();

            switch (dep) {
                case 'git':
                    await downloadAndInstallGit(win);
                    break;
                case 'python':
                    await downloadAndInstallPython(win);
                    break;
                case 'ffmpeg':
                    await downloadAndInstallFFmpeg(win);
                    break;
                case 'ytdlp':
                    await installWithProgress(`${pythonCommand} -m pip install --upgrade yt-dlp`, win, dep);
                    break;
                case 'ytmusic':
                    await installWithProgress(`${pythonCommand} -m pip install --upgrade ytmusicapi`, win, dep);
                    break;
                case 'qobuz':
                case 'deezer':
                case 'tidal':
                    await installWithProgress(
                        `${pythonCommand} -m pip install --upgrade git+https://github.com/mediaharbor/custom_streamrip.git`,
                        win,
                        dep
                    );
                    break;
                case 'apple':
                    await downloadAndInstallBento4(win);
                    await installWithProgress(
                        `${pythonCommand} -m pip install --upgrade pyapplemusicapi git+https://github.com/mediaharbor/custom_gamdl.git`,
                        win,
                        dep
                    );
                    break;
                case 'spotify':
                    await downloadAndInstallBento4(win);
                    await installWithProgress(
                        `${pythonCommand} -m pip install --upgrade git+https://github.com/mediaharbor/custom_votify.git`,
                        win,
                        dep
                    );
                    break;
                default:
                    throw new Error(`Unknown dependency: ${dep}`);
            }

            return true;
        } catch (error) {
            console.error(`Failed to install ${dep}:`, error);
            throw error;
        }
    });


    ipcMain.handle('complete-setup', async () => {
        try {
            const settings = loadTheSettings();
            settings.firstTime = false;
            await fs.promises.writeFile(settingsFilePath, JSON.stringify(settings, null, 2));
            return true;
        } catch (error) {
            console.error('Failed to complete setup:', error);
            throw error;
        }
    });

    ipcMain.on('restart-app', () => {
        app.relaunch();
        app.exit(0);
    });
}
async function installWithProgress(command, win, dep) {
    return new Promise((resolve, reject) => {
        const process = spawn(command, { shell: true });

        process.stdout.on('data', (data) => {
            const output = data.toString().trim(); // Convert to plain string
            console.log(output); // Debugging purpose

            // Match progress percentage from output if it exists
            const progressMatch = output.match(/(?<=\()\d{1,3}(?=%\))/);
            const progress = progressMatch ? parseInt(progressMatch[0], 10) : null;

            // Send progress as a plain JSON string
            win.webContents.send('installation-progress', JSON.stringify({
                dependency: dep,
                percent: progress,
                status: progress ? `Installing ${dep}: ${progress}%` : output,
            }));
        });

        process.stderr.on('data', (data) => {
            const errorOutput = data.toString().trim(); // Convert to plain string
            console.error(errorOutput);

            // Send error as a plain JSON string
            win.webContents.send('installation-error', JSON.stringify({
                dependency: dep,
                error: errorOutput,
            }));
        });

        process.on('close', (code) => {
            if (code === 0) {
                win.webContents.send('installation-progress', JSON.stringify({
                    dependency: dep,
                    percent: 100,
                    status: `${dep} installation completed successfully.`,
                }));
                resolve();
            } else {
                reject(new Error(`${dep} installation failed with exit code ${code}`));
            }
        });
    });
}

// Back-up error notifier (For temporary)
const originalConsoleError = console.error;
console.error = function (...args) {
    originalConsoleError.apply(console, args);
    const message = args.map(arg => {
        if (arg instanceof Error) {
            return arg.message;
        }
        return typeof arg === 'object' ? JSON.stringify(arg) : String(arg);
    }).join(' ');
    if (mainWindow && mainWindow.webContents) {
        mainWindow.webContents.send('out-error', message);
    }
};
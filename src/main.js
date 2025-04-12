const {downloadAndInstallBento4} = require( "./funcs/installers/bento4installer");

const {downloadAndInstallFFmpeg} = require("./funcs/installers/ffmpegInstaller");
const {downloadAndInstallGit} = require("./funcs/installers/gitInstaller");
const {downloadAndInstallPython} = require("./funcs/installers/pythonInstaller");
const {clipboard , Menu, MenuItem, app, BrowserWindow, ipcMain,dialog, shell,protocol   } = require('electron');
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

ipcMain.handle('copy-handler', async (event, text) => {
    try {
        return await clipboard.writeText(text);
    } catch (error) {
        console.error('Copy error:', error);
        throw error;
    }
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
                    resolve({ streamUrl, platform });
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


async function handleGetAlbumDetails(platform, albumId) {
    const platformScripts = {
        qobuz: 'funcs/apis/qobuzapi.py',
        tidal: 'funcs/apis/tidalapi.py',
        deezer: 'funcs/apis/deezerapi.py',
        youtubeMusic: 'funcs/apis/ytmusicsearchapi.py',
        spotify: 'funcs/apis/spotifyapi.py',
    };

    return new Promise(async (resolve, reject) => {
        try {
            const pythonScript = platformScripts[platform];
            if (!pythonScript) {
                throw new Error(`Unsupported platform: ${platform}`);
            }

            const scriptPath = getResourcePath(pythonScript);
            const pythonCommand = await getPythonCommand();

            const process = spawn(pythonCommand, [scriptPath, '--get-track-list', `album/${albumId}`]);
            let stdout = '';
            let stderr = '';

            process.stdout.on('data', (data) => {
                stdout += data.toString();
            });

            process.stderr.on('data', (data) => {
                stderr += data.toString();
            });

            process.on('close', (code) => {
                if (code === 0) {
                    try {
                        const result = JSON.parse(stdout);
                        const formattedResult = formatPlatformResponseAlbum(platform, result);
                        resolve(formattedResult);
                    } catch (error) {
                        console.error('JSON Parse Error:', error);
                        console.log('Raw stdout:', stdout);
                        reject(new Error(`Failed to parse JSON: ${error.message}`));
                    }
                } else {
                    console.error(`Process error: ${stderr}`);
                    reject(new Error(`Process failed with code ${code}: ${stderr}`));
                }
            });

            process.on('error', (error) => {
                reject(new Error(`Failed to start process: ${error.message}`));
            });
        } catch (error) {
            reject(error);
        }
    });
}

function formatPlatformResponseAlbum(platform, result) {
    const formatters = {
        qobuz: (data) => {
            const albumInfo = {
                title: 'Unknown Album',
                artist: data[0]?.performer?.name || 'Unknown Artist',
                releaseDate: data[0]?.release_date_original || 'Unknown Date',
                coverUrl: '',
                description: '',
                duration: data.reduce((sum, track) => sum + (track.duration || 0), 0),
                genre: '',
            };

            const tracks = data.map((track, index) => ({
                id: track.id,
                number: track.track_number || (index + 1),
                title: track.title,
                duration: track.duration,
                quality: `${track.maximum_bit_depth || '16'}bit / ${track.maximum_sampling_rate || '44.1'}kHz`,
                playUrl: track.id ? `https://play.qobuz.com/track/${track.id}` : null,
                artist: track.performer ? track.performer.name : albumInfo.artist,
            }));

            return { album: albumInfo, tracks };
        },

        tidal: (data) => {
            const albumData = data.data.attributes;
            const trackList = data.included;
            const trackOrderMap = new Map(
                data.data.relationships.items.data.map((item, index) => [item.id, index + 1])
            );

            const albumInfo = {
                title: albumData.title || 'Unknown Album',
                artist: 'Unknown Artist',
                releaseDate: albumData.releaseDate || 'Unknown Date',
                coverUrl: albumData.imageLinks?.[0]?.href || '',
                description: '',
                duration: parseDuration(albumData.duration),
                genre: '',
            };

            const tracks = trackList
                .filter(track => track.type === 'tracks')
                .sort((a, b) => {
                    const aOrder = trackOrderMap.get(a.id) || 0;
                    const bOrder = trackOrderMap.get(b.id) || 0;
                    return aOrder - bOrder;
                })
                .map(track => ({
                    id: track.id,
                    number: trackOrderMap.get(track.id) || 0,
                    title: track.attributes.title,
                    duration: parseDuration(track.attributes.duration),
                    quality: track.attributes.mediaTags.includes('LOSSLESS') ? '16bit / 44.1kHz' : 'AAC',
                    playUrl: track.attributes.externalLinks?.[0]?.href || null,
                    artist: 'Unknown Artist',
                }));

            return { album: albumInfo, tracks };
        },

        deezer: (data) => {
            const albumInfo = {
                title: data.name || 'Unknown Album',
                artist: data.artist || 'Unknown Artist',
                releaseDate: data.release_date || 'Unknown Date',
                coverUrl: data.md5_image ? `https://e-cdns-images.dzcdn.net/images/cover/${data.md5_image}/1000x1000.jpg` : '',
                description: '',
                duration: data.tracks.reduce((sum, track) => sum + (track.duration || 0), 0),
                genre: '',
            };

            const tracks = data.tracks.map(track => ({
                id: track.id,
                number: track.track_position || 0,
                title: track.title,
                duration: track.duration,
                quality: '',
                playUrl: track.preview || null,
                artist: track.artist?.name || albumInfo.artist,
            }));

            return { album: albumInfo, tracks };
        },

        youtubeMusic: (data) => {
            const albumInfo = {
                title: data.album.title || 'Unknown Album',
                artist: data.album.artist || 'Unknown Artist',
                releaseDate: data.album.releaseDate || 'Unknown Date',
                coverUrl: data.album.coverUrl || '',
                description: data.album.description || '',
                duration: data.album.duration || 0,
                genre: data.album.genre || '',
            };

            const tracks = data.tracks.map(track => ({
                id: track.id || '',
                number: track.number || 0,
                title: track.title || 'Unknown Title',
                duration: track.duration || 0,
                quality: track.quality || '256Kbps',
                playUrl: track.playUrl || null,
                artist: albumInfo.artist,
            }));

            return { album: albumInfo, tracks };
        },

        spotify: (data) => {
            const albumInfo = {
                title: data.album_name || 'Unknown Album',
                artist: data.artist_name || 'Unknown Artist',
                releaseDate: data.release_date || 'Unknown Date',
                coverUrl: data.cover_url || '',
                description: '',
                duration: '',
                genre: '',
            };

            const tracks = data.tracks.map(track => ({
                id: track.id,
                number: track.track_number || 0,
                title: track.name,
                duration: Math.floor(track.duration_ms / 1000),
                quality: '',
                playUrl: track.preview_url || null,
                artist: track.artists?.[0]?.name || albumInfo.artist,
            }));

            return { album: albumInfo, tracks };
        },
    };

    const formatter = formatters[platform];
    if (!formatter) {
        throw new Error(`No formatter available for platform: ${platform}`);
    }

    return formatter(result);
}

async function handleGetPlaylistDetails(platform, playlistId) {
    const platformScripts = {
        qobuz: 'funcs/apis/qobuzapi.py',
        tidal: 'funcs/apis/tidalapi.py',
        deezer: 'funcs/apis/deezerapi.py',
        youtubeMusic: 'funcs/apis/ytmusicsearchapi.py',
        spotify: 'funcs/apis/spotifyapi.py',
        youtube: 'funcs/apis/ytsearchapi.py'
    };

    return new Promise(async (resolve, reject) => {
        try {
            const pythonScript = platformScripts[platform];
            if (!pythonScript) {
                throw new Error(`Unsupported platform: ${platform}`);
            }

            const scriptPath = getResourcePath(pythonScript);
            const pythonCommand = await getPythonCommand();

            const process = spawn(pythonCommand, [scriptPath, '--get-track-list', `playlist/${playlistId}`]);
            let stdout = '';
            let stderr = '';

            process.stdout.on('data', (data) => {
                stdout += data.toString();
            });

            process.stderr.on('data', (data) => {
                stderr += data.toString();
            });

            process.on('close', (code) => {
                if (code === 0) {
                    try {
                        const result = JSON.parse(stdout);
                        const formattedResult = formatPlatformResponsePlaylist(platform, result);
                        resolve(formattedResult);
                    } catch (error) {
                        console.error('JSON Parse Error:', error);
                        console.log('Raw stdout:', stdout);
                        reject(new Error(`Failed to parse JSON: ${error.message}`));
                    }
                } else {
                    console.error(`Process error: ${stderr}`);
                    reject(new Error(`Process failed with code ${code}: ${stderr}`));
                }
            });

            process.on('error', (error) => {
                reject(new Error(`Failed to start process: ${error.message}`));
            });
        } catch (error) {
            reject(error);
        }
    });
}

function formatPlatformResponsePlaylist(platform, result) {
    const formatters = {
        qobuz: (data) => {
            const playlistInfo = {
                title: 'Unknown Playlist',
                creator: '',
                creationDate: data[0]?.creation_date || 'Unknown Date',
                coverUrl: '',
                description: data[0]?.description || 'No description available',
                duration: data.reduce((sum, track) => sum + (track.duration || 0), 0),
                totalTracks: data.length,
            };

            const tracks = data.map((track, index) => ({
                id: track.id,
                number: track.position || (index + 1),
                title: track.title,
                cover: track.album.image.small,
                albumTitle: track.album.title,
                albumArtist: track.album.artist.name,
                explicit: track.parental_warning,
                duration: track.duration,
                quality: `${track.maximum_bit_depth || '16'}bit / ${track.maximum_sampling_rate || '44.1'}kHz`,
                playUrl: track.url || null,
                artist: track.performer ? track.performer.name : 'Unknown Artist',
            }));

            return { playlist: playlistInfo, tracks };
        },

        deezer: (data) => {
            const playlistInfo = {
                title: data.name || 'Unknown Playlist',
                creator: data.artist || 'Unknown Creator',
                creationDate: data.release_date || 'Unknown Date',
                coverUrl: data.md5_image
                    ? `https://e-cdns-images.dzcdn.net/images/cover/${data.md5_image}/1000x1000.jpg`
                    : '',
                description: '',
                duration: data.tracks.reduce((sum, track) => sum + (track.duration || 0), 0),
                totalTracks: data.total_tracks || data.tracks.length
            };

            const tracks = data.tracks.map((track, index) => {
                return {
                    id: track.id,
                    number: index + 1,
                    title: track.title,
                    cover: track.album?.cover_small,
                    albumTitle: track.album?.title,
                    explicit: track.explicit_lyrics,
                    duration: track.duration,
                    quality: 'MP3 320kbps',
                    playUrl: track.preview || null,
                    albumArtist: track.artist?.name || 'Unknown Artist'
                };
            });

            return { playlist: playlistInfo, tracks };
        },


        youtubeMusic: (data) => {
            const playlistInfo = {
                title: data.album.title || 'Unknown Album',
                artist: data.album.artist || 'Unknown Artist',
                releaseDate: data.album.releaseDate || 'Unknown Date',
                coverUrl: data.album.coverUrl || '',
                description: data.album.description || '',
                duration: data.album.duration || 0,
                genre: data.album.genre || '',
            };

            const tracks = data.tracks.map(track => ({
                id: track.id || '',
                number: track.number || 0,
                title: track.title || 'Unknown Title',
                duration: track.duration || 0,
                quality: track.quality || '256Kbps',
                playUrl: track.playUrl || null,
                artist: playlistInfo.artist,
            }));

            return { playlist: playlistInfo, tracks };
        },

        spotify: (data) => {
            const playlistInfo = {
                title: data.playlist_name || 'Unknown Album',
                artist: data.owner_name || 'Unknown Artist',
                releaseDate: '', // No release date at playlist level
                coverUrl: data.cover_url || '',
                description: '',
                duration: '',
                genre: '',
            };

            const tracks = data.tracks.map((item, index) => {
                const track = item.track;
                return {
                    id: track?.id,
                    number: index + 1,
                    title: track.name,
                    duration: Math.floor(track.duration_ms / 1000),
                    quality: '320 Kbps',
                    playUrl: track.preview_url || null,
                    cover: track.album.images[2]?.url || '',
                    albumArtist: track.album.artists[0]?.name || '',
                    albumTitle: track.album?.name || '',
                };
            });

            return { playlist: playlistInfo, tracks };
        },


        youtube: (data) => {
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
                duration: track.duration,
            }));

            return { playlist: playlistInfo, tracks };
        }
    };

    const formatter = formatters[platform];
    if (!formatter) {
        throw new Error(`No formatter available for platform: ${platform}`);
    }

    return formatter(result);
}



function parseDuration(duration) {
    if (!duration) return 0;

    const matches = duration.match(/PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?/);
    if (!matches) return 0;

    const [_, hours, minutes, seconds] = matches;
    return (parseInt(hours || 0) * 3600) +
        (parseInt(minutes || 0) * 60) +
        (parseInt(seconds || 0));
}
// IPC handler
ipcMain.handle('get-album-details', async (event, platform, albumId) => {
    try {
        const result = await handleGetAlbumDetails(platform, albumId);
        return {
            success: true,
            data: result
        };
    } catch (error) {
        console.error('Error in get-album-details:', error);
        return {
            success: false,
            error: error.message
        };
    }
});

ipcMain.handle('get-playlist-details', async (event, platform, playlistId) => {
    try {
        const result = await handleGetPlaylistDetails(platform, playlistId);
        return {
            success: true,
            data: result
        };
    } catch (error) {
        console.error('Error in get-playlist-details:', error);
        return {
            success: false,
            error: error.message
        };
    }
});

ipcMain.handle('get-qobuz-track-list', async (event, { playUrl }) => {
    const pythonCommand = await getPythonCommand(); // Ensure this function is defined
    const platform = "qobuz"
    const scriptPath = getResourcePath(getPythonStreamScript(platform));

    return new Promise((resolve, reject) => {
        const command = `${pythonCommand} "${scriptPath}" --get-track-list "${playUrl}"`;
        exec(command, { encoding: 'utf8' }, (error, stdout, stderr) => {
            if (error) {
                console.error('Error executing Qobuz API script:', error);
                reject(new Error('Failed to retrieve track list'));
                return;
            }

            try {
                console.log(stdout)
                const trackList = JSON.parse(stdout);
                resolve(trackList);
            } catch (parseError) {
                console.error('Error parsing track list:', parseError);
                reject(new Error('Invalid track list format'));
            }
        });
    });
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
            preload: path.join(__dirname, 'preload.js')
        }
    });
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
        if (win && win.webContents) {
            win.webContents.send('out-error', message);
        }
    };

    const gotTheLock = app.requestSingleInstanceLock();
    // Remove during $npm start
    if (gotTheLock) {
        const argv = process.argv;
        if (process.platform === 'win32' && argv.length >= 2) {
            handleProtocolUrl(argv[1]);
        }
    }

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

    ipcMain.handle('clear-zotify-credentials', async (event) => {
        try {
            await gamRip.clearCredentials();
            return { success: true, message: 'Credentials cleared successfully' };
        } catch (error) {
            console.error('Error clearing credentials:', error);
            return { success: false, message: error.message };
        }
    });

    ipcMain.on('start-spotify-download', (event, command) => {
        gamRip.handleDownload(event, command, 'spotify')
    })

    ipcMain.on('start-apple-download', (event, command) => {
        gamRip.handleDownload(event, command, 'applemusic')
    })
    ipcMain.on('start-apple-batch-download', (event, command) => {
        gamRip.handleBatchDownload(event, data, 'applemusic');
    })
    ipcMain.on('start-spotify-batch-download', (event, command) => {
        gamRip.handleBatchDownload(event, data, 'spotify');
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
app.setAsDefaultProtocolClient('mediaharbor');
app.on('open-url', (event, url) => {
    event.preventDefault();
    handleProtocolUrl(url);
});
if (process.platform === 'win32') {
    const gotTheLock = app.requestSingleInstanceLock();

    if (!gotTheLock) {
        app.quit();
    } else {
        app.on('second-instance', (event, argv) => {
            if (process.platform === 'win32') {
                console.log('Second instance argv:', argv);
                const protocolUrl = argv.find(arg => arg.startsWith('mediaharbor://'));
                if (protocolUrl) {
                    handleProtocolUrl(protocolUrl);
                } else {
                    console.warn('No mediaharbor protocol URL found in argv:', argv);
                }
            }

            const mainWindow = BrowserWindow.getAllWindows()[0];
            if (mainWindow) {
                if (mainWindow.isMinimized()) mainWindow.restore();
                mainWindow.focus();
            }
        });
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
        youtubeMusic: './funcs/apis/ytaudiostream.py',
        qobuz: './funcs/apis/qobuzapi.py'
    };
    return scriptMap[platform] || '';
}
function getPipCommand() {
    const platform = os.platform();
    switch (platform) {
        case 'win32':
            return 'python -m pip';
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
    if (!Array.isArray(packages) || packages.length === 0) {
        console.error("Expected a non-empty array of packages.");
        event.sender.send('showNotification', {
            type: 'error',
            message: 'Invalid package list provided'
        });
        event.sender.send('toggleLoading', false);
        return;
    }

    console.log('Updating packages:', packages);
    event.sender.send('toggleLoading', true);

    const pipCommand = getPipCommand();
    const promises = packages.map(packageName => {
        return new Promise((resolve) => {
            const sanitizedPackage = sanitizePackageName(packageName);
            let installCommand = `${pipCommand} install --upgrade "${sanitizedPackage}"`;

            if (os.platform() !== 'win32' && process.getuid && process.getuid() !== 0) {
                installCommand += ' --user';
            }

            console.log(`Executing: ${installCommand}`);

            exec(installCommand, { shell: true }, (error, stdout, stderr) => {
                if (error) {
                    console.error(`Error installing ${sanitizedPackage}:`, error.message);
                    event.sender.send('showNotification', {
                        type: 'error',
                        message: `Failed to install ${sanitizedPackage}: ${error.message}`
                    });
                } else {
                    console.log(`Successfully installed ${sanitizedPackage}`);
                    event.sender.send('showNotification', {
                        type: 'success',
                        message: `Successfully installed ${sanitizedPackage}`
                    });
                }

                if (stderr && stderr.trim()) {
                    console.warn(`stderr for ${sanitizedPackage}:`, stderr);
                }

                if (stdout && stdout.trim()) {
                    console.log(`stdout for ${sanitizedPackage}:`, stdout);
                }

                resolve();
            });
        });
    });

    Promise.all(promises)
        .then(() => {
            console.log('All package installations completed');
            event.sender.send('toggleLoading', false);
            event.sender.send('showNotification', {
                type: 'success',
                message: `Update completed for ${packages.length} package(s)`
            });
        })
        .catch(err => {
            console.error('Unexpected error during installation:', err);
            event.sender.send('toggleLoading', false);
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
 * Checks if the installed Python version is between 3.10 and 3.13 inclusive.
 * @returns {Promise<boolean>} - True if the Python version is within the range, else false.
 */
async function checkPythonVersion() {
    const commandsToTry = ['python', 'python3', 'py'];

    for (const command of commandsToTry) {
        const version = await getPythonVersionOutput(command);
        if (version) {
            const [major, minor] = version.split('.').map(Number);
            if (major === 3 && minor >= 10 && minor <= 13) {
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
                    status.spotify = stdout.includes('custom_zotify');
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
                    await installWithProgress(`${pythonCommand} -m pip install --upgrade yt-dlp isodate`, win, dep);
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
                    
                    await downloadAndInstallBento4(win)
                    

                    await installWithProgress(
                        `${pythonCommand} -m pip install --upgrade pyapplemusicapi`,
                        win,
                        'apple',
                        {
                            startPercent: 30,
                            endPercent: 60,
                            prefix: 'Installing pyapplemusicapi'
                        }
                    );
                    await installWithProgress(
                        `${pythonCommand} -m pip install --upgrade pyapplemusicapi`,
                        win,
                        'apple',
                        {
                            startPercent: 30,
                            endPercent: 60,
                            prefix: 'Installing pyapplemusicapi'
                        }
                    );

                    await installWithProgress(
                        `${pythonCommand} -m pip install --upgrade git+https://github.com/mediaharbor/custom_gamdl.git`,
                        win,
                        'apple',
                        {
                            startPercent: 60,
                            endPercent: 100,
                            prefix: 'Installing custom_gamdl'
                        }
                    );
                    break;
                case 'spotify':
                    await installWithProgress(
                        `${pythonCommand} -m pip install --upgrade git+https://github.com/mediaharbor/custom_zotify.git`,
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
function installWithProgress(command, win, dep, options = {}) {
    const {
        startPercent = 0,
        endPercent = 100,
        prefix = `Installing ${dep}`
    } = options;

    return new Promise((resolve, reject) => {
        let currentCommand = command;
        const executeCommand = (cmd) => {
            const process = spawn(cmd, { shell: true });
            let hasErrors = false;
            let lastOutput = '';
            let externallyManagedError = false;

            const sendProgress = (progress, status) => {
                const scaledProgress = Math.floor(
                    startPercent + ((progress / 100) * (endPercent - startPercent))
                );
                win.webContents.send('installation-progress', JSON.stringify({
                    dependency: dep,
                    percent: scaledProgress,
                    status: status || `${prefix}: ${progress}%`
                }));
            };

            process.stdout.on('data', (data) => {
                const output = data.toString().trim();
                console.log(`[${dep}] stdout:`, output);
                lastOutput = output;

                if (output.includes('Cloning into')) {
                    sendProgress(10, `${prefix}: Cloning repository...`);
                } else if (output.includes('Receiving objects:')) {
                    const match = output.match(/Receiving objects:\s+(\d+)%/);
                    if (match) {
                        const progress = parseInt(match[1]);
                        sendProgress(10 + (progress * 0.3), `${prefix}: Cloning repository ${progress}%`);
                    }
                }
                else if (output.includes('Building wheels')) {
                    sendProgress(50, `${prefix}: Building package...`);
                } else if (output.includes('Successfully built')) {
                    sendProgress(75, `${prefix}: Package built successfully`);
                }

                else {
                    const downloadMatch = output.match(/(?<=\()\d{1,3}(?=%\))/);
                    if (downloadMatch) {
                        const progress = parseInt(downloadMatch[0]);
                        sendProgress(progress);
                    }
                }

                if (output.includes('Successfully installed')) {
                    sendProgress(100, `${prefix}: Installation completed`);
                }
            });

            process.stderr.on('data', (data) => {
                const errorOutput = data.toString().trim();

                if (errorOutput.includes('externally-managed-environment')) {
                    externallyManagedError = true;
                    return;
                }

                if (errorOutput.includes('is installed in') && errorOutput.includes('which is not on PATH')) {
                    const pathMatch = errorOutput.match(/'([^']+)'/);
                    if (pathMatch) {
                        const pythonPath = pathMatch[1];

                        let plainMessage = '';
                        const platform = process.platform;
                        
                        if (platform === 'darwin') {
                            plainMessage = `Package Installed Successfully
Run this command to add to PATH:
echo 'export PATH="${pythonPath}:$PATH"' >> ~/.zshrc && source ~/.zshrc`;
                        } else if (platform === 'win32') {
                            // Windows instructions
                            plainMessage = `Package Installed Successfully
To add to PATH in Windows:
1. Open System Properties > Advanced > Environment Variables
2. Edit the PATH variable and add: ${pythonPath}
3. Restart your terminal or command prompt for changes to take effect`;
                        } else {
                            plainMessage = `Package Installed Successfully
Run this command to add to PATH:
echo 'export PATH="${pythonPath}:$PATH"' >> ~/.bashrc && source ~/.bashrc`;
                        }
                        
                        win.webContents.send('installation-message', plainMessage);
                        return;
                    }
                }
                const isWarning = /warn|warning|notice|deprecated/i.test(errorOutput) &&
                    !/error|fail|exception|fatal/i.test(errorOutput);

                if (isWarning) {
                    console.warn(`[${dep}] warning:`, errorOutput);
                } else {
                    console.error(`[${dep}] error:`, errorOutput);
                    hasErrors = true;
                }

                win.webContents.send('installation-message', {
                    dependency: dep,
                    message: errorOutput,
                    type: isWarning ? 'warning' : 'error'
                });
            });

            process.on('close', (code) => {
                if (externallyManagedError) {
                    console.log(`[${dep}] Detected externally-managed-environment error, retrying with --break-system-packages`);
                    const modifiedCommand = cmd.replace(/pip install/g, 'pip install --break-system-packages');
                    executeCommand(modifiedCommand);
                    return;
                }
                
                if (code === 0 || (!hasErrors && code !== null)) {
                    sendProgress(100, `${prefix}: Installation completed successfully`);
                    resolve();
                } else {
                    const errorMessage = `${dep} installation failed with exit code ${code}. Last output: ${lastOutput}`;
                    win.webContents.send('installation-message', {
                        dependency: dep,
                        message: errorMessage,
                        type: 'error'
                    });
                    reject(new Error(errorMessage));
                }
            });

            process.on('error', (error) => {
                const errorMessage = `Failed to start ${dep} installation: ${error.message}`;
                console.error(`[${dep}] process error:`, errorMessage);
                win.webContents.send('installation-message', {
                    dependency: dep,
                    message: errorMessage,
                    type: 'error'
                });
                reject(new Error(errorMessage));
            });
        };
        executeCommand(currentCommand);
    });
}
function handleProtocolUrl(url) {
    try {
        const urlObj = new URL(url);
        if (urlObj.protocol !== 'mediaharbor:') return;

        const action = urlObj.pathname.slice(1);
        const params = urlObj.searchParams;
        const mediaUrl = params.get('url');
        let platform = params.get('platform');

        if (!mediaUrl) {
            throw new Error('No media URL provided.');
        }

        try {
            new URL(mediaUrl);
        } catch (e) {
            throw new Error('The media URL is not a valid URL.');
        }

        if (!platform) {
            platform = inferPlatformFromUrl(mediaUrl);
            if (!platform) {
                throw new Error('Unable to determine platform from the URL.');
            }
        }
        const mainWindow = BrowserWindow.getAllWindows()[0];
        if (!mainWindow) return;

        mainWindow.webContents.send('protocol-action', {
            url: mediaUrl,
            platform: platform,
            title: platform,
        });
    } catch (error) {
        console.error('Protocol URL handling error:', error);
        dialog.showErrorBox(
            'Protocol Error',
            `Failed to handle protocol URL: ${error.message}`
        );
    }
}
function inferPlatformFromUrl(url) {
    const parsedUrl = new URL(url);
    const hostname = parsedUrl.hostname.toLowerCase();

    if (hostname.includes('youtube.com') || hostname.includes('youtu.be')) {
        return 'youtube';
    } else if (hostname.includes('music.youtube.com')) {
        return 'youtubeMusic';
    } else if (hostname.includes('spotify.com')) {
        return 'spotify';
    } else if (hostname.includes('tidal.com')) {
        return 'tidal';
    } else if (hostname.includes('deezer.com')) {
        return 'deezer';
    } else if (hostname.includes('qobuz.com')) {
        return 'qobuz';
    } else if (hostname.includes('apple.com') || hostname.includes('itunes.apple.com') || hostname.includes('music.apple.com') || hostname.includes('music.*.apple.com')) {
        return 'applemusic';
    } else {
        return null;
    }
}
const { app, BrowserWindow, ipcMain } = require('electron');
const { shell } = require('electron');
const path = require('path');
const fs = require('fs');
const { spawn } = require('child_process');
const os = require('os');
const TOML = require('@iarna/toml');
const axios = require('axios');
const cheerio = require('cheerio');
let downloadCount = 0;
const settingsFilePath = path.join(app.getPath('userData'), 'mh-settings.json');
const sqlite3 = require('sqlite3').verbose();

function loadSettings() {
    try {
        const settingsData = fs.readFileSync(settingsFilePath, 'utf8');
        return JSON.parse(settingsData);
    } catch (err) {
        console.log('No user settings found, using default settings.');
        return getDefaultSettings(); // Fall back to default settings
    }
}

const userSettings = loadSettings();
const dbPath = userSettings.downloadsDatabasePath || path.join(app.getPath('userData'), 'downloads_database.db');


// Open or create the SQLite database
let db = new sqlite3.Database(dbPath, (err) => {
    if (err) {
        console.error('Error opening the database:', err.message);
    } else {
        console.log('Connected to the SQLite database.');
        // Create the table if it doesn't exist
        db.run(`CREATE TABLE IF NOT EXISTS downloads (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            downloadName TEXT,
            downloadArtistOrUploader TEXT,
            downloadLocation TEXT,
            downloadThumbnail TEXT
        )`);
    }
});
function saveDownloadToDatabase(downloadInfo) {
    const sql = `INSERT INTO downloads (downloadName, downloadArtistOrUploader, downloadLocation, downloadThumbnail) 
                 VALUES (?, ?, ?, ?)`;

    db.run(sql, [downloadInfo.downloadName, downloadInfo.downloadArtistOrUploader, downloadInfo.downloadLocation, downloadInfo.downloadThumbnail], function (err) {
        if (err) {
            return console.error('Error saving download to database:', err.message);
        }
        console.log(`A row has been inserted with rowid ${this.lastID}`);
    });
}

function loadDownloadsFromDatabase(callback) {
    const sql = `SELECT * FROM downloads`;

    db.all(sql, [], (err, rows) => {
        if (err) {
            console.error('Error loading downloads from database:', err.message);
            callback([]);  // Return empty array on error
            return;
        }
        callback(rows);
    });
}
function getDownloads() {
    return new Promise((resolve, reject) => {
        loadDownloadsFromDatabase((rows) => {
            resolve(rows);
        });
    });
}
async function fetchWebsiteTitle(url) {
    try {
        const response = await axios.get(url, {
            headers: {
                'Cache-Control': 'no-cache',
                'Pragma': 'no-cache',
                'Expires': '0'
            }
        });
        const html = response.data;
        const $ = cheerio.load(html);

        // Fetch the title from the <title> tag only
        let title = $('title').text().trim();

        // Trim the title if it's too long
        title = title.length > 50 ? title.slice(0, 50)+'…' : title;

        return title;
    } catch (error) {
        console.error('Error fetching title:', error);
        return 'Unknown Title';  // Fallback if there's an error
    }
}

function buildYtDlpMusicArgs(url, quality, settings) {
    const downloadPath = settings.downloadLocation || path.join(os.homedir(), 'Downloads');
    const args = [
        '-x',  // Extract audio
        '--audio-format', settings.youtubeAudioExtensions || 'mp3',
        '--audio-quality', quality,
        '--output', path.join(downloadPath, settings.download_output_template || '%(title)s.%(ext)s'),
    ];

    // Add settings-based arguments
    if (settings.no_playlist) {
        args.push('--no-playlist');
    }

    if (settings.max_retries) {
        args.push('--retries', settings.max_retries.toString());
    }

    if (settings.continue) {
        args.push('--continue');
    }

    if (!settings.continue) {
        args.push('--no-continue');
    }

    if (settings.download_speed_limit && settings.speed_limit_value > 0) {
        args.push('-r', `${settings.speed_limit_value}${settings.speed_limit_type}`);
    }

    if (settings.use_aria2) {
        args.push('--downloader', 'aria2c');
    }

    if (settings.use_proxy && settings.proxy_url) {
        args.push('--proxy', settings.proxy_url);
    }

    if (settings.use_authentication && settings.username && settings.password) {
        args.push('--username', settings.username);
        args.push('--password', settings.password);
    }

    if (settings.use_cookies) {
        if (settings.cookies) {
            args.push('--cookies', settings.cookies);
        } else if (settings.cookies_from_browser) {
            args.push('--cookies-from-browser', settings.cookies_from_browser);
        }
    }

    if (settings.add_metadata) {
        args.push('--embed-thumbnail');
        args.push('--add-metadata');
    }

    args.push(url);
    return args;
}

function handleYtDlpMusicDownload(event, data, settings) {
    const { url, quality } = data;
    const ytDlpCommand = 'yt-dlp';

    // Get video info first
    const videoInfoArgs = [
        '--print', '%(title)s',
        '--print', '%(uploader)s',
        '--print', '%(thumbnail)s',
        url
    ];

    const videoInfoProcess = spawn(ytDlpCommand, videoInfoArgs);
    let videoInfo = { title: '', uploader: '', thumbnail: '' };
    let outputLines = [];

    videoInfoProcess.stdout.on('data', (data) => {
        const output = data.toString().split('\n').filter(line => line.trim());
        outputLines = outputLines.concat(output);

        if (outputLines.length >= 3) {
            videoInfo.title = outputLines[0].trim();
            videoInfo.uploader = outputLines[1].trim();
            videoInfo.thumbnail = outputLines[2].trim();

            if (!videoInfo.thumbnail.startsWith('http')) {
                videoInfo.thumbnail = 'https:' + videoInfo.thumbnail;
            }

            downloadCount++;
            event.reply('youtube-music-info', {
                title: videoInfo.title,
                uploader: videoInfo.uploader,
                thumbnail: videoInfo.thumbnail,
                order: downloadCount
            });

            outputLines = [];
        }
    });

    videoInfoProcess.on('exit', () => {
        startMusicDownload(event, url, quality, settings, videoInfo);
    });
}

function startMusicDownload(event, url, quality, settings, videoInfo) {
    console.log(quality)
    const ytDlpCommand = 'yt-dlp';
    const args = buildYtDlpMusicArgs(url, quality, settings);

    const ytDlp = spawn(ytDlpCommand, args);

    ytDlp.stdout.on('data', (data) => {
        const output = data.toString();
        console.log(output);
        const progressMatch = output.match(/(\d+\.\d+)%/);
        if (progressMatch) {
            const progress = parseFloat(progressMatch[1]);
            event.reply('download-update', {
                progress,
                title: videoInfo.title,
                uploader: videoInfo.uploader,
                thumbnail: videoInfo.thumbnail,
                order: downloadCount
            });
        }
    });

    ytDlp.stderr.on('data', (errorData) => {
        const errorOutput = errorData.toString();
        console.error(`Error: ${errorOutput}`);
        event.reply('download-error', `Error: ${errorOutput}`);
    });

    ytDlp.on('exit', (code) => {
        if (code === 0) {
            const downloadInfo = {
                downloadName: videoInfo.title,
                downloadArtistOrUploader: videoInfo.uploader,
                downloadLocation: settings.downloadLocation || app.getPath('downloads'),
                downloadThumbnail: videoInfo.thumbnail
            };
            saveDownloadToDatabase(downloadInfo);
            event.reply('download-complete', { order: downloadCount });
        } else {
            event.reply('download-error', `Process exited with code ${code}`);
        }
    });
}

function buildYtDlpArgs(url, quality, settings, isGeneric = false) {
    const downloadPath = settings.downloadLocation || path.join(os.homedir(), 'Downloads');
    const args = [
        '-f', quality,
        '--output', path.join(downloadPath, settings.download_output_template || '%(title)s.%(ext)s'),
    ];

    // Add settings-based arguments
    if (!isGeneric) {
        if (settings.youtubeVideoExtensions && settings.yt_override_download_extension) {
            args.push('--merge-output-format', settings.youtubeVideoExtensions);
        }
    }

    if (settings.no_playlist || isGeneric) {
        args.push('--no-playlist');
    }

    if (settings.max_retries) {
        args.push('--retries', settings.max_retries.toString());
    }

    if (settings.continue) {
        args.push('--continue');
    }

    if (!settings.continue) {
        args.push('--no-continue');
    }

    if (settings.download_speed_limit && settings.speed_limit_value > 0) {
        args.push('-r', `${settings.speed_limit_value}${settings.speed_limit_type}`);
    }

    if (settings.use_aria2) {
        args.push('--downloader', 'aria2c');
    }

    if (settings.use_proxy && settings.proxy_url) {
        args.push('--proxy', settings.proxy_url);
    }

    if (settings.use_authentication && settings.username && settings.password) {
        args.push('--username', settings.username);
        args.push('--password', settings.password);
    }

    if (!settings.no_sponsorblock) {
        if (settings.sponsorblock_mark) {
            args.push('--sponsorblock-mark', settings.sponsorblock_mark);
        }
        if (settings.sponsorblock_remove) {
            args.push('--sponsorblock-remove', settings.sponsorblock_remove);
        }
        if (settings.sponsorblock_chapter_title) {
            args.push('--sponsorblock-chapter-title', settings.sponsorblock_chapter_title);
        }
        if (settings.sponsorblock_api_url) {
            args.push('--sponsorblock-api', settings.sponsorblock_api_url);
        }
    }

    if (settings.use_cookies) {
        if (settings.cookies) {
            args.push('--cookies', settings.cookies);
        } else if (settings.cookies_from_browser) {
            args.push('--cookies-from-browser', settings.cookies_from_browser);
        }
    }

    if (settings.add_metadata) {
        args.push('--write-thumbnail');
        args.push('--embed-thumbnail');
        args.push('--add-metadata');
    }

    if (settings.embed_chapters) {
        args.push('--embed-chapters');
    }

    if (settings.add_subtitle_to_file) {
        args.push('--embed-subs');
        args.push('--sub-langs', 'all');
    }

    args.push(url);
    return args;
}

function handleYtDlpDownload(event, data, settings, isGeneric = false) {
    const { url, quality } = data;
    const ytDlpCommand = 'yt-dlp';

    if (!isGeneric) {
        // Get video info first for YouTube videos
        const videoInfoArgs = [
            '--print', '%(title)s',
            '--print', '%(uploader)s',
            '--print', '%(thumbnail)s',
            url
        ];

        const videoInfoProcess = spawn(ytDlpCommand, videoInfoArgs);
        let videoInfo = { title: '', uploader: '', thumbnail: '' };
        let outputLines = [];

        videoInfoProcess.stdout.on('data', (data) => {
            const output = data.toString().split('\n').filter(line => line.trim());
            outputLines = outputLines.concat(output);

            if (outputLines.length >= 3) {
                videoInfo.title = outputLines[0].trim();
                videoInfo.uploader = outputLines[1].trim();
                videoInfo.thumbnail = outputLines[2].trim();

                if (!videoInfo.thumbnail.startsWith('http')) {
                    videoInfo.thumbnail = 'https:' + videoInfo.thumbnail;
                }

                downloadCount++;
                event.reply('youtube-video-info', {
                    title: videoInfo.title,
                    uploader: videoInfo.uploader,
                    thumbnail: videoInfo.thumbnail,
                    order: downloadCount
                });

                outputLines = [];
            }
        });

        videoInfoProcess.on('exit', () => {
            startDownload(event, url, quality, settings, videoInfo, isGeneric);
        });
    } else {
        // For generic downloads, start immediately
        startDownload(event, url, quality, settings, null, isGeneric);
    }
}

function extractDomain(url) {
    try {
        const domain = new URL(url).hostname;
        return domain.startsWith('www.') ? domain.slice(4) : domain;  // Remove 'www.' if present
    } catch (error) {
        console.error('Invalid URL:', error);
        return url;  // Fallback to the full URL if invalid
    }
}
async function fetchHighResImageOrFavicon(url) {
    try {
        const response = await axios.get(url);
        const html = response.data;
        const $ = cheerio.load(html);

        // Look for Open Graph image first
        let ogImage = $('meta[property="og:image"]').attr('content');

        // If no OG image, fallback to high-res favicons or Apple touch icons
        let favicon = $('link[rel="apple-touch-icon"]').attr('href') ||
            $('link[rel="icon"][sizes]').attr('href') ||  // Look for any icon with sizes attribute
            $('link[rel="icon"]').attr('href') ||  // Fallback to normal favicon
            '/favicon.ico';  // Fallback to default favicon

        // If we found an OG image, return that
        let image = ogImage || favicon;

        // If the image URL is relative, make it absolute by combining with base URL
        if (!image.startsWith('http')) {
            const baseUrl = new URL(url).origin;
            image = `${baseUrl}${image}`;
        }

        return image;
    } catch (error) {
        console.error('Error fetching image:', error);
        return '/favicon.ico';  // Fallback to generic favicon
    }
}


async function startDownload(event, url, quality, settings, videoInfo = null, isGeneric = false) {
    const ytDlpCommand = 'yt-dlp';
    const args = buildYtDlpArgs(url, quality, settings, isGeneric);
    downloadCount++;

    // Fetch metadata once at the start
    const metadata = {
        title: videoInfo ? videoInfo.title : await fetchWebsiteTitle(url),
        uploader: videoInfo ? videoInfo.uploader : extractDomain(url),
        thumbnail: videoInfo ? videoInfo.thumbnail : await fetchHighResImageOrFavicon(url),
        domain: extractDomain(url)
    };

    // Use fallback values if any fetches failed
    metadata.title = metadata.title || 'Unknown';
    metadata.uploader = metadata.uploader || 'Unknown';
    metadata.thumbnail = metadata.thumbnail || 'Unknown';

    if (isGeneric) {
        event.reply('generic-video-info', {
            url,
            order: downloadCount,
            ...metadata
        });
    }

    const ytDlp = spawn(ytDlpCommand, args);

    ytDlp.stdout.on('data', (data) => {
        const output = data.toString();
        console.log(output);
        const progressMatch = output.match(/(\d+\.\d+)%/);
        if (progressMatch) {
            const progress = parseFloat(progressMatch[1]);
            event.reply('download-update', {
                progress,
                order: downloadCount,
                ...metadata
            });
        }
    });

    ytDlp.stderr.on('data', (errorData) => {
        const errorOutput = errorData.toString();
        console.error(`Error: ${errorOutput}`);
        event.reply('download-error', `Error: ${errorOutput}`);
    });

    ytDlp.on('exit', (code) => {
        if (code !== 0) {
            event.reply('download-error', `Process exited with code ${code}`);
        } else {
            const downloadInfo = {
                downloadName: metadata.title,
                downloadArtistOrUploader: metadata.uploader,
                downloadLocation: settings.downloadLocation || app.getPath('downloads'),
                downloadThumbnail: metadata.thumbnail,
                downloadDomain: metadata.domain
            };

            saveDownloadToDatabase(downloadInfo);
            event.reply('download-complete', {
                order: downloadCount
            });
        }
    });
}
function closeDatabase() {
    return new Promise((resolve, reject) => {
        db.close((err) => {
            if (err) {
                reject(err);
            } else {
                console.log('Database connection closed.');
                resolve();
            }
        });
    });
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
    return new Promise((resolve, reject) => {
        const sql = 'DELETE FROM downloads WHERE id = ?';
        db.run(sql, [id], (err) => {
            if (err) {
                console.error('Error deleting download:', err);
                reject(err);
                return;
            }
            resolve();
        });
    });
});

ipcMain.handle('showItemInFolder', async (event, filePath) => {
    try {
        // Normalize the path to handle any path format issues
        const normalizedPath = path.normalize(filePath);

        // Check if path exists
        if (fs.existsSync(normalizedPath)) {
            // If it's a directory, show the folder
            if (fs.statSync(normalizedPath).isDirectory()) {
                // On Windows, we can use explorer.exe to open the folder
                if (process.platform === 'win32') {
                    require('child_process').exec(`explorer "${normalizedPath}"`);
                } else {
                    shell.openPath(normalizedPath);
                }
            } else {
                // If it's a file, show it in the folder
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
    console.log(userSettings.downloadsDatabasePath)
    try {
        await closeDatabase(); // Close the database connection first

        if (fs.existsSync(userSettings.downloadsDatabasePath)) {
            fs.unlinkSync(userSettings.downloadsDatabasePath);
            let db = new sqlite3.Database(dbPath, (err) => {
                if (err) {
                    console.error('Error opening the database:', err.message);
                } else {
                    console.log('Connected to the SQLite database.');
                    // Create the table if it doesn't exist
                    db.run(`CREATE TABLE IF NOT EXISTS downloads (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            downloadName TEXT,
            downloadArtistOrUploader TEXT,
            downloadLocation TEXT,
            downloadThumbnail TEXT
        )`);
                }
            });
            return { success: true };
        } else {
            return { success: false, message: 'File not found' };
        }
    } catch (error) {
        return { success: false, message: error.message };
    }

});

function createWindow() {
    const win = new BrowserWindow({
        width: 1200,
        height: 800,
        frame: true,
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            preload: path.join(__dirname, 'preload.js')
        }
    });

    win.loadFile('index.html');
    ipcMain.on('minimize-window', () => {
        win.minimize();
    });
    ipcMain.on('maximize-window', () => {
        if (win.isMaximized()) {
            win.unmaximize();
        } else {
            win.maximize();
        }
    });

    ipcMain.on('close-window', () => {
        win.close();
    });
    const downloadPath = path.join(os.homedir(), 'Downloads');


    function getStreamripPaths(callback) {
        const streamripProcess = spawn('custom_rip', ['config', 'path']);
        let stdout = '';
        let stderr = '';

        streamripProcess.stdout.on('data', (data) => {
            stdout += data.toString();
        });

        streamripProcess.stderr.on('data', (data) => {
            stderr += data.toString();
        });

        streamripProcess.on('close', (code) => {
            if (code !== 0) {
                console.error(`Error getting custom_rip config path: ${stderr}`);
                callback(null);
                return;
            }

            const configPathMatch = stdout.match(/Config path: '(.+)'/);
            if (configPathMatch && configPathMatch[1]) {
                const configPath = configPathMatch[1];
                const configDir = path.dirname(configPath);

                // Construct database paths
                const paths = {
                    configPath: configPath,
                    downloadsDbPath: path.join(configDir, 'downloads.db'),
                    failedDownloadsDbPath: path.join(configDir, 'failed_downloads.db')
                };

                callback(paths);
            } else {
                console.error('Could not find config path in output:', stdout);
                callback(null);
            }
        });
    }

// Update the load-settings handler
    ipcMain.on('load-settings', (event) => {
        fs.readFile(settingsFilePath, 'utf8', (err, data) => {
            let settings;

            if (err) {
                console.log('Error loading settings, using defaults:', err);
                settings = getDefaultSettings();
            } else {
                try {
                    settings = JSON.parse(data);
                } catch (parseErr) {
                    console.log('Error parsing settings JSON, using defaults:', parseErr);
                    settings = getDefaultSettings();
                }
            }

            // Get streamrip paths and update settings
            getStreamripPaths((paths) => {
                if (paths) {
                    // Read the streamrip config.toml
                    fs.readFile(paths.configPath, 'utf8', (tomlErr, tomlData) => {
                        if (!tomlErr) {
                            try {
                                const streamripConfig = TOML.parse(tomlData);

                                // Update settings with streamrip config values
                                settings.downloads_database_path = paths.downloadsDbPath;
                                settings.failed_downloads_database_path = paths.failedDownloadsDbPath;

                                // Map streamrip config to settings
                                settings.downloadLocation = streamripConfig.downloads?.folder || settings.downloadLocation;
                                settings.source_subdirectories = streamripConfig.downloads?.source_subdirectories || false;
                                settings.disc_subdirectories = streamripConfig.downloads?.disc_subdirectories || true;
                                settings.max_connections = streamripConfig.downloads?.max_connections || 6;
                                settings.concurrency = streamripConfig.downloads?.concurrency || true;
                                // Qobuz settings
                                settings.qobuz_download_booklets = streamripConfig.qobuz?.download_booklets || true;
                                settings.qobuz_token_or_email = streamripConfig.qobuz?.use_auth_token || true;
                                settings.qobuz_email_or_userid = streamripConfig.qobuz?.email_or_userid || "";
                                settings.qobuz_password_or_token = streamripConfig.qobuz?.password_or_token || "";

                                // Tidal settings
                                settings.tidal_download_videos = streamripConfig.tidal?.download_videos || true;

                                // Deezer settings
                                settings.deezer_use_deezloader = streamripConfig.deezer?.use_deezloader;
                                settings.deezer_arl = streamripConfig.deezer?.arl || '';

                                // Database settings
                                settings.downloads_database_check = streamripConfig.database?.downloads_enabled || true;
                                settings.failed_downloads_database_check = streamripConfig.database?.failed_downloads_enabled || true;
                                settings.downloads_database = streamripConfig.database?.downloads_path;
                                settings.failed_downloads_database = streamripConfig.database?.failed_downloads_path;
                                // Conversion settings
                                settings.conversion_check = streamripConfig.conversion?.enabled || false;
                                settings.conversion_codec = streamripConfig.conversion?.codec || "ALAC";
                                settings.conversion_sampling_rate = streamripConfig.conversion?.sampling_rate || 48000;
                                // MetaData settings
                                settings.meta_album_name_playlist_check = streamripConfig.metadata?.set_playlist_to_album || true;
                                settings.meta_album_order_playlist_check = streamripConfig.metadata?.renumber_playlist_tracks || true;
                                settings.excluded_tags = streamripConfig.metadata?.exclude || [];

                                // Save the updated settings
                                fs.writeFile(settingsFilePath, JSON.stringify(settings), (writeErr) => {
                                    if (writeErr) console.error('Error saving merged settings:', writeErr);
                                });
                            } catch (tomlParseErr) {
                                console.error('Error parsing streamrip config:', tomlParseErr);
                            }
                        }

                        // Send settings to renderer regardless of TOML parsing success
                        event.reply('settings-data', settings);
                    });
                } else {
                    // If we couldn't get streamrip paths, just send the current settings
                    event.reply('settings-data', settings);
                }
            });
        });
    });

// Update the save-settings handler
// Update the save-settings handler
    ipcMain.on('save-settings', async (event, settings) => {
        // First save to electron settings file
        try {
            await fs.promises.writeFile(settingsFilePath, JSON.stringify(settings));
        } catch (err) {
            console.error('Error saving settings:', err);
            event.reply('settings-error', 'Failed to save application settings');
            return;
        }

        // Get streamrip config path
        getStreamripPaths(async (paths) => {
            if (!paths?.configPath) {
                console.error('Could not get streamrip config path');
                event.reply('settings-error', 'Could not locate streamrip configuration');
                return;
            }

            try {
                // Read existing streamrip config
                const data = await fs.promises.readFile(paths.configPath, 'utf8');
                let config;

                try {
                    config = TOML.parse(data);
                } catch (parseErr) {
                    console.error('Error parsing existing TOML:', parseErr);
                    config = {}; // Start fresh if parsing fails
                }

                // Ensure all necessary objects exist
                config.downloads = config.downloads || {};
                config.qobuz = config.qobuz || {};
                config.database = config.database || {};
                config.conversion = config.conversion || {};

                // Update downloads section
                config.downloads = {
                    ...config.downloads,
                    folder: settings.downloadLocation,
                    source_subdirectories: settings.createPlatformSubfolders,
                    disc_subdirectories: settings.disc_subdirectories,
                    concurrency: settings.concurrency,
                    max_connections: parseInt(settings.max_connections),
                    requests_per_minute: parseInt(settings.requests_per_minute)
                };

                // Update qobuz section
                config.qobuz = {
                    ...config.qobuz,
                    download_booklets: settings.qobuz_download_booklets,
                    use_auth_token: settings.qobuz_token_or_email,
                    email_or_userid: settings.qobuz_email_or_userid,
                    password_or_token: settings.qobuz_password_or_token
                };
                // Update tidal section
                config.tidal = {
                    ...config.tidal,
                    download_videos: settings.tidal_download_videos
                }
                // Update deezer section
                config.deezer = {
                    ...config.deezer,
                    use_deezloader: settings.deezer_use_deezloader,
                    arl: settings.deezer_arl
                }
                // Update database section
                config.database = {
                    ...config.database,
                    downloads_enabled: settings.downloads_database_check,
                    downloads_path: settings.downloads_database_path,
                    failed_downloads_enabled: settings.failed_downloads_database_check,
                    failed_downloads_path: settings.failed_downloads_database
                };

                // Update conversion section
                config.conversion = {
                    ...config.conversion,
                    enabled: settings.conversion_check,
                    codec: settings.conversion_codec,
                    sampling_rate: parseInt(settings.conversion_sampling_rate),
                    bit_depth: parseInt(settings.conversion_bit_depth || 16)
                };
                config.metadata = {
                    ...config.metadata,
                    set_playlist_to_album: settings.meta_album_name_playlist_check,
                    renumber_playlist_tracks: settings.meta_album_order_playlist_check,
                    exclude: Array.isArray(settings.excluded_tags)
                        ? settings.excluded_tags
                        : settings.excluded_tags.trim() === ""
                            ? [] // Ensures an empty array when input is empty
                            : settings.excluded_tags.split(/\s+/)
                }

                // Convert config to TOML string
                const tomlString = TOML.stringify(config);

                // Write the updated config back to file
                try {
                    await fs.promises.writeFile(paths.configPath, tomlString, 'utf8');
                    event.reply('settings-saved', 'Settings saved successfully');
                } catch (writeErr) {
                    console.error('Error writing streamrip config:', writeErr);
                    event.reply('settings-error', 'Failed to save streamrip configuration');
                }
            } catch (readErr) {
                console.error('Error reading streamrip config:', readErr);
                event.reply('settings-error', 'Failed to read existing streamrip configuration');
            }
        });
    });
    ipcMain.on('start-yt-music-download', (event, data) => {
        fs.readFile(settingsFilePath, 'utf8', (err, settingsData) => {
            const settings = err ? getDefaultSettings() : JSON.parse(settingsData);
            handleYtDlpMusicDownload(event, data, settings);
        });
    });
    ipcMain.on('start-download', (event, data) => {
        const { service, url, quality } = data;
        let command, args;

        switch(service) {
            case 'youtube':
                command = 'yt-dlp';
                args = [
                    '-f', quality,
                    '--output', path.join(downloadPath, '%(title)s.%(ext)s'),
                    '--no-playlist',
                    '--progress',
                    url
                ];
                break;
            case 'qobuz':
            case 'tidal':
            case 'deezer':
                command = 'custom_rip';
                args = [
                    '--quality', quality,
                    'url',
                    url
                ];
                break;
            default:
                event.reply('download-error', 'Unsupported service');
                return;
        }

        downloadCount++;
        event.reply('download-info', {
            title: `Download from ${service}`,
            service: service,
            order: downloadCount
        });

        const process = spawn(command, args);

        process.stdout.on('data', (data) => {
            const output = data.toString();
            console.log(output);
            const progressMatch = output.match(/(\d+)\.?\d*%/);
            if (progressMatch) {
                const progress = parseFloat(progressMatch[1]);
                event.reply('download-update', {
                    progress,
                    order: downloadCount
                });
            }
        });

        process.stderr.on('data', (errorData) => {
            const errorOutput = errorData.toString();
            console.error(`Error: ${errorOutput}`);
            event.reply('download-error', `Error: ${errorOutput}`);
        });

        process.on('exit', (code) => {
            if (code !== 0) {
                event.reply('download-error', `Process exited with code ${code}`);
            } else {
                event.reply('download-complete', {
                    order: downloadCount
                });
            }
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

    ipcMain.on('start-streamrip', (event, command) => {
        const process = spawn('custom_rip', command.split(' '));

        process.stdout.on('data', (data) => {
            const output = data.toString();
            console.log(output);
            event.reply('streamrip-output', output);
        });

        process.stderr.on('data', (errorData) => {
            const errorOutput = errorData.toString();
            console.error(`Error: ${errorOutput}`);
            event.reply('streamrip-error', `Error: ${errorOutput}`);
        });

        process.on('exit', (code) => {
            if (code !== 0) {
                event.reply('streamrip-error', `Process exited with code ${code}`);
            } else {
                event.reply('streamrip-complete');
            }
        });
    });

    // Qobuz, Deezer, Tidal Downloads

    ipcMain.on('start-qobuz-download', (event, data) => {
        const { url, quality } = data;
        const ripCommand = 'custom_rip';
        const ripArgs = ['-q', quality, 'url', url];

        downloadCount++;
        event.reply('download-info', {
            title: 'Qobuz Download',
            order: downloadCount
        });

        const ripProcess = spawn(ripCommand, ripArgs);
        let currentEid = null;

        ripProcess.stdout.on('data', (data) => {
            const output = data.toString();
            console.log(output);

            const eidMatch = output.match(/track-id=(\d+)/);
            if (eidMatch && eidMatch[1] !== currentEid) {
                currentEid = eidMatch[1];
                getQobuzDetails(currentEid, event, downloadCount);
            }

            const progressMatch = output.match(/(\d+\.\d+)%/);
            if (progressMatch) {
                const progress = parseFloat(progressMatch[1]);
                event.reply('download-update', {
                    progress,
                    order: downloadCount
                });
            }
        });

        ripProcess.stderr.on('data', (errorData) => {
            const errorOutput = errorData.toString();
            console.error(`Error: ${errorOutput}`);
            event.reply('download-error', `Error: ${errorOutput}`);
        });

        ripProcess.on('exit', (code) => {
            if (code === 0) {
                // Get the settings for download location
                fs.readFile(settingsFilePath, 'utf8', (err, settingsData) => {
                    const settings = err ? getDefaultSettings() : JSON.parse(settingsData);
                    const downloadLocation = settings.downloadLocation || app.getPath('downloads');

                    // Save to database only if we have track details
                    if (currentEid) {
                        getQobuzDetails(currentEid, {
                            reply: (channel, details) => {
                                if (channel === 'qobuz-details') {
                                    const downloadInfo = {
                                        downloadName: details.title,
                                        downloadArtistOrUploader: details.album.artist.name,
                                        downloadLocation: downloadLocation,
                                        downloadThumbnail: details.album.image.small,
                                        service: 'qobuz'
                                    };
                                    saveDownloadToDatabase(downloadInfo);
                                }
                            }
                        }, downloadCount);
                    }
                });
                event.reply('download-complete', { order: downloadCount });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
    });

// Modified Deezer download handler
    ipcMain.on('start-deezer-download', (event, data) => {
        const { url, quality } = data;
        const ripCommand = 'custom_rip';
        const ripArgs = ['-q', quality, 'url', url];

        downloadCount++;
        event.reply('download-info', {
            title: 'Deezer Download',
            order: downloadCount
        });

        let trackId = null;
        const deezerIdMatch = url.match(/\/track\/(\d+)/);
        if (deezerIdMatch) {
            trackId = deezerIdMatch[1];
            getDeezerDetails(trackId, event, downloadCount);
        }

        const ripProcess = spawn(ripCommand, ripArgs);

        ripProcess.stdout.on('data', (data) => {
            const output = data.toString();
            console.log(output);
            const progressMatch = output.match(/(\d+\.\d+)%/);
            if (progressMatch) {
                const progress = parseFloat(progressMatch[1]);
                event.reply('download-update', {
                    progress,
                    order: downloadCount
                });
            }
        });

        ripProcess.stderr.on('data', (errorData) => {
            const errorOutput = errorData.toString();
            console.error(`Error: ${errorOutput}`);
            event.reply('download-error', `Error: ${errorOutput}`);
        });

        ripProcess.on('exit', (code) => {
            if (code === 0) {
                fs.readFile(settingsFilePath, 'utf8', (err, settingsData) => {
                    const settings = err ? getDefaultSettings() : JSON.parse(settingsData);
                    const downloadLocation = settings.downloadLocation || app.getPath('downloads');

                    if (trackId) {
                        getDeezerDetails(trackId, {
                            reply: (channel, details) => {
                                if (channel === 'deezer-details') {
                                    const downloadInfo = {
                                        downloadName: details.title,
                                        downloadArtistOrUploader: details.artist.name,
                                        downloadLocation: downloadLocation,
                                        downloadThumbnail: details.album.cover_medium,
                                        service: 'deezer'
                                    };
                                    saveDownloadToDatabase(downloadInfo);
                                }
                            }
                        }, downloadCount);
                    }
                });
                event.reply('download-complete', { order: downloadCount });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
    });

// Modified Tidal download handler
    ipcMain.on('start-tidal-download', (event, data) => {
        const { url, quality } = data;
        const ripCommand = 'custom_rip';
        const ripArgs = ['-q', quality, 'url', url];

        downloadCount++;
        event.reply('download-info', {
            title: 'Tidal Download',
            order: downloadCount
        });

        let currentTrackId = null;
        const tidalIdMatch = url.match(/\/track\/(\d+)/);
        if (tidalIdMatch) {
            currentTrackId = tidalIdMatch[1];
            getTidalDetails(currentTrackId, event, downloadCount);
        }

        const ripProcess = spawn(ripCommand, ripArgs);

        ripProcess.stdout.on('data', (data) => {
            const output = data.toString();
            console.log(output);

            const trackIdMatch = output.match(/track-id=(\d+)/);
            if (trackIdMatch && trackIdMatch[1] !== currentTrackId) {
                currentTrackId = trackIdMatch[1];
                getTidalDetails(currentTrackId, event, downloadCount);
            }

            const progressMatch = output.match(/(\d+\.\d+)%/);
            if (progressMatch) {
                const progress = parseFloat(progressMatch[1]);
                event.reply('download-update', {
                    progress,
                    order: downloadCount
                });
            }
        });

        ripProcess.stderr.on('data', (errorData) => {
            const errorOutput = errorData.toString();
            console.error(`Error: ${errorOutput}`);
            event.reply('download-error', `Error: ${errorOutput}`);
        });

        ripProcess.on('exit', (code) => {
            if (code === 0) {
                fs.readFile(settingsFilePath, 'utf8', (err, settingsData) => {
                    const settings = err ? getDefaultSettings() : JSON.parse(settingsData);
                    const downloadLocation = settings.downloadLocation || app.getPath('downloads');

                    if (currentTrackId) {
                        getTidalDetails(currentTrackId, {
                            reply: (channel, details) => {
                                if (channel === 'tidal-details') {
                                    const downloadInfo = {
                                        downloadName: details.title,
                                        downloadArtistOrUploader: details.artist,
                                        downloadLocation: downloadLocation,
                                        downloadThumbnail: details.thumbnail,
                                        service: 'tidal'
                                    };
                                    saveDownloadToDatabase(downloadInfo);
                                }
                            }
                        }, downloadCount);
                    }
                });
                event.reply('download-complete', { order: downloadCount });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
    });

}

function getTidalDetails(trackId, event, downloadCount) {
    const pythonProcess = spawn('py', ['tidalapi.py', '--get-track', trackId, '--country-code', 'US']);
    let output = '';

    pythonProcess.stdout.on('data', (data) => {
        output += data.toString();
    });

    pythonProcess.stderr.on('data', (data) => {
        console.error(`Error: ${data}`);
    });

    pythonProcess.on('close', (code) => {
        if (code === 0) {
            try {
                const details = JSON.parse(output);
                const trackDetails = {
                    title: details.data.attributes.title,
                    artist: details.included[0].attributes.name,
                    quality: details.data.attributes.mediaTags[0],
                    thumbnail: details.included[1].attributes.imageLinks[0].href
                };
                event.reply('tidal-details', {
                    order: downloadCount,
                    ...trackDetails
                });
            } catch (error) {
                console.error('Error parsing Tidal details:', error);
            }
        } else {
            console.error(`Python script exited with code ${code}`);
        }
    });
}


function getDeezerDetails(trackId, event, downloadCount) {
    const pythonProcess = spawn('py', ['deezerapi.py', '--get-details', trackId]);
    let output = '';

    pythonProcess.stdout.on('data', (data) => {
        output += data.toString();
    });

    pythonProcess.stderr.on('data', (data) => {
        console.error(`Error: ${data}`);
    });

    pythonProcess.on('close', (code) => {
        if (code === 0) {
            try {
                const details = JSON.parse(output);
                event.reply('deezer-details', {
                    order: downloadCount,
                    ...details
                });
            } catch (error) {
                console.error('Error parsing Deezer details:', error);
            }
        } else {
            console.error(`Python script exited with code ${code}`);
        }
    });
}

function getQobuzDetails(eid, event, downloadCount) {
    const pythonProcess = spawn('py', ['qobuzapi.py', '--get-details', eid]);
    let output = '';

    pythonProcess.stdout.on('data', (data) => {
        output += data.toString();
    });

    pythonProcess.stderr.on('data', (data) => {
        console.error(`Error: ${data}`);
    });

    pythonProcess.on('close', (code) => {
        if (code === 0) {
            try {
                const details = JSON.parse(output);
                event.reply('qobuz-details', {
                    order: downloadCount,
                    album: {
                        maximum_bit_depth: details.album.maximum_bit_depth,
                        image: {
                            small: details.album.image.small
                        },
                        artist: {
                            name: details.album.artist.name
                        },
                        maximum_sampling_rate: details.album.maximum_sampling_rate
                    },
                    title: details.title
                });
            } catch (error) {
                console.error('Error parsing Qobuz details:', error);
            }
        } else {
            console.error(`Python script exited with code ${code}`);
        }
    });
}

function getDefaultSettings() {
    return {
        theme: 'auto',
        downloadsDatabasePath: path.join(app.getPath('home'), 'MH', 'database.db'),
        downloadLocation: app.getPath('downloads'),
        createPlatformSubfolders: false,
        orpheusDL: false,
        streamrip: true,
        use_cookies: false,
        cookies: "",
        cookies_from_browser: "",
        override_download_extension: false,
        yt_override_download_extension: false,
        ytm_override_download_extension: false,
        youtubeVideoExtensions: "mp4",
        youtubeAudioExtensions: "mp3",
        use_aria2: false,
        auto_update: true,
        max_downloads: 0,
        download_speed_limit: false,
        speed_limit_type: M,
        speed_limit_value: 0,
        max_retries: 5,
        download_output_template: "%(title)s.%(ext)s",
        continue: true,
        add_metadata: false,
        embed_chapters: false,
        add_subtitle_to_file: false,
        use_proxy: false,
        proxy_url: "",
        use_authentication: false,
        username: "",
        password: "",
        sponsorblock_mark: "all",
        sponsorblock_remove: "",
        sponsorblock_chapter_title: "[SponsorBlock]: %(category_names)l",
        no_sponsorblock: false,
        sponsorblock_api_url: "https://sponsor.ajay.app",

        disc_subdirectories: true,
        concurrency: true,
        max_connections: 6,
        requests_per_minute: 60,
        qobuz_download_booklets: true,
        qobuz_token_or_email: false,
        qobuz_email_or_userid: "",
        qobuz_password_or_token: "",
        tidal_download_videos: false,
        deezer_use_deezloader: true,
        deezer_arl: "",
        downloads_database_check: false,
        downloads_database: "/path/to/downloads/downloads.db.db",
        failed_downloads_database_check: false,
        failed_downloads_database: "/path/to/failed/downloads/failed_downloads.db",
        conversion_check: false,
        conversion_codec: "MP3",
        conversion_sampling_rate: 44100,
        conversion_bit_depth: 16,
        meta_album_name_playlist_check: false,
        meta_album_order_playlist_check: false,
        meta_exclude_tags_check: false,
        excluded_tags: ""

    };
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
function parseToml(data) {
    return TOML.parse(data);
}

function stringifyToml(data) {
    return TOML.stringify(data);
}
function getCustomStreamripConfigPath(callback) {
    const streamripProcess = spawn('custom_rip', ['config', 'path']);
    let stdout = '';
    let stderr = '';

    streamripProcess.stdout.on('data', (data) => {
        stdout += data.toString();
    });

    streamripProcess.stderr.on('data', (data) => {
        stderr += data.toString();
    });

    streamripProcess.on('close', (code) => {
        if (code !== 0) {
            console.error(`Error getting custom_rip config path: ${stderr}`);
            callback(null);  // Call the callback with null in case of error
            return;
        }

        const configPathMatch = stdout.match(/Config path: '(.+)'/);
        if (configPathMatch && configPathMatch[1]) {
            callback(configPathMatch[1]);  // Return the config path if matched
        } else {
            console.error('Could not find config path in output:', stdout);
            callback(null);  // Return null if no match is found
        }
    });
}
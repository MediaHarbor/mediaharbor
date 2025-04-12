const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const { getNextDownloadOrder } = require('./downloadorder');
const readline = require('readline');
const { dialog, shell, ipcMain } = require('electron');
const prompt = require('electron-prompt');

class gamRip {
    constructor(settingsFilePath, app, dbFunctions) {
        this.settingsFilePath = settingsFilePath;
        this.app = app;
        this.activeProcesses = [];
        this.saveDownloadToDatabase = dbFunctions.saveDownloadToDatabase;
        if (process.platform === 'darwin' || process.platform === 'linux') {
            import('fix-path').then((module) => {
                module.default();
            }).catch(err => console.error('Failed to load fix-path:', err));
        }
        // Dynamic service configuration
        this.serviceConfig = {
            spotify: {
                serviceName: 'Spotify',
                downloadCommand: 'custom_zotify',
                argsBuilder: (command, settings) => {
                    const configPath = path.join(app.getPath('userData'), 'zotify_config.json');
                    const args = [
                        '--config', configPath,
                        '-l', settings.downloadLocation,
                        '--download-quality', command.quality || 'auto',
                    ];
                    const downloadLocation = settings?.createPlatformSubfolders
                        ? `${settings.downloadLocation}/Spotify`
                        : settings.downloadLocation;

                    args.push('-l', downloadLocation);

                    if (settings?.zotify_userName) {
                        args.unshift('--username', settings.zotify_userName);
                    }

                    args.push(command.url);
                    return args;
                },
                progressRegex: /\(\d+\/\d+\)\s+.*?:\s+(\d+)%/,
                metadataParsers: {
                    title: /Title\s+:\s*(.*)/,
                    artist: /Artist\s+:\s*(.*)/,
                    album: /Album\s+:\s*(.*)/,
                    cover: /Cover\s+:\s*(.*)/
                },
                batchSupport: false,
                // Zotify doesn't have batch / bulk download support with txt file, will make different approach, but not now.
                // batchArgsBuilder: (data, settings) => {
                // const configPath = path.join(app.getPath('userData'), 'spotify_config.json');
                // return [
                //     '--config-path', configPath,
                //     '-a', data.quality || 'auto',
                //     '-r', data.filePath
                // ];
                // }
            },
            applemusic: {
                serviceName: 'Apple Music',
                downloadCommand: 'custom_gamdl',
                argsBuilder: (command, settings) => {
                    const cookiesPath = settings.apple_cookies_path || path.join(process.env.USERPROFILE, 'Downloads', 'apple.com_cookies.txt');
                    const configPath = path.join(app.getPath('userData'), 'apple_config.json');
                    return [
                        '--config-path', configPath,
                        '-c', cookiesPath,
                        '--codec-song', command.quality || 'aac-legacy',
                        command.url
                    ];
                },
                progressRegex: /\[download\]\s+(\d+\.\d+)%/,
                metadataParsers: {
                    cover: /Cover:\s*(.*)/,
                    title: /Title:\s*(.*)/,
                    album: /Album:\s*(.*)/,
                    artist: /Artist:\s*(.*)/,
                },
                batchSupport: true,
                batchArgsBuilder: (data, settings) => {
                    const cookiesPath = settings.apple_cookies_path || path.join(process.env.USERPROFILE, 'Downloads', 'apple.com_cookies.txt');
                    return [
                        '-c', cookiesPath,
                        '--codec-song', data.quality || 'aac-legacy',
                        '-r', data.filePath
                    ];
                }
            }
        };
    }

    createProcessEnv() {
        return {
            PYTHONUNBUFFERED: '1',
            ...process.env,
            PYTHONIOENCODING: 'utf-8',
            LANG: 'en_US.UTF-8',
            LC_ALL: 'en_US.UTF-8'
        };
    }

    spawnProcess(command, args) {
        return spawn(command, args, {
            stdio: ['pipe', 'pipe', 'pipe'],
            encoding: 'utf8',
            env: this.createProcessEnv()
        });
    }

    handleDownload(event, command, serviceName) {
        fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
            if (err) {
                event.reply('download-error', 'Could not read settings file');
                return;
            }

            try {
                const settings = JSON.parse(settingsData);
                const config = this.serviceConfig[serviceName];

                const ripArgs = config.argsBuilder(command, settings);

                const downloadOrder = getNextDownloadOrder();

                event.reply('download-info', {
                    title: `${config.serviceName} Download`,
                    order: downloadOrder
                });

                const ripProcess = this.spawnProcess(config.downloadCommand, ripArgs);
                this.activeProcesses.push({
                    process: ripProcess,
                    order: downloadOrder
                });
                let trackInfo = {
                    cover: null,
                    title: null,
                    album: null,
                    artist: null,
                    progress: 0,
                    order: downloadOrder
                };

                let buffer = '';

                const handleOutput = (data) => {
                    buffer += data.toString('utf8');
                    const cleanData = data.toString('utf8').replace(/\r/g, '\n');
                    const lines = cleanData.split('\n');

                    lines.forEach(async line => {
                        console.log('Line:', line);

                        if (line.includes('Username:')) {
                            prompt({
                                title: 'Spotify Login',
                                label: 'Enter your Spotify username (NOT EMAIL):',
                                value: settings?.zotify_userName || '',
                                inputAttrs: {
                                    type: 'text'
                                },
                                type: 'input'
                            })
                                .then((username) => {
                                    if (username) {
                                        // Save username to settings
                                        fs.readFile(this.settingsFilePath, 'utf8', (err, data) => {
                                            if (!err) {
                                                const settings = JSON.parse(data);
                                                settings.zotify_userName = username;
                                                fs.writeFileSync(this.settingsFilePath, JSON.stringify(settings, null, 2));
                                            }
                                        });

                                        // Send username to process
                                        ripProcess.stdin.write(username + '\n');
                                    } else {
                                        ripProcess.kill();
                                        event.reply('download-error', 'Username is required');
                                    }
                                })
                                .catch(err => {
                                    console.error(err);
                                    ripProcess.kill();
                                    event.reply('download-error', 'Failed to get username');
                                });
                        }
                        // Handle authorization link
                        if (line.includes('Click on the following link to login:')) {
                            const nextLines = lines.slice(lines.indexOf(line), lines.indexOf(line) + 3);
                            for (const currentLine of nextLines) {
                                if (currentLine.includes('https://accounts.spotify.com/authorize')) {
                                    const url = currentLine.trim();
                                    console.log('Found authorization URL:', url);
                                    try {
                                        const success = await shell.openExternal(url, {
                                            activate: true,
                                            workingDirectory: process.cwd()
                                        });
                                        
                                        if (success) {
                                            console.log('URL opened successfully in browser');
                                        } else {
                                            throw new Error('Failed to open URL');
                                        }
                                    } catch (err) {
                                        console.error('Failed to open URL:', err);
                                        event.reply('download-error', `Failed to open authorization URL: ${err.message}`);
                                    }
                                    break;
                                }
                            }
                        }

                        const cleanLine = line.replace(/^Error:\s*/, '').trim();

                        // Parse metadata
                        for (let key in config.metadataParsers) {
                            const regex = config.metadataParsers[key];
                            const match = cleanLine.match(regex);
                            if (match) {
                                trackInfo[key] = match[1].trim();
                                if (key === 'cover') {
                                    trackInfo.thumbnail = trackInfo.cover;
                                }
                            }
                        }

                        // Parse download progress
                        const progressMatch = cleanLine.match(config.progressRegex);
                        if (progressMatch) {
                            trackInfo.progress = parseFloat(progressMatch[1]);

                            event.reply('download-update', {
                                order: downloadOrder,
                                progress: trackInfo.progress,
                                title: trackInfo.title,
                                thumbnail: trackInfo.cover,
                                artist: trackInfo.artist,
                                album: trackInfo.album,
                                isBatch: false
                            });
                        }
                    });
                };

                ripProcess.stdout.on('data', handleOutput);
                ripProcess.stderr.on('data', handleOutput);

                ripProcess.on('exit', (code) => {
                    this.activeProcesses = this.activeProcesses.filter(p => p.process !== ripProcess);
                    if (code === 0) {
                        fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
                            const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                            const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

                            const downloadInfo = {
                                downloadName: trackInfo.title,
                                downloadArtistOrUploader: trackInfo.artist,
                                downloadLocation: downloadLocation,
                                downloadThumbnail: trackInfo.cover,
                                service: config.serviceName,
                                albumName: trackInfo.album
                            };
                            this.saveDownloadToDatabase(downloadInfo);

                            event.reply('download-complete', {
                                order: downloadOrder,
                                location: downloadLocation,
                                title: trackInfo.title,
                                thumbnail: trackInfo.cover,
                                artist: trackInfo.artist,
                                album: trackInfo.album,
                                progress: 100,
                                isBatch: false
                            });
                        });
                    } else {
                        event.reply('download-error', `Process exited with code ${code}`);
                    }
                });
            } catch (error) {
                event.reply('download-error', `Failed to parse settings: ${error.message}`);
            }
        });
    }

    handleBatchDownload(event, data, serviceName) {
        fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
            if (err) {
                event.reply('download-error', 'Could not read settings file');
                return;
            }

            try {
                const settings = JSON.parse(settingsData);
                const config = this.serviceConfig[serviceName];

                const ripArgs = config.batchArgsBuilder(data, settings);

                const downloadOrder = getNextDownloadOrder();
                let totalTracks = 0;
                let completedTracks = 0;
                let trackProgressMap = {};
                let overallProgress = 0;

                console.log(`Starting ${config.serviceName} Batch Download with args:`, ripArgs);

                const throttledUpdate = this.throttle((data) => {
                    event.reply('download-update', data);
                }, 250);

                event.reply('download-info', {
                    title: `Batch Download #${downloadOrder}`,
                    downloadArtistOrUploader: config.serviceName,
                    order: downloadOrder,
                    isBatch: true
                });

                const ripProcess = this.spawnProcess(config.downloadCommand, ripArgs);

                ripProcess.on('error', (err) => {
                    console.error("Process error:", err);
                    event.reply('download-error', `Process error: ${err.message}`);
                });

                let buffer = '';

                ripProcess.stdout.on('data', (data) => {
                    buffer += data.toString('utf8');
                    const lines = buffer.split('\n');
                    buffer = lines.pop();

                    lines.forEach(line => {
                        console.log('Line:', line);

                        // Parse total tracks
                        const loadingMatch = line.match(/Found (\d+) tracks/);
                        if (loadingMatch) {
                            totalTracks = parseInt(loadingMatch[1]);
                            console.log("Total tracks found:", totalTracks);
                        }

                        // Parse track info and progress
                        const progressMatch = line.match(config.progressRegex);
                        if (progressMatch) {
                            const progress = parseFloat(progressMatch[1]);
                            const trackId = trackInfo.title || 'Unknown Track';

                            trackProgressMap[trackId] = {
                                trackTitle: trackInfo.title,
                                artist: trackInfo.artist,
                                progress: progress
                            };

                            const totalProgress = Object.values(trackProgressMap)
                                .reduce((sum, track) => sum + track.progress, 0);
                            overallProgress = (totalProgress / (totalTracks * 100)) * 100;

                            throttledUpdate({
                                tracksProgress: Object.values(trackProgressMap),
                                order: downloadOrder,
                                completedTracks,
                                totalTracks,
                                overallProgress: Math.min(overallProgress, 100),
                                isBatch: true
                            });
                        }

                        if (line.includes('Download completed:')) {
                            completedTracks++;
                            const completedTrackId = trackInfo.title || 'Unknown Track';
                            delete trackProgressMap[completedTrackId];
                        }
                    });
                });

                ripProcess.stderr.on('data', (errorData) => {
                    const errorOutput = errorData.toString('utf8');
                    console.error(`Error: ${errorOutput}`);
                    event.reply('download-error', `Error: ${errorOutput}`);
                });

                ripProcess.on('exit', (code) => {
                    if (code === 0) {
                        fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
                            const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                            const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

                            const downloadInfo = {
                                downloadName: `Batch Download #${downloadOrder}`,
                                downloadArtistOrUploader: config.serviceName,
                                downloadLocation: downloadLocation,
                                service: serviceName
                            };
                            this.saveDownloadToDatabase(downloadInfo);

                            event.reply('download-complete', {
                                order: downloadOrder,
                                completedTracks,
                                totalTracks,
                                overallProgress: 100,
                                isBatch: true
                            });
                        });
                    } else {
                        event.reply('download-error', `Process exited with code ${code}`);
                    }
                });
            } catch (parseError) {
                console.error("Settings parse error:", parseError);
                event.reply('download-error', 'Settings parse error');
            }
        });
    }

    // Helper method to throttle function calls
    throttle(func, limit) {
        let inThrottle;
        return function (...args) {
            if (!inThrottle) {
                func.apply(this, args);
                setTimeout(() => inThrottle = false, limit);
            }
        };
    }

    getDefaultSettings() {
        return {
            downloadLocation: this.app.getPath('downloads'),
            appleMusicCookiesPath: path.join(process.env.USERPROFILE, 'Downloads', 'apple.com_cookies.txt'),
            spotifyCookiesPath: path.join(process.env.USERPROFILE, 'Downloads', 'spotify.com_cookies.txt')
        };
    }
    clearCredentials() {
        return new Promise((resolve, reject) => {
            const configProcess = this.spawnProcess('zotify', ['--show-credentials-path', '0']);
            let configPath = '';

            configProcess.stdout.on('data', (data) => {
                configPath += data.toString().trim();
            });

            configProcess.on('error', (err) => {
                reject(new Error(`Failed to get config path: ${err.message}`));
            });

            configProcess.on('exit', (code) => {
                if (code !== 0) {
                    reject(new Error(`Failed to get config path, exit code: ${code}`));
                    return;
                }

                // Delete the config directory
                fs.rm(configPath, { recursive: true, force: true }, (err) => {
                    if (err) {
                        reject(new Error(`Failed to delete credentials: ${err.message}`));
                    } else {
                        // clear the username from settings
                        fs.readFile(this.settingsFilePath, 'utf8', (err, data) => {
                            if (!err) {
                                try {
                                    const settings = JSON.parse(data);
                                    settings.zotify_userName = null;
                                    fs.writeFileSync(this.settingsFilePath, JSON.stringify(settings, null, 2));
                                } catch (error) {
                                    console.error('Error updating settings:', error);
                                }
                            }
                            resolve();
                        });
                    }
                });
            });
        });
    }
}

module.exports = gamRip;

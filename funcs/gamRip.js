const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const {app} = require("electron");
const { getNextDownloadOrder } = require('./downloadorder');

class gamRip {
    constructor(settingsFilePath, app, dbFunctions) {
        this.settingsFilePath = settingsFilePath;
        this.app = app;
        this.saveDownloadToDatabase = dbFunctions.saveDownloadToDatabase;
    }

    handleSpotify(event, command) {
        const { url, quality } = command;
        fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
            if (err) {
                event.reply('download-error', 'Could not read settings file');
                return;
            }

            try {
                const settings = JSON.parse(settingsData);
                const configPath = path.join(app.getPath('userData'), 'spotify_config.json');

                const votifyArgs = [
                    '-a', quality || 'vorbis-low',
                    '--config-path', configPath,
                    url
                ];

                const downloadOrder = getNextDownloadOrder();

                event.reply('download-info', {
                    title: 'Spotify Download',
                    order: downloadOrder
                });

                const votifyProcess = spawn('custom_votify', votifyArgs, {
                    encoding: 'utf8',
                    env: {
                        ...process.env,
                        PYTHONIOENCODING: 'utf-8',
                        LANG: 'en_US.UTF-8',
                        LC_ALL: 'en_US.UTF-8'
                    }
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

                // Handle both stdout and stderr for metadata
                const handleOutput = (data) => {
                    buffer += data.toString('utf8');
                    const lines = buffer.split('\n');
                    buffer = lines.pop();

                    lines.forEach(line => {
                        console.log('Line:', line);

                        // Remove the "Error: " prefix if it exists
                        const cleanLine = line.replace(/^Error:\s*/, '').trim();

                        // Parse metadata
                        if (cleanLine.includes('Cover:')) {
                            trackInfo.cover = cleanLine.split('Cover:')[1].trim();
                            trackInfo.thumbnail = trackInfo.cover;
                        } else if (cleanLine.includes('Title:')) {
                            trackInfo.title = cleanLine.split('Title:')[1].trim();
                        } else if (cleanLine.includes('Album:')) {
                            trackInfo.album = cleanLine.split('Album:')[1].trim();
                        } else if (cleanLine.includes('Artist:')) {
                            trackInfo.artist = cleanLine.split('Artist:')[1].trim();
                        }

                        // Parse download progress
                        const progressMatch = cleanLine.match(/\[download\]\s+(\d+\.\d+)%/);
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

                // Apply the handler to both stdout and stderr
                votifyProcess.stdout.on('data', handleOutput);
                votifyProcess.stderr.on('data', handleOutput);

                votifyProcess.on('exit', (code) => {
                    if (code === 0) {
                        fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
                            const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                            const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

                            const downloadInfo = {
                                downloadName: trackInfo.title,
                                downloadArtistOrUploader: trackInfo.artist,
                                downloadLocation: downloadLocation,
                                downloadThumbnail: trackInfo.cover,
                                service: 'Spotify',
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
    handleApple(event, command) {
        const { url, quality } = command;

        fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
            if (err) {
                event.reply('download-error', 'Could not read settings file');
                return;
            }

            try {
                const settings = JSON.parse(settingsData);
                const cookiesPath = settings.apple_cookies_path || path.join(process.env.USERPROFILE, 'Downloads', 'apple.com_cookies.txt');

                const gamdlArgs = [
                    '-c', cookiesPath,
                    '--codec-song', quality || 'aac-legacy',
                    url
                ];

                const downloadOrder = getNextDownloadOrder();

                event.reply('download-info', {
                    title: 'Apple Music Download',
                    order: downloadOrder
                });

                const gamDLProcess = spawn('custom_gamdl', gamdlArgs, {
                    encoding: 'utf8',
                    env: {
                        ...process.env,
                        PYTHONIOENCODING: 'utf-8',
                        LANG: 'en_US.UTF-8',
                        LC_ALL: 'en_US.UTF-8'
                    }
                });

                let trackInfo = {
                    cover: null,
                    title: null,
                    album: null,
                    progress: 0,
                    order: downloadOrder
                };

                let buffer = '';
                gamDLProcess.stdout.on('data', (data) => {
                    // Accumulate data in buffer and process line by line
                    buffer += data.toString('utf8');
                    const lines = buffer.split('\n');
                    buffer = lines.pop(); // Keep the last incomplete line in buffer

                    lines.forEach(line => {
                        console.log('Line:', line);

                        if (line.startsWith('Cover:')) {
                            trackInfo.cover = line.substring(6).trim();
                            trackInfo.thumbnail = trackInfo.cover;
                        } else if (line.startsWith('Title:')) {
                            trackInfo.title = line.substring(6).trim();
                        } else if (line.startsWith('Album:')) {
                            trackInfo.album = line.substring(6).trim();
                        }
                        else if (line.startsWith('Artist:')) {
                            trackInfo.artist = line.substring(6).trim();
                        }

                        // Parse download progress
                        const progressMatch = line.match(/\[download\]\s+(\d+\.\d+)%/);
                        if (progressMatch) {
                            trackInfo.progress = parseFloat(progressMatch[1]);

                            // Send update matching the frontend rendering format
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

                        // Parse info messages
                        const infoMatch = line.match(/\[INFO.*?\] \((Track \d+\/\d+).*?\) (Downloading ".+")/);
                        if (infoMatch) {
                            console.log(`Download started: ${infoMatch[2]}`);
                        }
                    });
                });

                gamDLProcess.stderr.on('data', (errorData) => {
                    const errorOutput = errorData.toString('utf8');
                    console.error(`Error: ${errorOutput}`);
                    event.reply('download-error', `Error: ${errorOutput}`);
                });

                gamDLProcess.on('exit', (code) => {
                    if (code === 0) {
                        fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
                            const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                            const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

                            const downloadInfo = {
                                downloadName: trackInfo.title,
                                downloadArtistOrUploader: trackInfo.album, // Using album as artist
                                downloadLocation: downloadLocation,
                                downloadThumbnail: trackInfo.cover,
                                service: 'AppleMusic',
                                albumName: trackInfo.album
                            };
                            this.saveDownloadToDatabase(downloadInfo);

                            // Send final update matching frontend format
                            event.reply('download-complete', {
                                order: downloadOrder,
                                location: downloadLocation,
                                title: trackInfo.title,
                                thumbnail: trackInfo.cover,
                                artist: trackInfo.album,
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

    getDefaultSettings() {
        return {
            downloadLocation: this.app.getPath('downloads'),
            appleMusicCookiesPath: path.join(process.env.USERPROFILE, 'Downloads', 'apple.com_cookies.txt'),
            spotifyCookiesPath: path.join(process.env.USERPROFILE, 'Downloads', 'spotify.com_cookies.txt')
        };
    }
}

module.exports = gamRip;
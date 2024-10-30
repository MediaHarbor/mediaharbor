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
    handleSpotifyBatchDownload(event, data) {
        fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
            if (err) {
                event.reply('download-error', 'Could not read settings file');
                return;
            }

            try {
                const settings = JSON.parse(settingsData);
                const configPath = path.join(app.getPath('userData'), 'spotify_config.json');
                const { filePath, quality } = data;
                const votifyArgs = ['--config-path', configPath, '-a', quality || 'vorbis-low', '-r', filePath];
                const downloadOrder = getNextDownloadOrder();
                let totalTracks = 0;
                let completedTracks = 0;
                let trackProgressMap = {};
                let overallProgress = 0;

                console.log("Starting Spotify Batch Download with args:", votifyArgs);

                const throttledUpdate = this.throttle((data) => {
                    event.reply('download-update', data);
                }, 250);

                event.reply('download-info', {
                    title: `Batch Download #${downloadOrder}`,
                    downloadArtistOrUploader: 'Spotify',
                    order: downloadOrder,
                    isBatch: true
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

                votifyProcess.on('error', (err) => {
                    console.error("Process error:", err);
                    event.reply('download-error', `Process error: ${err.message}`);
                });

                votifyProcess.stdout.on('data', (data) => {
                    console.log("Received data:", data.toString('utf8'));
                    const lines = data.toString('utf8').split('\n');

                    lines.forEach(line => {
                        console.log("Processing line:", line);

                        const loadingMatch = line.match(/Found (\d+) tracks/);
                        if (loadingMatch) {
                            totalTracks = parseInt(loadingMatch[1]);
                            console.log("Total tracks found:", totalTracks);
                        }

                        const trackMatch = line.match(/\[(\d+)\] (.+?) - (.+?) \[(.+?)\] \((\d+\.\d+)%\)/);
                        if (trackMatch) {
                            const trackId = trackMatch[1];
                            const artist = trackMatch[2];
                            const title = trackMatch[3];
                            const progress = parseFloat(trackMatch[5]);

                            trackProgressMap[trackId] = {
                                trackTitle: title,
                                artist: artist,
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
                            const completedTrackMatch = line.match(/Download completed: \[(\d+)\]/);
                            if (completedTrackMatch) {
                                const completedTrackId = completedTrackMatch[1];
                                delete trackProgressMap[completedTrackId];
                            }
                        }
                    });
                });

                votifyProcess.stderr.on('data', (errorData) => {
                    const errorOutput = errorData.toString();
                    console.error("Process stderr:", errorOutput);
                    if (!errorOutput.includes('[INFO')) {
                        event.reply('download-error', `Error: ${errorOutput}`);
                    }
                });

                votifyProcess.on('exit', (code) => {
                    console.log(`Process exited with code: ${code}`);
                    if (code === 0) {
                        const downloadLocation = settings.downloadLocation || app.getPath('downloads');

                        const downloadInfo = {
                            downloadName: `Batch Download #${downloadOrder}`,
                            downloadArtistOrUploader: 'Spotify',
                            downloadLocation: downloadLocation,
                            service: 'spotify'
                        };
                        this.saveDownloadToDatabase(downloadInfo);

                        event.reply('download-complete', {
                            order: downloadOrder,
                            completedTracks,
                            totalTracks,
                            overallProgress: 100
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


    handleAppleMusicBatchDownload(event, data) {
        const { filePath, quality } = data;
        const settings = JSON.parse(fs.readFileSync(this.settingsFilePath, 'utf8'));
        const cookiesPath = settings.apple_cookies_path || path.join(process.env.USERPROFILE, 'Downloads', 'apple.com_cookies.txt');

        const gamdlArgs = [
            '-c', cookiesPath,
            '--codec-song', quality || 'aac-legacy',
            '-r',
            filePath
        ];

        const downloadOrder = getNextDownloadOrder();
        let totalUrls = 0;
        let completedTracks = 0;
        let trackProgressMap = {};
        let currentTrackInfo = {};
        let overallProgress = 0;

        // Throttle update frequency to prevent flickering
        const throttledUpdate = this.throttle((data) => {
            event.reply('download-update', data);
        }, 250); // Update every 250ms at most

        event.reply('download-info', {
            title: `Batch Download #${downloadOrder}`,
            downloadArtistOrUploader: 'Apple Music',
            order: downloadOrder,
            isBatch: true
        });

        const gamdlProcess = spawn('custom_gamdl', gamdlArgs, {
            encoding: 'utf8',
            env: {
                ...process.env,
                PYTHONIOENCODING: 'utf-8',
                LANG: 'en_US.UTF-8',
                LC_ALL: 'en_US.UTF-8'
            }
        });

        let buffer = '';

        gamdlProcess.stdout.on('data', (data) => {
            buffer += data.toString('utf8');
            const lines = buffer.split('\n');
            buffer = lines.pop();

            lines.forEach(line => {
                console.log('Line:', line);

                const urlMatch = line.match(/\(URL (\d+)\/(\d+)\)/);
                if (urlMatch) {
                    totalUrls = parseInt(urlMatch[2]);
                }


                if (line.startsWith('Title:')) {
                    currentTrackInfo.title = line.split('Title:')[1].trim();
                    currentTrackInfo.id = currentTrackInfo.title.replace(/[^a-zA-Z0-9]/g, '');
                } else if (line.startsWith('Album:')) {
                    currentTrackInfo.album = line.split('Album:')[1].trim();
                }

                const downloadMatch = line.match(/Downloading: (\d+\.?\d*)%/);
                if (downloadMatch && currentTrackInfo.id) {
                    const progress = parseFloat(downloadMatch[1]);

                    trackProgressMap[currentTrackInfo.id] = {
                        trackTitle: currentTrackInfo.title,
                        artist: currentTrackInfo.artist,
                        album: currentTrackInfo.album,
                        progress: progress
                    };

                    // Currently not works
                    const totalProgress = Object.values(trackProgressMap)
                        .reduce((sum, track) => sum + track.progress, 0);
                    overallProgress = (totalProgress / (totalUrls * 100)) * 100;

                    throttledUpdate({
                        tracksProgress: Object.values(trackProgressMap),
                        order: downloadOrder,
                        completedTracks,
                        totalTracks: totalUrls,
                        overallProgress: Math.min(overallProgress, 100),
                        isBatch: true
                    });
                }

                if (line.includes('100% of') && currentTrackInfo.id) {
                    completedTracks++;
                    delete trackProgressMap[currentTrackInfo.id];
                    currentTrackInfo = {};
                }
            });
        });

        gamdlProcess.stderr.on('data', (errorData) => {
            const errorOutput = errorData.toString();
            if (!errorOutput.includes('[INFO')) {
                event.reply('download-error', `Error: ${errorOutput}`);
            }
        });

        gamdlProcess.on('exit', (code) => {
            if (code === 0) {
                fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
                    const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                    const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

                    const downloadInfo = {
                        downloadName: `Batch Download #${downloadOrder}`,
                        downloadArtistOrUploader: 'Apple Music',
                        downloadLocation: downloadLocation,
                        service: 'applemusic'
                    };
                    this.saveDownloadToDatabase(downloadInfo);
                });
                event.reply('download-complete', {
                    order: downloadOrder,
                    completedTracks,
                    totalTracks: totalUrls,
                    overallProgress: 100
                });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
    }

    // Helper method to throttle function calls
    throttle(func, limit) {
        let inThrottle;
        return function(...args) {
            if (!inThrottle) {
                func.apply(this, args);
                inThrottle = true;
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
}

module.exports = gamRip;
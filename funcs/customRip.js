const { spawn } = require('child_process');
const fs = require('fs');

class CustomRip {
    constructor(settingsFilePath, app, dbFunctions) {
        this.settingsFilePath = settingsFilePath;
        this.app = app;
        this.downloadCount = 0;
        this.saveDownloadToDatabase = dbFunctions.saveDownloadToDatabase;
    }

    handleStreamRip(event, command) {
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
    }

    handleQobuzDownload(event, data) {
        const { url, quality } = data;
        const ripArgs = ['-q', quality, 'url', url];
        this.downloadCount++;

        event.reply('download-info', {
            title: 'Qobuz Download',
            order: this.downloadCount
        });

        const ripProcess = spawn('custom_rip', ripArgs);
        let currentEid = null;

        ripProcess.stdout.on('data', (data) => {
            const output = data.toString();
            console.log(output);

            const eidMatch = output.match(/track-id=(\d+)/);
            if (eidMatch && eidMatch[1] !== currentEid) {
                currentEid = eidMatch[1];
                this.getQobuzDetails(currentEid, event, this.downloadCount);
            }

            const progressMatch = output.match(/(\d+\.\d+)%/);
            if (progressMatch) {
                const progress = parseFloat(progressMatch[1]);
                event.reply('download-update', {
                    progress,
                    order: this.downloadCount
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
                fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
                    const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                    const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

                    if (currentEid) {
                        this.getQobuzDetails(currentEid, {
                            reply: (channel, details) => {
                                if (channel === 'qobuz-details') {
                                    const downloadInfo = {
                                        downloadName: details.title,
                                        downloadArtistOrUploader: details.album.artist.name,
                                        downloadLocation: downloadLocation,
                                        downloadThumbnail: details.album.image.small,
                                        service: 'qobuz'
                                    };
                                    this.saveDownloadToDatabase(downloadInfo);
                                }
                            }
                        }, this.downloadCount);
                    }
                });
                event.reply('download-complete', { order: this.downloadCount });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
    }

    handleDeezerDownload(event, data) {
        const { url, quality } = data;
        const ripArgs = ['-q', quality, 'url', url];
        this.downloadCount++;

        event.reply('download-info', {
            title: 'Deezer Download',
            order: this.downloadCount
        });

        let trackId = null;
        const deezerIdMatch = url.match(/\/track\/(\d+)/);
        if (deezerIdMatch) {
            trackId = deezerIdMatch[1];
            this.getDeezerDetails(trackId, event, this.downloadCount);
        }

        const ripProcess = spawn('custom_rip', ripArgs);

        ripProcess.stdout.on('data', (data) => {
            const output = data.toString();
            console.log(output);
            const progressMatch = output.match(/(\d+\.\d+)%/);
            if (progressMatch) {
                const progress = parseFloat(progressMatch[1]);
                event.reply('download-update', {
                    progress,
                    order: this.downloadCount
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
                fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
                    const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                    const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

                    if (trackId) {
                        this.getDeezerDetails(trackId, {
                            reply: (channel, details) => {
                                if (channel === 'deezer-details') {
                                    const downloadInfo = {
                                        downloadName: details.title,
                                        downloadArtistOrUploader: details.artist.name,
                                        downloadLocation: downloadLocation,
                                        downloadThumbnail: details.album.cover_medium,
                                        service: 'deezer'
                                    };
                                    this.saveDownloadToDatabase(downloadInfo);
                                }
                            }
                        }, this.downloadCount);
                    }
                });
                event.reply('download-complete', { order: this.downloadCount });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
    }

    handleTidalDownload(event, data) {
        const { url, quality } = data;
        const ripArgs = ['-q', quality, 'url', url];
        this.downloadCount++;

        event.reply('download-info', {
            title: 'Tidal Download',
            order: this.downloadCount
        });

        let currentTrackId = null;
        const tidalIdMatch = url.match(/\/track\/(\d+)/);
        if (tidalIdMatch) {
            currentTrackId = tidalIdMatch[1];
            this.getTidalDetails(currentTrackId, event, this.downloadCount);
        }

        const ripProcess = spawn('custom_rip', ripArgs);

        ripProcess.stdout.on('data', (data) => {
            const output = data.toString();
            console.log(output);
            const progressMatch = output.match(/(\d+\.\d+)%/);
            if (progressMatch) {
                const progress = parseFloat(progressMatch[1]);
                event.reply('download-update', {
                    progress,
                    order: this.downloadCount
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
                fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
                    const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                    const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

                    if (currentTrackId) {
                        this.getTidalDetails(currentTrackId, {
                            reply: (channel, details) => {
                                if (channel === 'tidal-details') {
                                    const downloadInfo = {
                                        downloadName: details.title,
                                        downloadArtistOrUploader: details.artist,
                                        downloadLocation: downloadLocation,
                                        downloadThumbnail: details.thumbnail,
                                        service: 'tidal'
                                    };
                                    this.saveDownloadToDatabase(downloadInfo);
                                }
                            }
                        }, this.downloadCount);
                    }
                });
                event.reply('download-complete', { order: this.downloadCount });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
    }

    getTidalDetails(trackId, event, downloadCount) {
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

    getDeezerDetails(trackId, event, downloadCount) {
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

    getQobuzDetails(eid, event, downloadCount) {
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

    getDefaultSettings() {
        return {
            downloadLocation: this.app.getPath('downloads')
        };
    }
    handleQobuzBatchDownload(event, data) {
        const { filePath, quality } = data;
        const ripArgs = ['-q', quality, 'file', filePath];
        this.downloadCount++;
        let totalTracks = 0;
        let completedTracks = 0;
        let trackProgressMap = {}; // To store the progress of each track

        event.reply('download-info', {
            title: `Batch Download #${this.downloadCount}`,
            downloadArtistOrUploader: 'Qobuz',
            order: this.downloadCount,
            isBatch: true
        });

        const ripProcess = spawn('custom_rip', ripArgs);
        let currentTrackId = null;

        ripProcess.stdout.on('data', (data) => {
            const output = data.toString();
            console.log(output);

            // Detect total tracks in batch
            const loadingMatch = output.match(/Loading (\d+) items/);
            if (loadingMatch) {
                totalTracks = parseInt(loadingMatch[1]);
            }

            // Track progress detection
            const trackMatch = output.match(/Downloading: Track (\d+): track-id=(\d+) (\d+)\/(\d+) \((\d+\.\d+)%\)/);
            if (trackMatch) {
                const trackNumber = trackMatch[1];
                const trackId = trackMatch[2];
                const progress = parseFloat(trackMatch[5]);

                // Fetch the title if the track is new
                if (!trackProgressMap[trackId]) {
                    currentTrackId = trackId;
                    this.getQobuzDetails(trackId, {
                        reply: (channel, details) => {
                            if (channel === 'qobuz-details') {
                                trackProgressMap[trackId] = {
                                    trackTitle: details.title, // Store track title
                                    trackId,
                                    progress
                                };

                                // Send the updated progress to the frontend
                                event.reply('download-update', {
                                    tracksProgress: Object.values(trackProgressMap),
                                    order: this.downloadCount,
                                    completedTracks,
                                    totalTracks
                                });
                            }
                        }
                    });
                } else {
                    // Update progress for the track
                    trackProgressMap[trackId].progress = progress;

                    // Send the updated progress to the frontend
                    event.reply('download-update', {
                        tracksProgress: Object.values(trackProgressMap),
                        order: this.downloadCount,
                        completedTracks,
                        totalTracks
                    });
                }
            }

            // Track completion detection
            if (output.includes('Completed: Track')) {
                completedTracks++;
                const completedTrackMatch = output.match(/Completed: Track \d+: track-id=(\d+)/);
                if (completedTrackMatch) {
                    const completedTrackId = completedTrackMatch[1];
                    delete trackProgressMap[completedTrackId]; // Remove the completed track from the map
                }

                // Send completion to the frontend
                event.reply('download-complete', {
                    order: this.downloadCount,
                    completedTracks,
                    totalTracks
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
                event.reply('download-complete', {
                    order: this.downloadCount,
                    completedTracks,
                    totalTracks
                });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
    }

    handleDeezerBatchDownload(event, data) {
        const { filePath, quality } = data;
        const ripArgs = ['-q', quality, 'file', filePath];
        this.downloadCount++;
        let totalTracks = 0;
        let completedTracks = 0;
        let trackProgressMap = {}; // To store the progress of each track

        event.reply('download-info', {
            title: `Batch Download #${this.downloadCount}`,
            downloadArtistOrUploader: 'Deezer',
            order: this.downloadCount
        });

        const ripProcess = spawn('custom_rip', ripArgs);

        ripProcess.stdout.on('data', (data) => {
            const output = data.toString();
            console.log(output);

            // Detect total tracks in batch
            const loadingMatch = output.match(/Loading (\d+) items/);
            if (loadingMatch) {
                totalTracks = parseInt(loadingMatch[1]);
            }

            // Track progress detection (handle multiple tracks concurrently)
            const trackMatch = output.match(/Downloading: Track \d+: track-id=(\d+) (\d+)\/(\d+) \((\d+\.\d+)%\)/);
            if (trackMatch) {
                const trackId = trackMatch[1];
                const progress = parseFloat(trackMatch[4]);

                // Fetch the track details
                this.getDeezerDetails(trackId, {
                    reply: (channel, details) => {
                        if (channel === 'deezer-details') {
                            trackProgressMap[trackId] = {
                                trackTitle: details.title, // Store track title
                                artist: details.artist.name,
                                progress
                            };

                            // Send updated progress to the frontend
                            event.reply('download-update', {
                                tracksProgress: Object.values(trackProgressMap), // Send all track progresses
                                order: this.downloadCount,
                                completedTracks,
                                totalTracks,
                                isBatch: true
                            });
                        }
                    }
                });
            }

            // Track completion detection
            if (output.includes('Completed: Track')) {
                completedTracks++;
                const completedTrackMatch = output.match(/Completed: Track \d+: track-id=(\d+)/);
                if (completedTrackMatch) {
                    const completedTrackId = completedTrackMatch[1];
                    delete trackProgressMap[completedTrackId]; // Remove completed track
                }

                // Send completion to the frontend
                event.reply('download-complete', {
                    order: this.downloadCount,
                    completedTracks,
                    totalTracks
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
                fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
                    const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                    const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

                    const downloadInfo = {
                        downloadName: `Batch Download #${this.downloadCount}`,
                        downloadArtistOrUploader: 'Deezer',
                        downloadLocation: downloadLocation,
                        service: 'deezer'
                    };
                    this.saveDownloadToDatabase(downloadInfo);
                });
                event.reply('download-complete', {
                    order: this.downloadCount,
                    completedTracks,
                    totalTracks
                });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
    }


    handleTidalBatchDownload(event, data) {
        const { filePath, quality } = data;
        const ripArgs = ['-q', quality, 'file', filePath];
        this.downloadCount++;
        let totalTracks = 0;
        let completedTracks = 0;
        let trackProgressMap = {}; // To store the progress of each track

        event.reply('download-info', {
            title: `Batch Download #${this.downloadCount}`,
            downloadArtistOrUploader: 'Tidal',
            order: this.downloadCount
        });

        const ripProcess = spawn('custom_rip', ripArgs);

        ripProcess.stdout.on('data', (data) => {
            const output = data.toString();
            console.log(output);

            // Detect total tracks in batch
            const loadingMatch = output.match(/Loading (\d+) items/);
            if (loadingMatch) {
                totalTracks = parseInt(loadingMatch[1]);
            }

            // Track progress detection (handle multiple tracks concurrently)
            const trackMatch = output.match(/Downloading: Track \d+: track-id=(\d+) (\d+)\/(\d+) \((\d+\.\d+)%\)/);
            if (trackMatch) {
                const trackId = trackMatch[1];
                const progress = parseFloat(trackMatch[4]);

                // Fetch the track details
                this.getTidalDetails(trackId, {
                    reply: (channel, details) => {
                        if (channel === 'tidal-details') {
                            trackProgressMap[trackId] = {
                                trackTitle: details.title, // Store track title
                                artist: details.artist,
                                progress
                            };

                            // Send updated progress to the frontend
                            event.reply('download-update', {
                                tracksProgress: Object.values(trackProgressMap), // Send all track progresses
                                order: this.downloadCount,
                                completedTracks,
                                totalTracks
                            });
                        }
                    }
                });
            }

            // Track completion detection
            if (output.includes('Completed: Track')) {
                completedTracks++;
                const completedTrackMatch = output.match(/Completed: Track \d+: track-id=(\d+)/);
                if (completedTrackMatch) {
                    const completedTrackId = completedTrackMatch[1];
                    delete trackProgressMap[completedTrackId]; // Remove completed track
                }

                // Send completion to the frontend
                event.reply('download-complete', {
                    order: this.downloadCount,
                    completedTracks,
                    totalTracks
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
                fs.readFile(this.settingsFilePath, 'utf8', (err, settingsData) => {
                    const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                    const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

                    const downloadInfo = {
                        downloadName: `Batch Download #${this.downloadCount}`,
                        downloadArtistOrUploader: 'Tidal',
                        downloadLocation: downloadLocation,
                        service: 'tidal',
                        isBatch: true

                    };
                    this.saveDownloadToDatabase(downloadInfo);
                });
                event.reply('download-complete', {
                    order: this.downloadCount,
                    completedTracks,
                    totalTracks
                });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
    }

}

module.exports = CustomRip;
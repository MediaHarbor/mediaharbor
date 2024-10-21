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
}

module.exports = CustomRip;
const { spawn } = require('child_process');
const fs = require('fs');
const { getNextDownloadOrder } = require('./downloadorder');
const {fetchServiceDetails} = require('./fetcher');
class CustomRip {
    constructor(settingsFilePath, app, dbFunctions) {
        this.settingsFilePath = settingsFilePath;
        this.app = app;
        this.saveDownloadToDatabase = dbFunctions.saveDownloadToDatabase;

        this.serviceConfig = {
            qobuz: {
                trackIdRegex: /track-id=(\d+)/,
                progressRegex: /(\d+\.\d+)%/,
                serviceName: 'Qobuz',
                detailsChannel: 'qobuz-details',
                downloadInfoParser: (details) => ({
                    downloadName: details.title,
                    downloadArtistOrUploader: details.album.artist.name,
                    downloadThumbnail: details.album.image.small
                }),
                formatDetailsForEvent: (details, downloadCount) => ({
                    order: downloadCount,
                    album: {
                        maximum_bit_depth: details.album.maximum_bit_depth,
                        image: { small: details.album.image.small },
                        artist: { name: details.album.artist.name },
                        maximum_sampling_rate: details.album.maximum_sampling_rate
                    },
                    title: details.title
                })
            },
            deezer: {
                trackIdRegex: /track-id=(\d+)/,
                progressRegex: /(\d+\.\d+)%/,
                serviceName: 'Deezer',
                detailsChannel: 'deezer-details',
                downloadInfoParser: (details) => ({
                    downloadName: details.title,
                    downloadArtistOrUploader: details.artist.name,
                    downloadThumbnail: details.album.cover_medium
                }),
                formatDetailsForEvent: (details, downloadCount) => ({
                    order: downloadCount,
                    ...details
                })
            },
            tidal: {
                trackIdRegex: /track-id=(\d+)/,
                progressRegex: /(\d+\.\d+)%/,
                serviceName: 'Tidal',
                detailsChannel: 'tidal-details',
                downloadInfoParser: (details) => ({
                    downloadName: details.title,
                    downloadArtistOrUploader: details.artist,
                    downloadThumbnail: details.thumbnail
                }),
                formatDetailsForEvent: (details, downloadCount) => ({
                    order: downloadCount,
                    ...details
                })
            }
        };
    }

    createProcessEnv() {
        return {
            ...process.env,
            PYTHONIOENCODING: 'utf-8',
            LANG: 'en_US.UTF-8',
            LC_ALL: 'en_US.UTF-8',
            FORCE_COLOR: '1'
        };
    }

    spawnProcess(command, args) {
        return spawn(command, args, {
            env: this.createProcessEnv(),
            stdio: ['pipe', 'pipe', 'pipe'],
            windowsHide: true
        });
    }

    async handleDownload(event, data, serviceName) {
        const { url, quality } = data;
        const ripArgs = ['-q', quality, 'url', url];
        const downloadOrder = getNextDownloadOrder();
        const config = this.serviceConfig[serviceName];

        event.reply('download-info', {
            title: `${config.serviceName} Download`,
            order: downloadOrder
        });

        const ripProcess = this.spawnProcess('custom_rip', ripArgs);
        let currentTrackId = null;

        ripProcess.stdout.on('data', async (data) => {
            const output = data.toString('utf-8');
            console.log(output);

            const trackIdMatch = output.match(config.trackIdRegex);
            if (trackIdMatch && trackIdMatch[1] !== currentTrackId) {
                currentTrackId = trackIdMatch[1];
                try {
                    const details = await fetchServiceDetails(serviceName, currentTrackId);
                    event.reply(config.detailsChannel, config.formatDetailsForEvent(details, downloadOrder));
                } catch (error) {
                    console.error('Error fetching track details:', error);
                }
            }

            const progressMatch = output.match(config.progressRegex);
            if (progressMatch) {
                const progress = parseFloat(progressMatch[1]);
                event.reply('download-update', {
                    progress,
                    order: downloadOrder
                });
            }
        });

        ripProcess.stderr.on('data', (errorData) => {
            const errorOutput = errorData.toString('utf-8');
            console.error(`Error: ${errorOutput}`);
            event.reply('download-error', `Error: ${errorOutput}`);
        });

        ripProcess.on('exit', async (code) => {
            if (code === 0) {
                fs.readFile(this.settingsFilePath, 'utf8', async (err, settingsData) => {
                    const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                    const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

                    if (currentTrackId) {
                        try {
                            const details = await fetchServiceDetails(serviceName, currentTrackId);
                            const downloadInfo = {
                                ...config.downloadInfoParser(details),
                                downloadLocation,
                                service: serviceName
                            };
                            this.saveDownloadToDatabase(downloadInfo);
                        } catch (error) {
                            console.error('Error fetching track details:', error);
                        }
                    }
                });
                event.reply('download-complete', { order: downloadOrder });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
    }

    async handleBatchDownload(event, data, serviceName) {
        const { filePath, quality } = data;
        const ripArgs = ['-q', quality, 'file', filePath];
        const downloadOrder = getNextDownloadOrder();
        const config = this.serviceConfig[serviceName];
        let totalTracks = 0;
        let completedTracks = 0;
        let trackProgressMap = {};

        event.reply('download-info', {
            title: `Batch Download #${downloadOrder}`,
            downloadArtistOrUploader: config.serviceName,
            order: downloadOrder,
            isBatch: true
        });

        const ripProcess = this.spawnProcess('custom_rip', ripArgs);

        ripProcess.stdout.on('data', async (data) => {
            const output = data.toString();
            console.log(output);

            const loadingMatch = output.match(/Loading (\d+) items/);
            if (loadingMatch) {
                totalTracks = parseInt(loadingMatch[1]);
            }

            const trackMatch = output.match(/Downloading: Track \d+: track-id=(\d+) \d+\/\d+ \((\d+\.\d+)%\)/);
            if (trackMatch) {
                const [, trackId, progress] = trackMatch;

                if (!trackProgressMap[trackId]) {
                    try {
                        const details = await fetchServiceDetails(serviceName, trackId);
                        trackProgressMap[trackId] = {
                            trackTitle: details.title,
                            artist: config.downloadInfoParser(details).downloadArtistOrUploader,
                            progress: parseFloat(progress)
                        };

                        event.reply('download-update', {
                            tracksProgress: Object.values(trackProgressMap),
                            order: downloadOrder,
                            completedTracks,
                            totalTracks,
                            isBatch: true
                        });
                    } catch (error) {
                        console.error('Error fetching track details:', error);
                    }
                } else {
                    trackProgressMap[trackId].progress = parseFloat(progress);
                    event.reply('download-update', {
                        tracksProgress: Object.values(trackProgressMap),
                        order: downloadOrder,
                        completedTracks,
                        totalTracks,
                        isBatch: true
                    });
                }
            }

            if (output.includes('Completed: Track')) {
                completedTracks++;
                const completedTrackMatch = output.match(/Completed: Track \d+: track-id=(\d+)/);
                if (completedTrackMatch) {
                    delete trackProgressMap[completedTrackMatch[1]];
                }
                event.reply('download-complete', {
                    order: downloadOrder,
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
                        downloadName: `Batch Download #${downloadOrder}`,
                        downloadArtistOrUploader: config.serviceName,
                        downloadLocation,
                        service: serviceName,
                        isBatch: true
                    };
                    this.saveDownloadToDatabase(downloadInfo);
                });
                event.reply('download-complete', {
                    order: downloadOrder,
                    completedTracks,
                    totalTracks
                });
            } else {
                event.reply('download-error', `Process exited with code ${code}`);
            }
        });
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

    getDefaultSettings() {
        return {
            downloadLocation: this.app.getPath('downloads')
        };
    }
}

module.exports = CustomRip;
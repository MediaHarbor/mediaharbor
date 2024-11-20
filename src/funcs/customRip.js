const { spawn } = require('child_process');
const fs = require('fs');
const readline = require('readline');
const { getNextDownloadOrder } = require('./downloadorder');
const { getPythonCommand } = require("./spawner");
const path = require("path");
const { app } = require("electron");


class CustomRip {
    constructor(settingsFilePath, app, dbFunctions) {
        this.settingsFilePath = settingsFilePath;
        this.app = app;
        this.saveDownloadToDatabase = dbFunctions.saveDownloadToDatabase;
        this.debug = true;
        if (process.platform === 'darwin' || process.platform === 'linux') {
            import('fix-path').then((module) => {
                module.default();
            }).catch(err => console.error('Failed to load fix-path:', err));
        }

        this.errorHandlers = {
            'AuthenticationError: Invalid credentials': 'Invalid Credentials',
            'Found invalid url': 'Invalid URL detected, skipping.',
            'Enter': 'Enter your credintals on settings!',
            'ClientConnectorCertificateError': `
        SSL verification failed. Try:
        1. Run "Install Certificates.command" in your Python directory.
        2. Ensure Python & aiohttp are up-to-date.
        For more help, contact support.
    `,
            'Try running rip config reset': 'Config is broken try running $rip config reset, if not fixes report issue with logs'
        };
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

    log(...args) {
        if (this.debug) {
            console.log('[CustomRip Debug]', ...args);
        }
    }

    logError(...args) {
        console.error('[CustomRip Error]', ...args);
    }
    checkForErrors(line) {
        for (const [pattern, message] of Object.entries(this.errorHandlers)) {
            if (line.includes(pattern)) {
                return message;
            }
        }
        return null;
    }


    createProcessEnv() {
        const baseEnv = {
            ...process.env,
            PYTHONIOENCODING: 'utf-8',
            LANG: 'en_US.UTF-8',
            LC_ALL: 'en_US.UTF-8',
            FORCE_COLOR: '0', 
            DEBUG: this.debug ? '1' : '0'
        };

        if (process.platform === 'linux' || process.platform === 'darwin') {
            const additionalPaths = [
                '/usr/local/bin',
                '/usr/bin',
                '/bin',
                path.join(process.env.HOME || '', '.local/bin')
            ];

            const currentPath = process.env.PATH || '';
            baseEnv.PATH = additionalPaths.join(path.delimiter) + path.delimiter + currentPath;

            this.log('macOS/Linux PATH:', baseEnv.PATH);
        }

        this.log('Process Environment:', baseEnv);
        return baseEnv;
    }

    spawnProcess(command, args) {
        this.log('Spawning process:', { command, args });

        const options = {
            env: this.createProcessEnv(),
            stdio: ['pipe', 'pipe', 'pipe'],
            windowsHide: true,
            shell: process.platform === 'linux'
        };

        if (process.platform === 'win32' && !command.endsWith('.exe')) {
            command += '.exe';
        }

        this.log('Spawn options:', options);

        try {
            const proc = spawn(command, args, options);
            this.log('Process spawned successfully. PID:', proc.pid);
            return proc;
        } catch (error) {
            this.logError('Failed to spawn process:', error);
            throw error;
        }
    }


    // Forgot to keep this long time ago while copying from other IDE. currently works but, it can't show live data. will ve fixed on 1.1.0
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


    async handleDownload(event, data, serviceName) {
        const { default: stripAnsi } = await import('strip-ansi');
        this.log('Starting download:', { data, serviceName });

        const { url, quality } = data;
        const ripArgs = ['-q', quality, 'url', url];
        const downloadOrder = getNextDownloadOrder();
        const config = this.serviceConfig[serviceName];

        event.reply('download-info', {
            title: `${config.serviceName} Download`,
            order: downloadOrder
        });

        try {
            const ripProcess = this.spawnProcess('custom_rip', ripArgs);
            let currentTrackId = null;

            const rl = readline.createInterface({
                input: ripProcess.stdout,
                crlfDelay: Infinity
            });

            let timeout = setTimeout(() => {
                this.logError('Process timed out waiting for credentials input.');
                ripProcess.kill('SIGTERM');
                event.reply('download-error', 'Credentials required but not provided in settings. Process timed out.');
            }, 5000); // Set timeout to 5 seconds

            rl.on('line', async (line) => {
                clearTimeout(timeout); // Clear the timeout if output is received
                timeout = setTimeout(() => { // Reset timeout on each new line
                    this.logError('Process timed out waiting for credentials input.');
                    ripProcess.kill('SIGTERM');
                    event.reply('download-error', 'Credentials required but not provided in settings. Process timed out.');
                }, 5000);

                const output = stripAnsi(line);
                this.log('Process output line:', output);

                // Custom error handling
                const errorMessage = this.checkForErrors(output);
                if (errorMessage) {
                    event.reply('download-error', errorMessage);
                    return;
                }

                const trackIdMatch = output.match(config.trackIdRegex);
                if (trackIdMatch && trackIdMatch[1] !== currentTrackId) {
                    currentTrackId = trackIdMatch[1];
                    try {
                        const details = await this.fetchServiceDetails(serviceName, currentTrackId);
                        event.reply(config.detailsChannel, config.formatDetailsForEvent(details, downloadOrder));
                    } catch (error) {
                        this.logError('Error fetching track details:', error);
                        event.reply('download-error', `Error fetching track details: ${error.message}`);
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

            ripProcess.stderr.on('data', (data) => {
                clearTimeout(timeout); // Clear the timeout if output is received
                timeout = setTimeout(() => { // Reset timeout on each new line
                    this.logError('Process timed out waiting for credentials input.');
                    ripProcess.kill('SIGTERM');
                    event.reply('download-error', 'Credentials required but not provided in settings. Process timed out.');
                }, 5000);

                const errorOutput = data.toString('utf-8');
                this.logError('Process error:', errorOutput);

                // Custom error handling for stderr
                const errorMessage = this.checkForErrors(errorOutput);
                if (errorMessage) {
                    event.reply('download-error', errorMessage);
                } else {
                    event.reply('download-error', `Error: ${errorOutput}`);
                }
            });

            ripProcess.on('exit', async (code) => {
                clearTimeout(timeout); // Clear timeout on process exit
                this.log('Process exited with code:', code);
                if (code === 0) {
                    await this.handleSuccessfulDownload(event, currentTrackId, serviceName, downloadOrder);
                } else {
                    event.reply('download-error', `Process exited with code ${code}`);
                }
            });
        } catch (error) {
            this.logError('Failed to start download process:', error);
            event.reply('download-error', `Failed to start download: ${error.message}`);
        }
    }


    async handleSuccessfulDownload(event, currentTrackId, serviceName, downloadOrder) {
        try {
            const settingsData = await fs.promises.readFile(this.settingsFilePath, 'utf8');
            const settings = JSON.parse(settingsData);
            const downloadLocation = settings.downloadLocation || this.app.getPath('downloads');

            if (currentTrackId) {
                try {
                    const details = await this.fetchServiceDetails(serviceName, currentTrackId);
                    const downloadInfo = {
                        ...this.serviceConfig[serviceName].downloadInfoParser(details),
                        downloadLocation,
                        service: serviceName
                    };
                    await this.saveDownloadToDatabase(downloadInfo);
                } catch (error) {
                    this.logError('Error saving download info:', error);
                }
            }
            event.reply('download-complete', { order: downloadOrder });
        } catch (error) {
            this.logError('Error handling successful download:', error);
            event.reply('download-error', `Error finalizing download: ${error.message}`);
        }
    }

    async fetchServiceDetails(serviceName, trackId) {
        this.log('Fetching service details:', { serviceName, trackId });

        try {
            const pythonCommand = await getPythonCommand();
            const pythonScript = this.getPythonScript(serviceName);

            if (!pythonScript) {
                throw new Error(`Unsupported service: ${serviceName}`);
            }

            const scriptFile = this.getResourcePath(pythonScript);

            if (!fs.existsSync(scriptFile)) {
                throw new Error(`Python script not found: ${scriptFile}`);
            }

            this.log('Executing Python script:', {
                command: pythonCommand,
                script: scriptFile,
                trackId: trackId
            });

            return new Promise((resolve, reject) => {
                const pythonProcess = spawn(
                    pythonCommand,
                    [scriptFile, '--get-details', trackId],
                    {
                        env: this.createProcessEnv(),
                        shell: process.platform === 'linux'
                    }
                );

                let output = '';
                let errorOutput = '';

                pythonProcess.stdout.on('data', (data) => {
                    const chunk = data.toString('utf-8');
                    output += chunk;
                    this.log('Python process stdout:', chunk);
                });

                pythonProcess.stderr.on('data', (data) => {
                    const chunk = data.toString('utf-8');
                    errorOutput += chunk;
                    this.logError('Python process stderr:', chunk);
                });

                pythonProcess.on('close', (code) => {
                    this.log('Python process closed:', { code, output, errorOutput });

                    if (code === 0) {
                        try {
                            const details = JSON.parse(output);
                            this.log('Successfully parsed service details:', details);
                            resolve(details);
                        } catch (error) {
                            this.logError('Failed to parse service details:', { error: error.message, output });
                            reject(new Error(`Failed to parse details: ${error.message}\nOutput: ${output}`));
                        }
                    } else {
                        this.logError('Python script failed:', { code, errorOutput });
                        reject(new Error(`Python script failed with code ${code}\nError: ${errorOutput}`));
                    }
                });
            });
        } catch (error) {
            this.logError('Error in fetchServiceDetails:', error);
            throw error;
        }
    }

    getPythonScript(serviceName) {
        const scriptMap = {
            youtube: 'funcs/apis/ytsearchapi.py',
            youtubeMusic: 'funcs/apis/ytmusicsearchapi.py',
            spotify: 'funcs/apis/spotifyapi.py',
            tidal: 'funcs/apis/tidalapi.py',
            deezer: 'funcs/apis/deezerapi.py',
            qobuz: 'funcs/apis/qobuzapi.py'
        };

        const scriptName = scriptMap[serviceName] || '';
        this.log('Getting Python script for service:', { serviceName, scriptName });
        return scriptName;
    }

    getResourcePath(filename) {
        let resourcePath;
        if (this.app.isPackaged) {
            resourcePath = path.join(process.resourcesPath, 'app.asar.unpacked', 'src', filename);
        } else {
            resourcePath = path.join(__dirname, filename);
        }
        return path.normalize(resourcePath);
    }


    getDefaultSettings() {
        const defaultPath = this.app.getPath('downloads');
        this.log('Getting default settings:', { downloadLocation: defaultPath });
        return { downloadLocation: defaultPath };
    }

}

module.exports = CustomRip;

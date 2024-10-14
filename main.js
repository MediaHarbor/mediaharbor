const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('path');
const { spawn } = require('child_process');
const os = require('os');

function createWindow() {
    const win = new BrowserWindow({
        width: 1200,
        height: 800,
        frame: false,
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
    let downloadCount = 0;


    ipcMain.on('start-yt-music-download', (event, data) => {
        const { url, quality } = data;
        const ytDlpCommand = 'yt-dlp';
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
                event.reply('download-info', {
                    title: videoInfo.title,
                    uploader: videoInfo.uploader,
                    thumbnail: videoInfo.thumbnail,
                    order: downloadCount
                });

                outputLines = [];
            }
        });

        videoInfoProcess.on('exit', () => {
            const ytDlpArgs = [
                '-f', quality,
                '--output', path.join(downloadPath, '%(title)s.%(ext)s'),
                '--no-playlist',
                '--progress',
                url
            ];

            const ytDlp = spawn(ytDlpCommand, ytDlpArgs);
            ytDlp.stdout.on('data', (data) => {
                const output = data.toString();
                console.log(output);
                const progressMatch = output.match(/(\d+)\.?\d*%/);
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
                event.reply('download-debug', `Error: ${errorOutput}`);
            });

            ytDlp.on('exit', (code) => {
                if (code !== 0) {
                    event.reply('download-debug', `yt-dlp process exited with code ${code}`);
                }
            });
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

    ipcMain.on('start-deezer-download', (event, data) => {
        const { url, quality } = data;
        const ripCommand = 'custom_rip';
        const ripArgs = ['-q', quality, 'url', url];

        downloadCount++;
        event.reply('download-info', {
            title: 'Deezer Download',
            order: downloadCount
        });

        const deezerIdMatch = url.match(/\/track\/(\d+)/);
        if (deezerIdMatch) {
            const trackId = deezerIdMatch[1];
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
            if (code !== 0) {
                event.reply('download-error', `Process exited with code ${code}`);
            } else {
                event.reply('download-complete', {
                    order: downloadCount
                });
            }
        });
    });

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

            if (output.includes('Skipping track') && output.includes('Marked as downloaded')) {
                const skippedEidMatch = output.match(/Skipping track (\d+)/);
                if (skippedEidMatch) {
                    const skippedEid = skippedEidMatch[1];
                    getQobuzDetails(skippedEid, event, downloadCount);
                    event.reply('download-update', {
                        progress: 100,
                        order: downloadCount
                    });
                }
            }
        });

        ripProcess.stderr.on('data', (errorData) => {
            const errorOutput = errorData.toString();
            console.error(`Error: ${errorOutput}`);
            event.reply('download-error', `Error: ${errorOutput}`);
        });

        ripProcess.on('exit', (code) => {
            if (code !== 0) {
                event.reply('download-error', `Process exited with code ${code}`);
            } else {
                event.reply('download-complete', {
                    order: downloadCount
                });
            }
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
    ipcMain.on('start-tidal-download', (event, data) => {
        const { url, quality } = data;
        const ripCommand = 'custom_rip';
        const ripArgs = ['-q', quality, 'url', url];

        downloadCount++;
        event.reply('download-info', {
            title: 'Tidal Download',
            order: downloadCount
        });

        const tidalIdMatch = url.match(/\/track\/(\d+)/);
        if (tidalIdMatch) {
            const trackId = tidalIdMatch[1];
            getTidalDetails(trackId, event, downloadCount);
        }

        const ripProcess = spawn(ripCommand, ripArgs);
        let currentTrackId = null;

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

            if (output.includes('Skipping track') && output.includes('Marked as downloaded')) {
                const skippedTrackIdMatch = output.match(/Skipping track (\d+)/);
                if (skippedTrackIdMatch) {
                    const skippedTrackId = skippedTrackIdMatch[1];
                    getTidalDetails(skippedTrackId, event, downloadCount);
                    event.reply('download-update', {
                        progress: 100,
                        order: downloadCount
                    });
                }
            }
        });

        ripProcess.stderr.on('data', (errorData) => {
            const errorOutput = errorData.toString();
            console.error(`Error: ${errorOutput}`);
            event.reply('download-error', `Error: ${errorOutput}`);
        });

        ripProcess.on('exit', (code) => {
            if (code !== 0) {
                event.reply('download-error', `Process exited with code ${code}`);
            } else {
                event.reply('download-complete', {
                    order: downloadCount
                });
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
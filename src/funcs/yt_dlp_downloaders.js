const path = require("path");
const fs = require("fs");
const os = require("os");
const { spawn } = require("child_process");
const { app } = require("electron");
const { fetchWebsiteTitle, extractDomain, fetchHighResImageOrFavicon } = require("./fetchers");
const { resolveCommand } = require("./venvManager");
const { getNextDownloadOrder } = require('./downloadorder');
const logger = require("./logger");
let fixPath;

if (process.platform === 'darwin' || process.platform === 'linux') {
    import('fix-path').then((module) => {
        fixPath = module.default;
        fixPath();
    }).catch(() => {});
}

const activeDownloads = new Map();
function buildYtDlpMusicArgs(url, quality, settings) {
    const downloadPath = settings.downloadLocation || path.join(os.homedir(), 'Downloads');
    const args = [
        '-x',
        '--audio-format', settings.youtubeAudioExtensions || 'mp3',
        '--audio-quality', quality,
        '--output', path.join(downloadPath, settings.download_output_template || '%(title)s.%(ext)s'),
        '--no-warnings',
        '--ignore-errors',
        '--no-abort-on-error',
        '--newline',
        '--progress',
    ];

    if (settings.no_playlist) {
        args.push('--no-playlist');
    }

    if (settings.max_retries) {
        args.push('--retries', settings.max_retries.toString());
    } else {
        args.push('--retries', '10');
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

function buildYtDlpArgs(url, quality, settings, isGeneric = false) {
    const downloadPath = settings.downloadLocation || path.join(os.homedir(), 'Downloads');
    const args = [
        '-f', quality,
        '--output', path.join(downloadPath, settings.download_output_template || '%(title)s.%(ext)s'),
        '--no-warnings',
        '--ignore-errors',
        '--no-abort-on-error',
        '--newline',
        '--progress',
    ];

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
    } else {
        args.push('--retries', '10');
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

async function handleYtDlpMusicDownload(event, data, settings) {

    const { url, quality, isPlaylist, platform = 'youtubemusic' } = data;
    logger.info('download', `Starting music download: ${url}`);
    const ytDlpCommand = resolveCommand('yt-dlp');
    const downloadId = getNextDownloadOrder();

    activeDownloads.set(downloadId, {
        url,
        type: 'music',
        infoFetched: false,
        hasError: false,
        fullOutput: ''
    });

    const videoInfoArgs = [
        '--print', '%(title)s',
        '--print', '%(uploader)s',
        '--print', '%(thumbnail)s',
        '--print', '%(album)s',
        '--no-download',
        url
    ];

    const videoInfoProcess = spawn(ytDlpCommand, videoInfoArgs, {
        env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
        shell: false
    });
    let videoInfo = { title: '', uploader: '', thumbnail: '', album: '' };
    let outputLines = [];

    videoInfoProcess.stdout.on('data', (data) => {
        const output = data.toString().split('\n').filter(line => line.trim());
        outputLines = outputLines.concat(output);

        if (outputLines.length >= 3) {
            videoInfo.title = outputLines[0].trim();
            videoInfo.uploader = outputLines[1].trim();
            videoInfo.thumbnail = outputLines[2].trim();
            videoInfo.album = (outputLines[3] || '').trim();

            if (!videoInfo.thumbnail.startsWith('http')) {
                videoInfo.thumbnail = 'https:' + videoInfo.thumbnail;
            }

            const downloadState = activeDownloads.get(downloadId);
            if (downloadState && !downloadState.infoFetched) {
                downloadState.infoFetched = true;

                event.reply('download-info', {
                    title: videoInfo.title,
                    artist: videoInfo.uploader,
                    thumbnail: videoInfo.thumbnail,
                    album: videoInfo.album || undefined,
                    platform,
                    quality,
                    order: downloadId
                });

                outputLines = [];
            }
        }
    });
    if (isPlaylist) {
        await handlePlaylistDownload(event, url, quality, settings, downloadId, platform);
    } else {
        videoInfoProcess.on('exit', () => {
            startMusicDownload(event, url, quality, settings, videoInfo, downloadId, platform);
        });
    }

}
async function handlePlaylistDownload(event, url, quality, settings, downloadId, platform = 'youtubemusic') {
    const ytDlpCommand = resolveCommand('yt-dlp');

    activeDownloads.set(downloadId, {
        infoFetched: false,
        totalFiles: 0,
        currentFile: 0,
        currentFileProgress: 0
    });

    const playlistInfoArgs = [
        '--flat-playlist',
        '--print', '%(playlist)s',
        '--print', '%(playlist_uploader)s',
        '--print', '%(playlist_thumbnail)s',
        '--print', '%(playlist_count)s',
        '--no-download',
        url
    ];

    const playlistInfoProcess = spawn(ytDlpCommand, playlistInfoArgs, {
        env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
        shell: false
    });
    let playlistInfo = { title: '', uploader: '', thumbnail: '', totalVideos: 0 };
    let outputLines = [];

    playlistInfoProcess.stdout.on('data', (data) => {
        const output = data.toString().split('\n').filter(line => line.trim());
        outputLines = outputLines.concat(output);

        const downloadState = activeDownloads.get(downloadId);
        if (outputLines.length >= 4 && downloadState && !downloadState.infoFetched) {
            playlistInfo.title = outputLines[0].trim();
            playlistInfo.uploader = outputLines[1].trim();
            playlistInfo.thumbnail = outputLines[2].trim();
            playlistInfo.totalVideos = parseInt(outputLines[3].trim()) || 0;

            if (!playlistInfo.thumbnail.startsWith('http')) {
                playlistInfo.thumbnail = 'https:' + playlistInfo.thumbnail;
            }

            downloadState.infoFetched = true;
            downloadState.totalFiles = playlistInfo.totalVideos;
            downloadState.currentFile = 0;
            downloadState.currentFileProgress = 0;

            event.reply('download-info', {
                title: playlistInfo.title,
                artist: playlistInfo.uploader,
                thumbnail: playlistInfo.thumbnail,
                platform,
                quality,
                order: downloadId
            });
        }
    });

    playlistInfoProcess.on('error', (error) => {
        logger.error('download', `Failed to fetch playlist info: ${error.message}`);
        event.reply('download-error', `Failed to fetch playlist info: ${error.message}`);
        activeDownloads.delete(downloadId);
    });

    await new Promise((resolve) => playlistInfoProcess.on('exit', resolve));

    if (!activeDownloads.has(downloadId)) {
        return;
    }

    const playlistDir = path.join(settings.downloadLocation, sanitizeFileName(playlistInfo.title));
    if (!fs.existsSync(playlistDir)) {
        fs.mkdirSync(playlistDir, { recursive: true });
    }

    const playlistSettings = {
        ...settings,
        downloadLocation: playlistDir,
        output_template: '%(playlist_index)s - %(title)s.%(ext)s',
        no_playlist: false
    };

    const downloadArgs = buildYtDlpMusicArgs(url, quality, playlistSettings);
    const noPlaylistIndex = downloadArgs.indexOf('--no-playlist');
    if (noPlaylistIndex !== -1) {
        downloadArgs.splice(noPlaylistIndex, 1);
    }
    downloadArgs.push('--yes-playlist');

    const ytDlp = spawn(ytDlpCommand, downloadArgs, {
        env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
        shell: false
    });
    let lastProgressUpdate = Date.now();
    const downloadStateRef = activeDownloads.get(downloadId);
    if (downloadStateRef) downloadStateRef.process = ytDlp;

    ytDlp.stdout.on('data', (data) => {
        const output = data.toString();
        lastProgressUpdate = Date.now();

        const downloadState = activeDownloads.get(downloadId);
        if (!downloadState) return;

        const fileMatch = output.match(/\[download\] Downloading item (\d+) of (\d+)/);
        if (fileMatch) {
            const currentFile = parseInt(fileMatch[1]);
            const totalFiles = parseInt(fileMatch[2]);
            downloadState.currentFile = currentFile;
            downloadState.totalFiles = totalFiles;
            downloadState.currentFileProgress = 0;
        }

        const progressMatch = output.match(/\[download\]\s+(\d+\.?\d*)%/);
        if (progressMatch && downloadState) {
            downloadState.currentFileProgress = parseFloat(progressMatch[1]);

            const totalProgress = calculateTotalProgress(downloadState);

            event.reply('download-update', {
                progress: totalProgress,
                title: playlistInfo.title,
                artist: playlistInfo.uploader,
                thumbnail: playlistInfo.thumbnail,
                order: downloadId
            });
        }

        if (output.includes('[ExtractAudio] Destination:')) {
            if (downloadState) {
                downloadState.currentFileProgress = 100;
                const totalProgress = calculateTotalProgress(downloadState);

                event.reply('download-update', {
                    progress: totalProgress,
                    title: playlistInfo.title,
                    artist: playlistInfo.uploader,
                    thumbnail: playlistInfo.thumbnail,
                    order: downloadId
                });
            }
        }
    });

    function calculateTotalProgress(downloadState) {
        const { currentFile, totalFiles, currentFileProgress } = downloadState;
        const filesCompleted = (currentFile - 1) * 100;
        const currentProgress = currentFileProgress;
        return (filesCompleted + currentProgress) / totalFiles;
    }

    ytDlp.stderr.on('data', (data) => {
        const error = data.toString();
        if (!error.includes('YouTube Music is not directly supported')) {
            logger.warn('download', `Music playlist stderr: ${error.trim()}`);
            event.reply('download-error', error);
        }
        lastProgressUpdate = Date.now();
    });

    ytDlp.on('error', (error) => {
        clearInterval(stallCheckInterval);
        logger.error('download', `yt-dlp process crashed during music playlist: ${error.message}`);
        event.reply('download-error', `yt-dlp process crashed: ${error.message}`);
        activeDownloads.delete(downloadId);
    });

    const stallCheckInterval = setInterval(() => {
        if (Date.now() - lastProgressUpdate > 60000) {
            clearInterval(stallCheckInterval);
            ytDlp.kill();
            logger.warn('download', `Download stalled, killing process: ${playlistInfo.title}`);
            event.reply('download-error', `Download stalled for ${playlistInfo.title}`);
            activeDownloads.delete(downloadId);
        }
    }, 10000);

    ytDlp.on('exit', async (code) => {
        clearInterval(stallCheckInterval);

        if (code === 0) {
            const files = fs.readdirSync(playlistDir)
                .filter(file => file.endsWith('.mp3'))
                .sort((a, b) => {
                    const numA = parseInt(a.split(' ')[0]);
                    const numB = parseInt(b.split(' ')[0]);
                    return numA - numB;
                });

            const m3uContent = '#EXTM3U\n' + files.map(file => {
                return `#EXTINF:-1,${file.substring(file.indexOf(' - ') + 3, file.lastIndexOf('.'))}\n${file}`;
            }).join('\n');

            fs.writeFileSync(path.join(playlistDir, `${sanitizeFileName(playlistInfo.title)}.m3u`), m3uContent);

            const downloadInfo = {
                downloadName: playlistInfo.title,
                downloadArtistOrUploader: playlistInfo.uploader,
                downloadLocation: playlistDir,
                downloadThumbnail: playlistInfo.thumbnail
            };
            logger.info('download', `Music playlist download complete: ${playlistInfo.title}`);
            event.reply('download-complete', { order: downloadId });
        } else {
            logger.error('download', `Music playlist download failed with exit code ${code}: ${playlistInfo.title}`);
            event.reply('download-error', `Playlist download failed with code ${code}`);
        }
        activeDownloads.delete(downloadId);
    });
}
function sanitizeFileName(filename) {
    return filename.replace(/[<>:"/\\|?*]/g, '_');
}

function startMusicDownload(event, url, quality, settings, videoInfo, downloadId, platform = 'youtubemusic') {
    const ytDlpCommand = resolveCommand('yt-dlp');
    const args = buildYtDlpMusicArgs(url, quality, settings);

    let hasStarted = false;
    let isAlreadyDownloaded = false;
    let lastProgressUpdate = Date.now();

    const ytDlp = spawn(ytDlpCommand, args, {
        env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
        shell: false
    });

    const downloadState = activeDownloads.get(downloadId);
    if (downloadState) downloadState.process = ytDlp;

    ytDlp.stdout.on('data', (data) => {
        const output = data.toString();

        if (output.includes('has already been downloaded')) {
            isAlreadyDownloaded = true;
            event.reply('download-update', {
                progress: 100,
                title: videoInfo.title,
                artist: videoInfo.uploader,
                thumbnail: videoInfo.thumbnail,
                order: downloadId
            });
            return;
        }

        const progressMatch = output.match(/(\d+\.\d+)%/);
        if (progressMatch) {
            hasStarted = true;
            lastProgressUpdate = Date.now();
            const progress = parseFloat(progressMatch[1]);
            event.reply('download-update', {
                progress,
                title: videoInfo.title,
                artist: videoInfo.uploader,
                thumbnail: videoInfo.thumbnail,
                order: downloadId
            });
        }
    });

    const stallCheckInterval = setInterval(() => {
        if (hasStarted && !isAlreadyDownloaded && Date.now() - lastProgressUpdate > 10000) {
            clearInterval(stallCheckInterval);
            ytDlp.kill();
            logger.warn('download', `Download stalled, killing process: ${videoInfo.title}`);
            event.reply('download-error', `Download stalled for ${videoInfo.title}`);
            activeDownloads.delete(downloadId);
        }
    }, 5000);

    ytDlp.stderr.on('data', (errorData) => {
        const errorOutput = errorData.toString();
        const downloadState = activeDownloads.get(downloadId);
        if (downloadState) {
            downloadState.hasError = true;
            downloadState.errorLog = (downloadState.errorLog || '') + errorOutput;
        }
        if (!isAlreadyDownloaded) {
            logger.warn('download', `Music download stderr: ${errorOutput.trim()}`);
        }
    });

    ytDlp.on('exit', (code) => {
        clearInterval(stallCheckInterval);
        const downloadState = activeDownloads.get(downloadId);

        if (code === 0 || isAlreadyDownloaded) {
            event.reply('download-complete', { order: downloadId });
        } else if (!isAlreadyDownloaded) {
            const exitMsg = `Process exited with code ${code}`;

            if (downloadState) {
                downloadState.hasError = true;
                downloadState.errorLog += `\n${exitMsg}`;
            }

            logger.error('download', `Music download failed: ${exitMsg} - ${downloadState?.errorLog || exitMsg}`);
            event.reply('download-error', {
                order: downloadId,
                error: exitMsg,
                fullLog: downloadState?.errorLog || exitMsg
            });
        }
        activeDownloads.delete(downloadId);
    });
}

async function handleYtDlpDownload(event, data, settings, isGeneric = false) {
    const { url, quality, isPlaylist, platform = isGeneric ? 'generic' : 'youtube' } = data;
    logger.info('download', `Starting video download: ${url}`);
    const ytDlpCommand = resolveCommand('yt-dlp');
    const downloadId = getNextDownloadOrder();
    if (!isPlaylist){
        activeDownloads.set(downloadId, {
            url,
            type: isGeneric ? 'generic' : 'video',
            infoFetched: false
        });

        if (!isGeneric) {
            const videoInfoArgs = [
                '--print', '%(title)s',
                '--print', '%(uploader)s',
                '--print', '%(thumbnail)s',
                '--print', '%(album)s',
                '--no-download',
                url
            ];

            const videoInfoProcess = spawn(ytDlpCommand, videoInfoArgs, {
                env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
                shell: false
            });
            let videoInfo = { title: '', uploader: '', thumbnail: '', album: '' };
            let outputLines = [];

            videoInfoProcess.stdout.on('data', (data) => {
                const output = data.toString().split('\n').filter(line => line.trim());
                outputLines = outputLines.concat(output);

                if (outputLines.length >= 3) {
                    videoInfo.title = outputLines[0].trim();
                    videoInfo.uploader = outputLines[1].trim();
                    videoInfo.thumbnail = outputLines[2].trim();
                    videoInfo.album = (outputLines[3] || '').trim();

                    if (!videoInfo.thumbnail.startsWith('http')) {
                        videoInfo.thumbnail = 'https:' + videoInfo.thumbnail;
                    }

                    const downloadState = activeDownloads.get(downloadId);
                    if (downloadState && !downloadState.infoFetched) {
                        downloadState.infoFetched = true;

                        event.reply('download-info', {
                            title: videoInfo.title,
                            artist: videoInfo.uploader,
                            thumbnail: videoInfo.thumbnail,
                            album: videoInfo.album || undefined,
                            platform,
                            quality,
                            order: downloadId
                        });

                        outputLines = [];
                    }
                }
            });

            videoInfoProcess.on('exit', () => {
                if (videoInfo.title) {
                    startDownload(event, url, quality, settings, videoInfo, downloadId, isGeneric, platform);
                } else {
                    logger.error('download', `Failed to fetch video info for ${url}`);
                    event.reply('download-error', `Failed to fetch video info for ${url}`);
                    activeDownloads.delete(downloadId);
                }
            });
        } else {
            startDownload(event, url, quality, settings, null, downloadId, isGeneric, platform);
        }
    }
    else {
        await handleVideoPlaylistDownload(event, url, quality, settings, downloadId, platform);
    }
}

async function startDownload(event, url, quality, settings, videoInfo = null, downloadId, isGeneric = false, platform = 'youtube') {
    const ytDlpCommand = resolveCommand('yt-dlp');
    const args = buildYtDlpArgs(url, quality, settings, isGeneric);

    const metadata = {
        title: videoInfo ? videoInfo.title : await fetchWebsiteTitle(url),
        artist: videoInfo ? videoInfo.uploader : extractDomain(url),
        thumbnail: videoInfo ? videoInfo.thumbnail : await fetchHighResImageOrFavicon(url),
        domain: extractDomain(url)
    };

    metadata.title = metadata.title || 'Unknown';
    metadata.artist = metadata.artist || 'Unknown';
    metadata.thumbnail = metadata.thumbnail || 'Unknown';

    const downloadState = activeDownloads.get(downloadId);
    if (isGeneric && downloadState && !downloadState.infoFetched) {
        downloadState.infoFetched = true;
        event.reply('download-info', {
            url,
            order: downloadId,
            platform,
            quality,
            ...metadata
        });
    }

    const ytDlp = spawn(ytDlpCommand, args, {
        env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
        shell: false
    });

    if (downloadState) downloadState.process = ytDlp;

    let fullOutput = '';

    ytDlp.stdout.on('data', (data) => {
        const output = data.toString();
        fullOutput += output;

        const progressMatch = output.match(/(\d+\.\d+)%/);
        if (progressMatch) {
            const progress = parseFloat(progressMatch[1]);
            event.reply('download-update', {
                progress,
                order: downloadId,
                ...metadata
            });
        }
    });

    ytDlp.stderr.on('data', (errorData) => {
        const errorOutput = errorData.toString();
        fullOutput += errorOutput;

        logger.warn('download', `Video download stderr: ${errorOutput.trim()}`);
        event.reply('download-error', {
            order: downloadId,
            error: errorOutput,
            fullLog: fullOutput
        });
    });

    ytDlp.on('exit', (code) => {
        if (code !== 0) {
            fullOutput += `\n[MediaHarbor] Process exited with code ${code}`;

            logger.error('download', `Video download failed with exit code ${code}: ${metadata.title}`);
            event.reply('download-error', {
                order: downloadId,
                error: `Process exited with code ${code}`,
                fullLog: fullOutput
            });
        } else {
            const downloadInfo = {
                downloadName: metadata.title,
                downloadArtistOrUploader: metadata.artist,
                downloadLocation: settings.downloadLocation || app.getPath('downloads'),
                downloadThumbnail: metadata.thumbnail,
                downloadDomain: metadata.domain
            };
            logger.info('download', `Download complete: ${metadata.title}`);
            event.reply('download-complete', {
                order: downloadId
            });
        }
        activeDownloads.delete(downloadId);
    });
}
async function handleVideoPlaylistDownload(event, url, quality, settings, downloadId, platform = 'youtube') {
    const ytDlpCommand = resolveCommand('yt-dlp');

    activeDownloads.set(downloadId, {
        url,
        type: 'video_playlist',
        infoFetched: false
    });

    const playlistInfoArgs = [
        '--flat-playlist',
        '--print', '%(playlist_title)s',
        '--print', '%(playlist_uploader)s',
        '--print', '%(playlist_thumbnail)s',
        '--print', '%(playlist_count)s',
        '--no-download',
        url
    ];

    const playlistInfoProcess = spawn(ytDlpCommand, playlistInfoArgs, {
        env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
        shell: false
    });
    let playlistInfo = { title: '', uploader: '', thumbnail: '', totalVideos: 0 };
    let outputLines = [];

    playlistInfoProcess.stdout.on('data', (data) => {
        const output = data.toString().split('\n').filter(line => line.trim());
        outputLines = outputLines.concat(output);

        if (outputLines.length >= 4) {
            playlistInfo.title = outputLines[0].trim();
            playlistInfo.uploader = outputLines[1].trim();
            playlistInfo.thumbnail = outputLines[2].trim();
            playlistInfo.totalVideos = parseInt(outputLines[3].trim()) || 0;

            if (!playlistInfo.thumbnail.startsWith('http')) {
                playlistInfo.thumbnail = 'https:' + playlistInfo.thumbnail;
            }

            const downloadState = activeDownloads.get(downloadId);
            if (downloadState && !downloadState.infoFetched) {
                downloadState.infoFetched = true;
                downloadState.totalFiles = playlistInfo.totalVideos;
                downloadState.currentFile = 0;
                downloadState.currentFileProgress = 0;
                event.reply('download-info', {
                    title: playlistInfo.title,
                    artist: playlistInfo.uploader,
                    thumbnail: playlistInfo.thumbnail,
                    platform,
                    quality,
                    order: downloadId
                });
            }
        }
    });

    playlistInfoProcess.on('exit', (code) => {
        if (code === 0) {
            startVideoPlaylistDownload(event, url, quality, settings, playlistInfo, downloadId, platform);
        } else {
            logger.error('download', `Failed to fetch video playlist info for ${url}`);
            event.reply('download-error', `Failed to fetch playlist info for ${url}`);
            activeDownloads.delete(downloadId);
        }
    });
}

async function startVideoPlaylistDownload(event, url, quality, settings, playlistInfo, downloadId, platform = 'youtube') {
    logger.info('download', `Starting video playlist download: ${playlistInfo.title} (${url})`);
    const ytDlpCommand = resolveCommand('yt-dlp');

    const downloadState = activeDownloads.get(downloadId);

    if (!downloadState) return;

    downloadState.infoFetched = true;
    downloadState.totalFiles = playlistInfo.totalVideos;
    downloadState.currentFile = 0;
    downloadState.currentFileProgress = 0;

    const playlistDir = path.join(settings.downloadLocation || app.getPath('downloads'),
        sanitizeFileName(playlistInfo.title));
    if (!fs.existsSync(playlistDir)) {
        fs.mkdirSync(playlistDir, { recursive: true });
    }

    const playlistSettings = {
        ...settings,
        downloadLocation: playlistDir,
        output_template: '%(playlist_index)s - %(title)s.%(ext)s',
        no_playlist: false
    };

    const args = buildYtDlpArgs(url, quality, playlistSettings);
    const noPlaylistIndex = args.indexOf('--no-playlist');
    if (noPlaylistIndex !== -1) {
        args.splice(noPlaylistIndex, 1);
    }
    args.push('--yes-playlist');

    const ytDlp = spawn(ytDlpCommand, args, {
        env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
        shell: false
    });

    const downloadState2 = activeDownloads.get(downloadId);
    if (downloadState2) downloadState2.process = ytDlp;

    ytDlp.stdout.on('data', (data) => {
        const output = data.toString();
        lastProgressUpdate = Date.now();

        const downloadState = activeDownloads.get(downloadId);
        if (!downloadState) return;

        const fileMatch = output.match(/\[download\] Downloading (video|item) (\d+) of (\d+)/);
        if (fileMatch) {
            const currentFile = parseInt(fileMatch[2]);
            const totalFiles = parseInt(fileMatch[3]);
            downloadState.currentFile = currentFile;
            downloadState.totalFiles = totalFiles;
            downloadState.currentFileProgress = 0;
        }

        const progressMatch = output.match(/\[download\]\s+([\d\.]+)%/);
        if (progressMatch && downloadState) {
            downloadState.currentFileProgress = parseFloat(progressMatch[1]);

            const totalProgress = calculateTotalProgress(downloadState);

            event.reply('download-update', {
                progress: totalProgress,
                title: playlistInfo.title,
                artist: playlistInfo.uploader,
                thumbnail: playlistInfo.thumbnail,
                order: downloadId
            });
        } else {
            const fragmentMatch = output.match(/Fragment\s+(\d+)\s+of\s+(\d+)/i);
            if (fragmentMatch && downloadState) {
                const currentFragment = parseInt(fragmentMatch[1]);
                const totalFragments = parseInt(fragmentMatch[2]);

                downloadState.currentFileProgress = (currentFragment / totalFragments) * 100;

                const totalProgress = calculateTotalProgress(downloadState);

                event.reply('download-update', {
                    progress: totalProgress,
                    title: playlistInfo.title,
                    artist: playlistInfo.uploader,
                    thumbnail: playlistInfo.thumbnail,
                    order: downloadId
                });
            }
        }
    });

    function calculateTotalProgress(downloadState) {
        const { currentFile, totalFiles, currentFileProgress } = downloadState;
        const filesCompleted = (currentFile - 1) * 100;
        const currentProgress = currentFileProgress;
        return (filesCompleted + currentProgress) / totalFiles;
    }

    ytDlp.stderr.on('data', (errorData) => {
        const errorOutput = errorData.toString();
        logger.warn('download', `Video playlist stderr: ${errorOutput.trim()}`);
        event.reply('download-error', `Error: ${errorOutput}`);
        lastProgressUpdate = Date.now();
    });

    ytDlp.on('error', (error) => {
        clearInterval(stallCheckInterval);
        logger.error('download', `yt-dlp process crashed during video playlist: ${error.message}`);
        event.reply('download-error', `yt-dlp process crashed: ${error.message}`);
        activeDownloads.delete(downloadId);
    });

    const stallCheckInterval = setInterval(() => {
        if (Date.now() - lastProgressUpdate > 60000) {
            clearInterval(stallCheckInterval);
            ytDlp.kill();
            logger.warn('download', `Download stalled, killing process: ${playlistInfo.title}`);
            event.reply('download-error', `Download stalled for ${playlistInfo.title}`);
            activeDownloads.delete(downloadId);
        }
    }, 10000);

    ytDlp.on('exit', (code) => {
        clearInterval(stallCheckInterval);
        if (code === 0) {
            const files = fs.readdirSync(playlistDir)
                .filter(file => file.endsWith('.mp4') || file.endsWith('.mkv') || file.endsWith('.webm'))
                .sort((a, b) => {
                    const numA = parseInt(a.split(' ')[0]);
                    const numB = parseInt(b.split(' ')[0]);
                    return numA - numB;
                });

            const m3uContent = '#EXTM3U\n' + files.map(file => {
                const title = file.substring(file.indexOf(' - ') + 3, file.lastIndexOf('.'));
                return `#EXTINF:-1,${title}\n${file}`;
            }).join('\n');

            fs.writeFileSync(
                path.join(playlistDir, `${sanitizeFileName(playlistInfo.title)}.m3u8`),
                m3uContent,
                { encoding: 'utf8' }
            );

            const downloadInfo = {
                downloadName: playlistInfo.title,
                downloadArtistOrUploader: playlistInfo.uploader,
                downloadLocation: playlistDir,
                downloadThumbnail: playlistInfo.thumbnail
            };
            logger.info('download', `Video playlist download complete: ${playlistInfo.title}`);
            event.reply('download-complete', { order: downloadId });
        } else {
            logger.error('download', `Video playlist download failed with exit code ${code}: ${playlistInfo.title}`);
            event.reply('download-error', `Process exited with code ${code}`);
        }
        activeDownloads.delete(downloadId);
    });
}
function cancelDownload(downloadId) {
    const state = activeDownloads.get(downloadId);
    if (state) {
        if (state.process) state.process.kill();
        activeDownloads.delete(downloadId);
    }
}

module.exports = { handleYtDlpDownload, handleYtDlpMusicDownload, cancelDownload };

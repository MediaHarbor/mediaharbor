const path = require("path");
const fs = require("fs");
const os = require("os");
const { spawn } = require("child_process");
const { app } = require("electron");
const { saveDownloadToDatabase } = require("./db");
const { fetchWebsiteTitle, extractDomain, fetchHighResImageOrFavicon } = require("./fetchers");
const { getNextDownloadOrder } = require('./downloadorder');
let fixPath;
let downloadCount = 0;

if (process.platform === 'darwin' || process.platform === 'linux') {
    import('fix-path').then((module) => {
        fixPath = module.default;
        fixPath();
    }).catch(err => console.error('Failed to import fix-path:', err));
}

const activeDownloads = new Map();
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

async function handleYtDlpMusicDownload(event, data, settings) {

    const { url, quality, isPlaylist  } = data;
    const ytDlpCommand = 'yt-dlp';
    const downloadId = getNextDownloadOrder();

    activeDownloads.set(downloadId, {
        url,
        type: 'music',
        infoFetched: false
    });

    // Get video info first
    const videoInfoArgs = [
        '--print', '%(title)s',
        '--print', '%(uploader)s',
        '--print', '%(thumbnail)s',
        '--no-download',
        url
    ];

    const videoInfoProcess = spawn(ytDlpCommand, videoInfoArgs, {
        env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
        shell: process.platform === 'linux'
    });
    let videoInfo = { title: '', uploader: '', thumbnail: '' };
    let outputLines = [];

    videoInfoProcess.stdout.on('data', (data) => {
        const output = data.toString().split('\n').filter(line => line.trim());
        outputLines = outputLines.concat(output);

        if (outputLines.length >= 3 && !activeDownloads.get(downloadId)?.infoFetched) {
            videoInfo.title = outputLines[0].trim();
            videoInfo.uploader = outputLines[1].trim();
            videoInfo.thumbnail = outputLines[2].trim();

            if (!videoInfo.thumbnail.startsWith('http')) {
                videoInfo.thumbnail = 'https:' + videoInfo.thumbnail;
            }

            activeDownloads.get(downloadId).infoFetched = true;

            event.reply('youtube-music-info', {
                title: videoInfo.title,
                uploader: videoInfo.uploader,
                thumbnail: videoInfo.thumbnail,
                order: downloadId
            });

            outputLines = [];
        }
    });
    if (isPlaylist) {
        await handlePlaylistDownload(event, url, quality, settings, downloadId);
    } else {
        videoInfoProcess.on('exit', () => {
            if (videoInfo.title) {
                startMusicDownload(event, url, quality, settings, videoInfo, downloadId);
            } else {
                event.reply('download-error', `Failed to fetch video info for ${url}`);
                activeDownloads.delete(downloadId);
            }
        });
    }

}
async function handlePlaylistDownload(event, url, quality, settings, downloadId) {
    const ytDlpCommand = 'yt-dlp';

    // Initialize download state first
    activeDownloads.set(downloadId, {
        infoFetched: false,
        totalFiles: 0,
        currentFile: 0,
        currentFileProgress: 0  // Add this to track individual file progress
    });

    // First, get playlist info with thumbnail and total videos
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
        shell: process.platform === 'linux'
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

            event.reply('youtube-music-info', {
                title: playlistInfo.title,
                uploader: playlistInfo.uploader,
                thumbnail: playlistInfo.thumbnail,
                order: downloadId
            });
        }
    });

    // Error handler for playlist info process
    playlistInfoProcess.on('error', (error) => {
        console.error('Playlist info process error:', error);
        event.reply('download-error', `Failed to fetch playlist info: ${error.message}`);
        activeDownloads.delete(downloadId);
    });

    await new Promise((resolve) => playlistInfoProcess.on('exit', resolve));

    // Check if the download was cancelled during info fetch
    if (!activeDownloads.has(downloadId)) {
        return;
    }

    // Setup download directory
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
        shell: process.platform === 'linux'
    });
    let lastProgressUpdate = Date.now();

    ytDlp.stdout.on('data', (data) => {
        const output = data.toString();
        console.log(output);
        lastProgressUpdate = Date.now();

        const downloadState = activeDownloads.get(downloadId);
        if (!downloadState) return;

        // Update current file number when starting a new file
        const fileMatch = output.match(/\[download\] Downloading item (\d+) of (\d+)/);
        if (fileMatch) {
            const currentFile = parseInt(fileMatch[1]);
            const totalFiles = parseInt(fileMatch[2]);
            downloadState.currentFile = currentFile;
            downloadState.totalFiles = totalFiles;
            downloadState.currentFileProgress = 0; // Reset progress for new file
        }

        // Update progress percentage for current file
        const progressMatch = output.match(/\[download\]\s+(\d+\.?\d*)%/);
        if (progressMatch && downloadState) {
            downloadState.currentFileProgress = parseFloat(progressMatch[1]);

            // Calculate total progress
            const totalProgress = calculateTotalProgress(downloadState);

            event.reply('download-update', {
                progress: totalProgress,
                title: playlistInfo.title,
                uploader: playlistInfo.uploader,
                thumbnail: playlistInfo.thumbnail,
                order: downloadId
            });
        }

        // Handle audio extraction progress
        if (output.includes('[ExtractAudio] Destination:')) {
            if (downloadState) {
                downloadState.currentFileProgress = 100;
                const totalProgress = calculateTotalProgress(downloadState);

                event.reply('download-update', {
                    progress: totalProgress,
                    title: playlistInfo.title,
                    uploader: playlistInfo.uploader,
                    thumbnail: playlistInfo.thumbnail,
                    order: downloadId
                });
            }
        }
    });

    // Helper function to calculate total progress
    function calculateTotalProgress(downloadState) {
        const { currentFile, totalFiles, currentFileProgress } = downloadState;
        const filesCompleted = (currentFile - 1) * 100;
        const currentProgress = currentFileProgress;
        return (filesCompleted + currentProgress) / totalFiles;
    }

    ytDlp.stderr.on('data', (data) => {
        const error = data.toString();
        console.error(`Error: ${error}`);
        if (!error.includes('YouTube Music is not directly supported')) {
            event.reply('download-error', error);
        }
        lastProgressUpdate = Date.now();
    });

    const stallCheckInterval = setInterval(() => {
        if (Date.now() - lastProgressUpdate > 30000) {
            clearInterval(stallCheckInterval);
            ytDlp.kill();
            event.reply('download-error', `Download stalled for ${playlistInfo.title}`);
            activeDownloads.delete(downloadId);
        }
    }, 5000);

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
            saveDownloadToDatabase(downloadInfo);
            event.reply('download-complete', { order: downloadId });
        } else {
            event.reply('download-error', `Playlist download failed with code ${code}`);
        }
        activeDownloads.delete(downloadId);
    });
}
function sanitizeFileName(filename) {
    return filename.replace(/[<>:"/\\|?*]/g, '_');
}

function startMusicDownload(event, url, quality, settings, videoInfo, downloadId) {
    const ytDlpCommand = 'yt-dlp';
    const args = buildYtDlpMusicArgs(url, quality, settings);

    let hasStarted = false;
    let isAlreadyDownloaded = false;
    let lastProgressUpdate = Date.now();

    const ytDlp = spawn(ytDlpCommand, args, {
        env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
        shell: process.platform === 'linux'
    });

    ytDlp.stdout.on('data', (data) => {
        const output = data.toString();
        console.log(output);

        // Check for "has already been downloaded" message
        if (output.includes('has already been downloaded')) {
            isAlreadyDownloaded = true;
            // Send 100% progress immediately for already downloaded files
            event.reply('download-update', {
                progress: 100,
                title: videoInfo.title,
                uploader: videoInfo.uploader,
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
                uploader: videoInfo.uploader,
                thumbnail: videoInfo.thumbnail,
                order: downloadId
            });
        }
    });

    // Set a timer to check for stalled downloads
    const stallCheckInterval = setInterval(() => {
        if (hasStarted && !isAlreadyDownloaded && Date.now() - lastProgressUpdate > 10000) {
            clearInterval(stallCheckInterval);
            ytDlp.kill();
            event.reply('download-error', `Download stalled for ${videoInfo.title}`);
            activeDownloads.delete(downloadId);
        }
    }, 5000);

    ytDlp.stderr.on('data', (errorData) => {
        const errorOutput = errorData.toString();
        console.error(`Error: ${errorOutput}`);
        if (!isAlreadyDownloaded) {  // Don't send error if file was already downloaded
            event.reply('download-error', `Error: ${errorOutput}`);
        }
    });

    ytDlp.on('exit', (code) => {
        clearInterval(stallCheckInterval);

        if (code === 0 || isAlreadyDownloaded) {
            const downloadInfo = {
                downloadName: videoInfo.title,
                downloadArtistOrUploader: videoInfo.uploader,
                downloadLocation: settings.downloadLocation || app.getPath('downloads'),
                downloadThumbnail: videoInfo.thumbnail
            };
            saveDownloadToDatabase(downloadInfo);
            event.reply('download-complete', { order: downloadId });
        } else if (!isAlreadyDownloaded) {  // Don't send error if file was already downloaded
            event.reply('download-error', `Process exited with code ${code}`);
        }
        activeDownloads.delete(downloadId);
    });
}

async function handleYtDlpDownload(event, data, settings, isGeneric = false) {
    const { url, quality, isPlaylist } = data;
    const ytDlpCommand = 'yt-dlp';
    const downloadId = getNextDownloadOrder();
    if (!isPlaylist){
        // Store initial state in activeDownloads
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
                '--no-download',
                url
            ];

            const videoInfoProcess = spawn(ytDlpCommand, videoInfoArgs, {
                env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
                shell: process.platform === 'linux'
            });
            let videoInfo = { title: '', uploader: '', thumbnail: '' };
            let outputLines = [];

            videoInfoProcess.stdout.on('data', (data) => {
                const output = data.toString().split('\n').filter(line => line.trim());
                outputLines = outputLines.concat(output);

                if (outputLines.length >= 3 && !activeDownloads.get(downloadId).infoFetched) {
                    videoInfo.title = outputLines[0].trim();
                    videoInfo.uploader = outputLines[1].trim();
                    videoInfo.thumbnail = outputLines[2].trim();

                    if (!videoInfo.thumbnail.startsWith('http')) {
                        videoInfo.thumbnail = 'https:' + videoInfo.thumbnail;
                    }

                    // Mark that we've fetched info for this download
                    activeDownloads.get(downloadId).infoFetched = true;

                    event.reply('youtube-video-info', {
                        title: videoInfo.title,
                        uploader: videoInfo.uploader,
                        thumbnail: videoInfo.thumbnail,
                        order: downloadId
                    });

                    outputLines = [];
                }
            });

            videoInfoProcess.on('exit', () => {
                if (videoInfo.title) {
                    startDownload(event, url, quality, settings, videoInfo, downloadId, isGeneric);
                } else {
                    event.reply('download-error', `Failed to fetch video info for ${url}`);
                    activeDownloads.delete(downloadId);
                }
            });
        } else {
            startDownload(event, url, quality, settings, null, downloadId, isGeneric);
        }
    }
    else {
        await handleVideoPlaylistDownload(event, url, quality, settings, downloadId);
    }
}

async function startDownload(event, url, quality, settings, videoInfo = null, downloadId, isGeneric = false) {
    const ytDlpCommand = 'yt-dlp';
    const args = buildYtDlpArgs(url, quality, settings, isGeneric);

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

    if (isGeneric && !activeDownloads.get(downloadId).infoFetched) {
        activeDownloads.get(downloadId).infoFetched = true;
        event.reply('generic-video-info', {
            url,
            order: downloadId,
            ...metadata
        });
    }

    const ytDlp = spawn(ytDlpCommand, args, {
        env: { ...process.env, PYTHONIOENCODING: 'utf-8' },
        shell: process.platform === 'linux'
    });

    ytDlp.stdout.on('data', (data) => {
        const output = data.toString();
        console.log(output);
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
                order: downloadId
            });
        }
        activeDownloads.delete(downloadId);
    });
}
async function handleVideoPlaylistDownload(event, url, quality, settings, downloadId) {
    const ytDlpCommand = 'yt-dlp';

    activeDownloads.set(downloadId, {
        url,
        type: 'video_playlist',
        infoFetched: false
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
        shell: process.platform === 'linux'
    });
    let playlistInfo = { title: '', uploader: '', thumbnail: '', totalVideos: 0 };
    let outputLines = [];

    playlistInfoProcess.stdout.on('data', (data) => {
        const output = data.toString().split('\n').filter(line => line.trim());
        outputLines = outputLines.concat(output);

        if (outputLines.length >= 4 && !activeDownloads.get(downloadId)?.infoFetched) {
            playlistInfo.title = outputLines[0].trim();
            playlistInfo.uploader = outputLines[1].trim();
            playlistInfo.thumbnail = outputLines[2].trim();
            playlistInfo.totalVideos = parseInt(outputLines[3].trim()) || 0;

            activeDownloads.get(downloadId).infoFetched = true;

            event.reply('video-playlist-info', {
                title: playlistInfo.title,
                uploader: playlistInfo.uploader,
                thumbnail: playlistInfo.thumbnail,
                order: downloadId
            });
        }
    });

    playlistInfoProcess.on('exit', () => {
        startVideoPlaylistDownload(event, url, quality, settings, playlistInfo, downloadId);
    });
}

async function handleVideoPlaylistDownload(event, url, quality, settings, downloadId) {
    const ytDlpCommand = 'yt-dlp';

    activeDownloads.set(downloadId, {
        url,
        type: 'video_playlist',
        infoFetched: false
    });

    // Fetch playlist info
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
        shell: process.platform === 'linux'
    });
    let playlistInfo = { title: '', uploader: '', thumbnail: '', totalVideos: 0 };
    let outputLines = [];

    playlistInfoProcess.stdout.on('data', (data) => {
        const output = data.toString().split('\n').filter(line => line.trim());
        outputLines = outputLines.concat(output);

        if (outputLines.length >= 4 && !activeDownloads.get(downloadId)?.infoFetched) {
            playlistInfo.title = outputLines[0].trim();
            playlistInfo.uploader = outputLines[1].trim();
            playlistInfo.thumbnail = outputLines[2].trim();
            playlistInfo.totalVideos = parseInt(outputLines[3].trim()) || 0;

            if (!playlistInfo.thumbnail.startsWith('http')) {
                playlistInfo.thumbnail = 'https:' + playlistInfo.thumbnail;
            }

            activeDownloads.get(downloadId).infoFetched = true;
            activeDownloads.get(downloadId).totalFiles = playlistInfo.totalVideos;
            activeDownloads.get(downloadId).currentFile = 0;
            activeDownloads.get(downloadId).currentFileProgress = 0;

            event.reply('video-playlist-info', {
                title: playlistInfo.title,
                uploader: playlistInfo.uploader,
                thumbnail: playlistInfo.thumbnail,
                order: downloadId
            });
        }
    });

    playlistInfoProcess.on('exit', (code) => {
        if (code === 0) {
            startVideoPlaylistDownload(event, url, quality, settings, playlistInfo, downloadId);
        } else {
            event.reply('download-error', `Failed to fetch playlist info for ${url}`);
            activeDownloads.delete(downloadId);
        }
    });
}

function sanitizeFileName(filename) {
    return filename.replace(/[<>:"/\\|?*]/g, '_');
}

async function startVideoPlaylistDownload(event, url, quality, settings, playlistInfo, downloadId) {
    const ytDlpCommand = 'yt-dlp';

    // Initialize download state
    const downloadState = activeDownloads.get(downloadId);
    if (!downloadState) return;

    downloadState.infoFetched = true;
    downloadState.totalFiles = playlistInfo.totalVideos;
    downloadState.currentFile = 0;
    downloadState.currentFileProgress = 0;

    // Create playlist directory
    const playlistDir = path.join(settings.downloadLocation || app.getPath('downloads'),
        sanitizeFileName(playlistInfo.title));
    if (!fs.existsSync(playlistDir)) {
        fs.mkdirSync(playlistDir, { recursive: true });
    }

    // Update settings with new download location and output template
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
        shell: process.platform === 'linux'
    });

    let lastProgressUpdate = Date.now();

    ytDlp.stdout.on('data', (data) => {
        const output = data.toString();
        console.log(output);
        lastProgressUpdate = Date.now();

        const downloadState = activeDownloads.get(downloadId);
        if (!downloadState) return;

        // Update current file number when starting a new file
        const fileMatch = output.match(/\[download\] Downloading (video|item) (\d+) of (\d+)/);
        if (fileMatch) {
            const currentFile = parseInt(fileMatch[2]);
            const totalFiles = parseInt(fileMatch[3]);
            downloadState.currentFile = currentFile;
            downloadState.totalFiles = totalFiles;
            downloadState.currentFileProgress = 0; // Reset progress for new file
        }

        // Update progress percentage for current file
        const progressMatch = output.match(/\[download\]\s+([\d\.]+)%/);
        if (progressMatch && downloadState) {
            downloadState.currentFileProgress = parseFloat(progressMatch[1]);

            // Calculate total progress
            const totalProgress = calculateTotalProgress(downloadState);

            event.reply('download-update', {
                progress: totalProgress,
                title: playlistInfo.title,
                uploader: playlistInfo.uploader,
                thumbnail: playlistInfo.thumbnail,
                order: downloadId
            });
        } else {
            // Handle fragment downloads
            const fragmentMatch = output.match(/Fragment\s+(\d+)\s+of\s+(\d+)/i);
            if (fragmentMatch && downloadState) {
                const currentFragment = parseInt(fragmentMatch[1]);
                const totalFragments = parseInt(fragmentMatch[2]);

                // Estimate current file progress based on fragments
                downloadState.currentFileProgress = (currentFragment / totalFragments) * 100;

                // Calculate total progress
                const totalProgress = calculateTotalProgress(downloadState);

                event.reply('download-update', {
                    progress: totalProgress,
                    title: playlistInfo.title,
                    uploader: playlistInfo.uploader,
                    thumbnail: playlistInfo.thumbnail,
                    order: downloadId
                });
            }
        }
    });

    // Helper function to calculate total progress
    function calculateTotalProgress(downloadState) {
        const { currentFile, totalFiles, currentFileProgress } = downloadState;
        const filesCompleted = (currentFile - 1) * 100;
        const currentProgress = currentFileProgress;
        return (filesCompleted + currentProgress) / totalFiles;
    }

    // Handle errors
    ytDlp.stderr.on('data', (errorData) => {
        const errorOutput = errorData.toString();
        console.error(`Error: ${errorOutput}`);
        event.reply('download-error', `Error: ${errorOutput}`);
        lastProgressUpdate = Date.now();
    });

    // Set up stall check
    const stallCheckInterval = setInterval(() => {
        if (Date.now() - lastProgressUpdate > 30000) {
            clearInterval(stallCheckInterval);
            ytDlp.kill();
            event.reply('download-error', `Download stalled for ${playlistInfo.title}`);
            activeDownloads.delete(downloadId);
        }
    }, 5000);

    ytDlp.on('exit', (code) => {
        clearInterval(stallCheckInterval);
        if (code === 0) {
            // Create M3U8 playlist file
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
            saveDownloadToDatabase(downloadInfo);
            event.reply('download-complete', { order: downloadId });
        } else {
            event.reply('download-error', `Process exited with code ${code}`);
        }
        activeDownloads.delete(downloadId);
    });
}
module.exports = {handleYtDlpDownload, handleYtDlpMusicDownload};
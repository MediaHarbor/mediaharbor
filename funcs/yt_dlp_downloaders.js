const path = require("path");
const os = require("os");
const { spawn } = require("child_process");
const { app } = require("electron");
const { saveDownloadToDatabase } = require("./db");
const { fetchWebsiteTitle, extractDomain, fetchHighResImageOrFavicon } = require("./fetchers");
const { getNextDownloadOrder } = require('./downloadorder');
let downloadCount = 0;
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

    const { url, quality } = data;
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

    const videoInfoProcess = spawn(ytDlpCommand, videoInfoArgs);
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

    videoInfoProcess.on('exit', () => {
        if (videoInfo.title) {
            startMusicDownload(event, url, quality, settings, videoInfo, downloadId);
        } else {
            event.reply('download-error', `Failed to fetch video info for ${url}`);
            activeDownloads.delete(downloadId);
        }
    });
}

function startMusicDownload(event, url, quality, settings, videoInfo, downloadId) {
    const ytDlpCommand = 'yt-dlp';
    const args = buildYtDlpMusicArgs(url, quality, settings);

    let hasStarted = false;
    let isAlreadyDownloaded = false;
    let lastProgressUpdate = Date.now();

    const ytDlp = spawn(ytDlpCommand, args);

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
    const { url, quality } = data;
    const ytDlpCommand = 'yt-dlp';
    const downloadId = getNextDownloadOrder();

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

        const videoInfoProcess = spawn(ytDlpCommand, videoInfoArgs);
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

    const ytDlp = spawn(ytDlpCommand, args);

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


module.exports = {handleYtDlpDownload, handleYtDlpMusicDownload};
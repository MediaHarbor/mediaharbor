const {
    spawn: spawn
} = require("child_process");
const fs = require("fs");
const path = require("path");
const {
    getNextDownloadOrder: getNextDownloadOrder
} = require("./downloadorder");
const {
    dialog: dialog,
    shell: shell
} = require("electron");
const { resolveMetadata } = require("./appleMusicMeta");
const { resolveSpotifyMetadata } = require("./spotifyMeta");
const { saveServiceConfig, spotifyConfigPath } = require("./settings");
const logger = require("./logger");
const { resolveCommand } = require("./venvManager");
const { getBento4ToolPath } = require("./installers/bento4installer");
class gamRip {
    constructor(settingsFilePath, app) {
        this.settingsFilePath = settingsFilePath;
        this.app = app;
        this.activeProcesses = [];
        if (process.platform === "darwin" || process.platform === "linux") {
            import("fix-path").then(module => {
                module.default()
            }).catch(() => {})
        }
        this.serviceConfig = {
            spotify: {
                serviceName: "Spotify",
                downloadCommand: "votify",
                argsBuilder: (command, settings) => {
                    const configPath = path.join(app.getPath("userData"), "votify_config.ini");
                    const downloadLocation = settings?.createPlatformSubfolders ? `${settings.downloadLocation}/Spotify` : settings.downloadLocation;
                    const args = ["--config-path", configPath, "-o", downloadLocation];
                    if (command.quality && command.quality !== "auto") {
                        args.push("--audio-quality", command.quality);
                    }
                    if (settings?.spotify_cookies_path) {
                        args.push("-c", settings.spotify_cookies_path);
                    }
                    args.push(command.url);
                    return args
                },
                progressRegex: /\[download\]\s+(\d+\.\d+)%/,
                metadataParsers: {
                    title: /Downloading\s+"(.+)"/
                },
                prefetchMetadata: true,
                prefetchResolver: resolveSpotifyMetadata,
                batchSupport: true,
                batchArgsBuilder: (data, settings) => {
                    const configPath = path.join(app.getPath("userData"), "votify_config.ini");
                    const downloadLocation = settings?.createPlatformSubfolders ? `${settings.downloadLocation}/Spotify` : settings.downloadLocation;
                    const args = ["--config-path", configPath, "-o", downloadLocation, "-r"];
                    if (data.quality && data.quality !== "auto") {
                        args.push("--audio-quality", data.quality);
                    }
                    if (settings?.spotify_cookies_path) {
                        args.push("-c", settings.spotify_cookies_path);
                    }
                    args.push(data.filePath);
                    return args
                }
            },
            applemusic: {
                serviceName: "Apple Music",
                downloadCommand: "gamdl",
                argsBuilder: (command, settings) => {
                    const args = this._buildGamdlArgs(settings, command.quality);
                    args.push(command.url);
                    return args
                },
                progressRegex: /\[download\]\s+(\d+\.\d+)%/,
                metadataParsers: {
                    title: /Downloading\s+"(.+)"/
                },
                prefetchMetadata: true,
                batchSupport: true,
                batchArgsBuilder: (data, settings) => {
                    const args = this._buildGamdlArgs(settings, data.quality);
                    args.push("-r", data.filePath);
                    return args
                }
            }
        }
    }
    _buildGamdlArgs(settings, quality) {
        const configPath = path.join(this.app.getPath("userData"), "gamdl_config.ini");
        const args = ["--config-path", configPath];

        const cookiesPath = settings.apple_cookies_path || path.join(process.env.USERPROFILE || process.env.HOME, "Downloads", "apple.com_cookies.txt");
        args.push("-c", path.resolve(cookiesPath));

        let outputPath;
        if (settings.createPlatformSubfolders) {
            outputPath = path.join(settings.downloadLocation || this.app.getPath("downloads"), "Apple Music");
        } else {
            outputPath = settings.downloadLocation || this.app.getPath("downloads");
        }
        args.push("-o", path.resolve(outputPath));

        if (settings.apple_temp_path) {
            args.push("--temp-path", path.resolve(settings.apple_temp_path));
        }

        args.push("--song-codec-priority", quality || "aac-legacy");

        if (settings.apple_download_mode) {
            args.push("--download-mode", settings.apple_download_mode);
        }

        if (settings.apple_remux_mode) {
            args.push("--music-video-remux-mode", settings.apple_remux_mode);
        }

        if (settings.apple_cover_format) {
            args.push("--cover-format", settings.apple_cover_format);
        }
        if (settings.apple_cover_size) {
            args.push("--cover-size", String(settings.apple_cover_size));
        }

        if (settings.apple_save_cover) args.push("--save-cover");
        if (settings.apple_save_playlist) args.push("--save-playlist");
        if (settings.apple_overwrite) args.push("--overwrite");
        if (settings.apple_synced_lyrics_only) args.push("--synced-lyrics-only");
        if (settings.apple_no_synced_lyrics) args.push("--no-synced-lyrics");
        if (settings.apple_use_album_date) args.push("--use-album-date");
        if (settings.apple_fetch_extra_tags) args.push("--fetch-extra-tags");
        if (settings.apple_no_exceptions) args.push("--no-exceptions");
        if (settings.apple_use_wrapper) args.push("--use-wrapper");

        if (settings.apple_synced_lyrics_format) {
            args.push("--synced-lyrics-format", settings.apple_synced_lyrics_format);
        }

        if (settings.apple_language) {
            args.push("--language", settings.apple_language);
        }

        if (settings.apple_truncate) {
            args.push("--truncate", String(settings.apple_truncate));
        }

        if (settings.apple_exclude_tags) {
            args.push("--exclude-tags", settings.apple_exclude_tags);
        }

        if (settings.apple_log_level && settings.apple_log_level !== "INFO") {
            args.push("--log-level", settings.apple_log_level);
        }

        if (settings.apple_template_folder_album) {
            args.push("--album-folder-template", settings.apple_template_folder_album);
        }
        if (settings.apple_template_folder_compilation) {
            args.push("--compilation-folder-template", settings.apple_template_folder_compilation);
        }
        if (settings.apple_template_file_single_disc) {
            args.push("--single-disc-file-template", settings.apple_template_file_single_disc);
        }
        if (settings.apple_template_file_multi_disc) {
            args.push("--multi-disc-file-template", settings.apple_template_file_multi_disc);
        }
        if (settings.apple_template_folder_no_album) {
            args.push("--no-album-folder-template", settings.apple_template_folder_no_album);
        }
        if (settings.apple_template_file_no_album) {
            args.push("--no-album-file-template", settings.apple_template_file_no_album);
        }
        if (settings.apple_template_file_playlist) {
            args.push("--playlist-file-template", settings.apple_template_file_playlist);
        }
        if (settings.apple_date_tag_template) {
            args.push("--date-tag-template", settings.apple_date_tag_template);
        }

        if (settings.apple_mv_enabled) {
            if (settings.apple_mv_codec_priority) {
                args.push("--music-video-codec-priority", settings.apple_mv_codec_priority);
            }
            if (settings.apple_mv_remux_format) {
                args.push("--music-video-remux-format", settings.apple_mv_remux_format);
            }
            if (settings.apple_mv_resolution) {
                args.push("--music-video-resolution", settings.apple_mv_resolution);
            }
            if (settings.apple_uploaded_video_quality) {
                args.push("--uploaded-video-quality", settings.apple_uploaded_video_quality);
            }
        }

        const mp4decryptPath = (settings.apple_custom_paths_enabled && settings.apple_mp4decrypt_path && settings.apple_mp4decrypt_path !== "mp4decrypt")
            ? settings.apple_mp4decrypt_path
            : getBento4ToolPath("mp4decrypt");
        if (mp4decryptPath !== "mp4decrypt") args.push("--mp4decrypt-path", mp4decryptPath);

        const mp4boxPath = (settings.apple_custom_paths_enabled && settings.apple_mp4box_path && settings.apple_mp4box_path !== "MP4Box")
            ? settings.apple_mp4box_path
            : getBento4ToolPath("MP4Box");
        if (mp4boxPath !== "MP4Box") args.push("--mp4box-path", mp4boxPath);

        const ffmpegPath = (settings.apple_custom_paths_enabled && settings.apple_ffmpeg_path && settings.apple_ffmpeg_path !== "ffmpeg")
            ? settings.apple_ffmpeg_path
            : resolveCommand("ffmpeg");
        if (ffmpegPath !== "ffmpeg") args.push("--ffmpeg-path", ffmpegPath);

        if (settings.apple_custom_paths_enabled) {
            if (settings.apple_nm3u8dlre_path && settings.apple_nm3u8dlre_path !== "N_m3u8DL-RE") {
                args.push("--nm3u8dlre-path", settings.apple_nm3u8dlre_path);
            }
            if (settings.apple_wvd_path) {
                args.push("--wvd-path", settings.apple_wvd_path);
            }
        }

        if (settings.apple_use_wrapper) {
            if (settings.apple_wrapper_account_url) {
                args.push("--wrapper-account-url", settings.apple_wrapper_account_url);
            }
            if (settings.apple_wrapper_decrypt_ip) {
                args.push("--wrapper-decrypt-ip", settings.apple_wrapper_decrypt_ip);
            }
        }

        return args;
    }
    createProcessEnv() {
        return {
            PYTHONUNBUFFERED: "1",
            ...process.env,
            PYTHONIOENCODING: "utf-8",
            LANG: "en_US.UTF-8",
            LC_ALL: "en_US.UTF-8"
        }
    }
    spawnProcess(command, args) {
        const resolvedCommand = resolveCommand(command);
        return spawn(resolvedCommand, args, {
            stdio: ["pipe", "pipe", "pipe"],
            encoding: "utf8",
            env: this.createProcessEnv()
        })
    }
    handleDownload(event, command, serviceName) {
        fs.readFile(this.settingsFilePath, "utf8", async (err, settingsData) => {
            if (err) {
                event.reply("download-error", {
                    order: -1,
                    error: "Could not read settings file",
                    fullLog: `Could not read settings file: ${err.message}`
                });
                return
            }
            try {
                const settings = JSON.parse(settingsData);
                const config = this.serviceConfig[serviceName];
                const ripArgs = config.argsBuilder(command, settings);
                const downloadOrder = getNextDownloadOrder();
                event.reply("download-info", {
                    title: `${config.serviceName} Download`,
                    platform: serviceName,
                    quality: String(command.quality ?? ''),
                    order: downloadOrder
                });

                let prefetchedMeta = null;
                if (config.prefetchMetadata && command.url) {
                    try {
                        const resolver = config.prefetchResolver || resolveMetadata;
                        prefetchedMeta = await resolver(command.url);
                    } catch (e) {
                    }
                }

                const ripProcess = this.spawnProcess(config.downloadCommand, ripArgs);
                logger.info('download', `Started ${config.serviceName} download: ${command.url}`);
                this.activeProcesses.push({
                    process: ripProcess,
                    order: downloadOrder
                });
                let trackInfo = {
                    cover: prefetchedMeta?.coverUrl || null,
                    title: prefetchedMeta?.title || null,
                    album: prefetchedMeta?.album || null,
                    artist: prefetchedMeta?.artist || null,
                    progress: 0,
                    order: downloadOrder
                };

                if (prefetchedMeta) {
                    event.reply("download-info", {
                        order: downloadOrder,
                        title: trackInfo.title,
                        thumbnail: trackInfo.cover,
                        artist: trackInfo.artist,
                        album: trackInfo.album,
                        platform: serviceName,
                        quality: String(command.quality ?? ''),
                    });
                }

                let buffer = "";
                let fullOutput = "";
                let skippedTracks = [];
                const handleOutput = (data, isError = false) => {
                    const output = data.toString("utf8");
                    fullOutput += output;
                    buffer += output;
                    const cleanData = output.replace(/\r/g, "\n");
                    const lines = cleanData.split("\n");
                    lines.forEach(async line => {
                        if (line.includes("does not exist") && line.includes("press enter to continue")) {
                            const isCookies = line.toLowerCase().includes("cookies");
                            const fileType = isCookies ? "cookies" : "WVD";
                            try {
                                const result = await dialog.showOpenDialog({
                                    title: `Select Spotify ${fileType} file`,
                                    properties: ["openFile"],
                                    filters: isCookies
                                        ? [{ name: "Cookies", extensions: ["txt"] }, { name: "All Files", extensions: ["*"] }]
                                        : [{ name: "WVD Files", extensions: ["wvd"] }, { name: "All Files", extensions: ["*"] }]
                                });
                                if (result.canceled || !result.filePaths.length) {
                                    ripProcess.kill();
                                    logger.error('download', `${config.serviceName} ${fileType} file required but not provided for ${command.url}`);
                                    event.reply("download-error", {
                                        order: downloadOrder,
                                        error: `Spotify ${fileType} file is required`,
                                        fullLog: fullOutput + `\n[MediaHarbor] ${fileType} file selection was cancelled`
                                    });
                                    return;
                                }
                                const selectedPath = result.filePaths[0];
                                const settingKey = isCookies ? "spotify_cookies_path" : "spotify_wvd_path";
                                fs.readFile(this.settingsFilePath, "utf8", (err, data) => {
                                    if (!err) {
                                        try {
                                            const s = JSON.parse(data);
                                            s[settingKey] = selectedPath;
                                            fs.writeFileSync(this.settingsFilePath, JSON.stringify(s, null, 4));
                                            saveServiceConfig(spotifyConfigPath, s, 'spotify').catch(() => {});
                                        } catch (e) { logger.warn('gam', `Failed to parse or update settings file: ${e.message || e}`); }
                                    }
                                });
                                ripProcess.stdin.write(selectedPath + "\n");
                            } catch (err) {
                                ripProcess.kill();
                                logger.error('download', `Failed to select ${fileType} file for ${command.url}: ${err.message}`);
                                event.reply("download-error", {
                                    order: downloadOrder,
                                    error: `Failed to select ${fileType} file`,
                                    fullLog: fullOutput + `\n[MediaHarbor] Failed to select ${fileType} file: ${err.message}`
                                });
                            }
                            return;
                        }
                        if (line.includes("Select which") && (line.includes("to download") || line.includes("codec"))) {
                            ripProcess.kill();
                            const hint = line.includes("media") ? "Set 'Artist media option' in Spotify settings (e.g. albums, singles)"
                                : line.includes("album") ? "Use a direct album/track URL instead of an artist URL"
                                : line.includes("video") ? "Use a direct video URL or set video format/resolution in settings"
                                : "Use a direct URL instead of an artist URL";
                            logger.error('download', `Interactive selection not supported for ${command.url}: ${hint}`);
                            event.reply("download-error", {
                                order: downloadOrder,
                                error: `Votify requires interactive selection which is not supported in MediaHarbor. ${hint}`,
                                fullLog: fullOutput + `\n[MediaHarbor] Interactive selection not supported: ${line}`
                            });
                            return;
                        }
                        if (line.includes("Click on the following link to login:")) {
                            const nextLines = lines.slice(lines.indexOf(line), lines.indexOf(line) + 3);
                            for (const currentLine of nextLines) {
                                if (currentLine.includes("https://accounts.spotify.com/authorize")) {
                                    const url = currentLine.trim();
                                    try {
                                        const success = await shell.openExternal(url, {
                                            activate: true,
                                            workingDirectory: process.cwd()
                                        });
                                        if (!success) {
                                            throw new Error("Failed to open URL")
                                        }
                                    } catch (err) {
                                        logger.error('download', `Failed to open authorization URL for ${command.url}: ${err.message}`);
                                        event.reply("download-error", {
                                            order: downloadOrder,
                                            error: `Failed to open authorization URL: ${err.message}`,
                                            fullLog: fullOutput + `\n[MediaHarbor] Failed to open authorization URL: ${err.message}`
                                        })
                                    }
                                    break
                                }
                            }
                        }
                        const cleanLine = line.replace(/^Error:\s*/, "").trim();
                        const skipMatch = cleanLine.match(/Skipping\s+"(.+?)":\s*(.+)/);
                        if (skipMatch) {
                            skippedTracks.push({ title: skipMatch[1], reason: skipMatch[2].trim() });
                            logger.warn('download', `Skipped track "${skipMatch[1]}": ${skipMatch[2].trim()}`);
                        }
                        for (let key in config.metadataParsers) {
                            const regex = config.metadataParsers[key];
                            const match = cleanLine.match(regex);
                            if (match) {
                                trackInfo[key] = match[1].trim();
                                if (key === "cover") {
                                    trackInfo.thumbnail = trackInfo.cover
                                }
                            }
                        }
                        const progressMatch = cleanLine.match(config.progressRegex);
                        if (progressMatch) {
                            trackInfo.progress = parseFloat(progressMatch[1]);
                            event.reply("download-update", {
                                order: downloadOrder,
                                progress: trackInfo.progress,
                                title: trackInfo.title,
                                thumbnail: trackInfo.cover,
                                artist: trackInfo.artist,
                                album: trackInfo.album,
                                isBatch: false
                            })
                        }
                    })
                };
                ripProcess.stdout.on("data", data => handleOutput(data, false));
                ripProcess.stderr.on("data", data => {
                    handleOutput(data, true);
                    const errStr = data.toString("utf8");
                    if (errStr.includes("ERROR") || errStr.includes("CRITICAL") || errStr.includes("Traceback")) {
                        logger.warn('download', `Stderr error for ${command.url}: ${errStr.substring(0, 200)}`);
                        event.reply("download-error", {
                            order: downloadOrder,
                            title: trackInfo.title || `Download #${downloadOrder}`,
                            error: errStr,
                            fullLog: fullOutput
                        })
                    }
                });
                ripProcess.on("exit", code => {
                    this.activeProcesses = this.activeProcesses.filter(p => p.process !== ripProcess);
                    const finishedMatch = fullOutput.match(/Finished with (\d+) error/);
                    const finishedErrors = finishedMatch ? parseInt(finishedMatch[1]) : 0;

                    if (code === 0) {
                        fs.readFile(this.settingsFilePath, "utf8", (err, settingsData) => {
                            const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                            const downloadLocation = settings.downloadLocation || this.app.getPath("downloads");
                            if (skippedTracks.length > 0 && !trackInfo.progress) {
                                const skipMsg = skippedTracks.map(s => `"${s.title}": ${s.reason}`).join('\n');
                                logger.error('download', `${config.serviceName} download skipped all tracks for ${command.url}: ${skipMsg.substring(0, 200)}`);
                                event.reply("download-error", {
                                    order: downloadOrder,
                                    title: trackInfo.title || `Download #${downloadOrder}`,
                                    error: `Skipped: ${skipMsg}`,
                                    fullLog: fullOutput
                                });
                                return;
                            }
                            if (finishedErrors > 0) {
                                const warnMsg = skippedTracks.length > 0
                                    ? skippedTracks.map(s => `Skipped "${s.title}": ${s.reason}`).join('\n')
                                    : `Finished with ${finishedErrors} error(s)`;
                                logger.error('download', `${config.serviceName} download finished with ${finishedErrors} error(s) for ${command.url}`);
                                event.reply("download-error", {
                                    order: downloadOrder,
                                    title: trackInfo.title || `Download #${downloadOrder}`,
                                    error: warnMsg,
                                    fullLog: fullOutput
                                });
                                return;
                            }
                            event.reply("download-complete", {
                                order: downloadOrder,
                                location: downloadLocation,
                                title: trackInfo.title,
                                thumbnail: trackInfo.cover,
                                artist: trackInfo.artist,
                                album: trackInfo.album,
                                progress: 100,
                                isBatch: false,
                                fullLog: fullOutput,
                                warnings: skippedTracks.length > 0
                                    ? skippedTracks.map(s => `Skipped "${s.title}": ${s.reason}`).join('\n')
                                    : undefined
                            })
                            logger.info('download', `${config.serviceName} download complete: ${trackInfo.title || command.url}`);
                        })
                    } else {
                        fullOutput += `\n[MediaHarbor] Process exited with code ${code}`;
                        logger.error('download', `${config.serviceName} download failed for ${command.url} with exit code ${code}`);
                        event.reply("download-error", {
                            order: downloadOrder,
                            title: trackInfo.title || `Download #${downloadOrder}`,
                            error: `Process exited with code ${code}`,
                            fullLog: fullOutput
                        })
                    }
                });
                ripProcess.on("error", err => {
                    fullOutput += `\n[MediaHarbor] Process error: ${err.message}`;
                    logger.error('download', `${config.serviceName} process crashed for ${command.url}: ${err.message}`);
                    event.reply("download-error", {
                        order: downloadOrder,
                        title: trackInfo.title || `Download #${downloadOrder}`,
                        error: `Process error: ${err.message}`,
                        fullLog: fullOutput
                    })
                })
            } catch (error) {
                logger.error('download', `Failed to parse settings: ${error.message}`);
                event.reply("download-error", {
                    order: -1,
                    error: `Failed to parse settings: ${error.message}`,
                    fullLog: `Failed to parse settings: ${error.message}\n${error.stack}`
                })
            }
        })
    }
    handleBatchDownload(event, data, serviceName) {
        fs.readFile(this.settingsFilePath, "utf8", (err, settingsData) => {
            if (err) {
                event.reply("download-error", {
                    order: -1,
                    error: "Could not read settings file",
                    fullLog: `Could not read settings file: ${err.message}`
                });
                return
            }
            try {
                const settings = JSON.parse(settingsData);
                const config = this.serviceConfig[serviceName];
                const ripArgs = config.batchArgsBuilder(data, settings);
                const downloadOrder = getNextDownloadOrder();
                let totalTracks = 0;
                let completedTracks = 0;
                let trackProgressMap = {};
                let overallProgress = 0;
                let fullOutput = "";
                const throttledUpdate = this.throttle(data => {
                    event.reply("download-update", data)
                }, 250);
                event.reply("download-info", {
                    title: `Batch Download #${downloadOrder}`,
                    downloadArtistOrUploader: config.serviceName,
                    platform: serviceName,
                    quality: String(data.quality ?? ''),
                    order: downloadOrder,
                    isBatch: true
                });
                const ripProcess = this.spawnProcess(config.downloadCommand, ripArgs);
                logger.info('download', `Started ${config.serviceName} batch download (${data.urls ? data.urls.length + ' URLs' : 'batch'})`);
                ripProcess.on("error", err => {
                    fullOutput += `\n[MediaHarbor] Process error: ${err.message}`;
                    logger.error('download', `${config.serviceName} batch process crashed: ${err.message}`);
                    event.reply("download-error", {
                        order: downloadOrder,
                        error: `Process error: ${err.message}`,
                        fullLog: fullOutput
                    })
                });
                let buffer = "";
                let currentTrackTitle = "Unknown Track";
                let batchSkippedTracks = [];
                ripProcess.stdout.on("data", data => {
                    const output = data.toString("utf8");
                    fullOutput += output;
                    buffer += output;
                    const lines = buffer.split("\n");
                    buffer = lines.pop();
                    lines.forEach(line => {
                        const loadingMatch = line.match(/Found (\d+) tracks/);
                        if (loadingMatch) {
                            totalTracks = parseInt(loadingMatch[1]);
                        }
                        const trackCountMatch = line.match(/\[Track\s+(\d+)\/(\d+)\]/);
                        if (trackCountMatch) {
                            totalTracks = parseInt(trackCountMatch[2]);
                            completedTracks = parseInt(trackCountMatch[1]) - 1;
                        }
                        const titleMatch = line.match(/Downloading\s+"(.+)"/);
                        if (titleMatch) {
                            currentTrackTitle = titleMatch[1];
                        }
                        const progressMatch = line.match(config.progressRegex);
                        if (progressMatch) {
                            const progress = parseFloat(progressMatch[1]);
                            const trackId = currentTrackTitle;
                            trackProgressMap[trackId] = {
                                trackTitle: currentTrackTitle,
                                artist: null,
                                progress: progress
                            };
                            const totalProgress = Object.values(trackProgressMap).reduce((sum, track) => sum + track.progress, 0);
                            overallProgress = totalTracks > 0 ? totalProgress / (totalTracks * 100) * 100 : 0;
                            throttledUpdate({
                                tracksProgress: Object.values(trackProgressMap),
                                order: downloadOrder,
                                completedTracks: completedTracks,
                                totalTracks: totalTracks,
                                overallProgress: Math.min(overallProgress, 100),
                                isBatch: true
                            })
                        }
                        if (line.includes("Download completed:") || (line.includes("Finished") && completedTracks < totalTracks)) {
                            completedTracks++;
                            delete trackProgressMap[currentTrackTitle]
                        }
                    })
                });
                ripProcess.stderr.on("data", errorData => {
                    const errorOutput = errorData.toString("utf8");
                    fullOutput += errorOutput;
                    errorOutput.split("\n").forEach(line => {
                        const trackCountMatch = line.match(/\[Track\s+(\d+)\/(\d+)\]/);
                        if (trackCountMatch) {
                            totalTracks = parseInt(trackCountMatch[2]);
                            completedTracks = parseInt(trackCountMatch[1]) - 1;
                        }
                        const titleMatch = line.match(/Downloading\s+"(.+)"/);
                        if (titleMatch) {
                            currentTrackTitle = titleMatch[1];
                        }
                        const skipMatch = line.match(/Skipping\s+"(.+?)":\s*(.+)/);
                        if (skipMatch) {
                            batchSkippedTracks.push({ title: skipMatch[1], reason: skipMatch[2].trim() });
                            logger.warn('download', `Batch skipped track "${skipMatch[1]}": ${skipMatch[2].trim()}`);
                        }
                        const progressMatch = line.match(config.progressRegex);
                        if (progressMatch) {
                            const progress = parseFloat(progressMatch[1]);
                            trackProgressMap[currentTrackTitle] = {
                                trackTitle: currentTrackTitle,
                                artist: null,
                                progress: progress
                            };
                            const totalProgress = Object.values(trackProgressMap).reduce((sum, track) => sum + track.progress, 0);
                            overallProgress = totalTracks > 0 ? totalProgress / (totalTracks * 100) * 100 : 0;
                            throttledUpdate({
                                tracksProgress: Object.values(trackProgressMap),
                                order: downloadOrder,
                                completedTracks: completedTracks,
                                totalTracks: totalTracks,
                                overallProgress: Math.min(overallProgress, 100),
                                isBatch: true
                            });
                        }
                    });
                    if (errorOutput.includes("ERROR") || errorOutput.includes("CRITICAL") || errorOutput.includes("Traceback")) {
                        logger.warn('download', `Batch stderr error: ${errorOutput.substring(0, 200)}`);
                        event.reply("download-error", {
                            order: downloadOrder,
                            title: `Batch Download #${downloadOrder}`,
                            error: errorOutput,
                            fullLog: fullOutput
                        })
                    }
                });
                ripProcess.on("exit", code => {
                    const finishedMatch = fullOutput.match(/Finished with (\d+) error/);
                    const finishedErrors = finishedMatch ? parseInt(finishedMatch[1]) : 0;

                    if (code === 0) {
                        fs.readFile(this.settingsFilePath, "utf8", (err, settingsData) => {
                            const settings = err ? this.getDefaultSettings() : JSON.parse(settingsData);
                            const downloadLocation = settings.downloadLocation || this.app.getPath("downloads");
                            if (finishedErrors > 0) {
                                const warnMsg = batchSkippedTracks.length > 0
                                    ? batchSkippedTracks.map(s => `Skipped "${s.title}": ${s.reason}`).join('\n')
                                    : `Finished with ${finishedErrors} error(s)`;
                                logger.error('download', `${config.serviceName} batch finished with ${finishedErrors} error(s)`);
                                event.reply("download-error", {
                                    order: downloadOrder,
                                    title: `Batch Download #${downloadOrder}`,
                                    error: warnMsg,
                                    fullLog: fullOutput
                                });
                                return;
                            }
                            event.reply("download-complete", {
                                order: downloadOrder,
                                title: `Batch Download #${downloadOrder}`,
                                completedTracks: completedTracks,
                                totalTracks: totalTracks,
                                overallProgress: 100,
                                isBatch: true,
                                fullLog: fullOutput,
                                warnings: batchSkippedTracks.length > 0
                                    ? batchSkippedTracks.map(s => `Skipped "${s.title}": ${s.reason}`).join('\n')
                                    : undefined
                            })
                            logger.info('download', `${config.serviceName} batch download complete: ${completedTracks}/${totalTracks} tracks`);
                        })
                    } else {
                        fullOutput += `\n[MediaHarbor] Process exited with code ${code}`;
                        logger.error('download', `${config.serviceName} batch download failed with exit code ${code}`);
                        event.reply("download-error", {
                            order: downloadOrder,
                            title: `Batch Download #${downloadOrder}`,
                            error: `Process exited with code ${code}`,
                            fullLog: fullOutput
                        })
                    }
                })
            } catch (parseError) {
                logger.error('download', `Batch settings parse error: ${parseError.message}`);
                event.reply("download-error", {
                    order: -1,
                    error: "Settings parse error",
                    fullLog: `Settings parse error: ${parseError.message}\n${parseError.stack}`
                })
            }
        })
    }
    throttle(func, limit) {
        let inThrottle;
        return function(...args) {
            if (!inThrottle) {
                func.apply(this, args);
                inThrottle = true;
                setTimeout(() => inThrottle = false, limit)
            }
        }
    }
    getDefaultSettings() {
        return {
            downloadLocation: this.app.getPath("downloads"),
            appleMusicCookiesPath: path.join(process.env.USERPROFILE, "Downloads", "apple.com_cookies.txt"),
            spotifyCookiesPath: path.join(process.env.USERPROFILE, "Downloads", "spotify.com_cookies.txt")
        }
    }
    clearCredentials() {
        return new Promise((resolve, reject) => {
            try {
                const configPath = path.join(this.app.getPath("userData"), "votify_config.ini");
                if (fs.existsSync(configPath)) {
                    fs.rmSync(configPath, { force: true });
                }
                fs.readFile(this.settingsFilePath, "utf8", (err, data) => {
                    if (!err) {
                        try {
                            const settings = JSON.parse(data);
                            settings.spotify_cookies_path = "";
                            fs.writeFileSync(this.settingsFilePath, JSON.stringify(settings, null, 2))
                        } catch (error) {
                        }
                    }
                    resolve()
                })
            } catch (err) {
                reject(new Error(`Failed to clear Spotify credentials: ${err.message}`))
            }
        })
    }
}
module.exports = gamRip;
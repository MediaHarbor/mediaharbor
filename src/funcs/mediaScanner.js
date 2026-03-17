const path = require("path");
const crypto = require("crypto");
const fs = require("fs/promises");
const { spawn } = require("child_process");
const {
    app: app
} = require("electron");
const mm = require("music-metadata");
const {
    Worker: Worker
} = require("worker_threads");
const ffmpegPath = require('ffmpeg-static');
const sharp = require('sharp');
const logger = require('./logger');

const VIDEO_FORMATS = new Set([".mkv", ".mp4", ".flv", ".avi", ".mov", ".webm"]);
const MUSIC_FORMATS = new Set([".opus", ".flac", ".mp3", ".aac", ".m4a", ".wav"]);
const CACHE_VERSION = "1.0";
const THUMBNAIL_SIZE_LIMIT = 5e5;
class CacheManager {
    constructor() {
        this.cachePath = path.join(app.getPath("userData"), "media-cache.json");
        this.cache = new Map;
        this.loaded = false;
        this.pendingSave = null
    }
    async load() {
        if (this.loaded) return;
        try {
            const data = await fs.readFile(this.cachePath, "utf8");
            const parsed = JSON.parse(data);
            if (parsed.version === CACHE_VERSION) {
                this.cache = new Map(Object.entries(parsed.data))
            }
        } catch (err) {
        }
        this.loaded = true
    }
    async save(folderHash, data) {
        this.cache.set(folderHash, data);
        clearTimeout(this.pendingSave);
        this.pendingSave = setTimeout(async () => {
            try {
                await fs.writeFile(this.cachePath, JSON.stringify({
                    version: CACHE_VERSION,
                    data: Object.fromEntries(this.cache)
                }))
            } catch (err) {
            }
        }, 1e3)
    }
    get(folderHash) {
        return this.cache.get(folderHash)
    }
}
class MediaScanner {
    constructor() {
        this.cacheManager = new CacheManager;
        this.processQueue = [];
        this.processingCount = 0;
        this.maxConcurrent = 10
    }
    getFolderHash(directory) {
        return crypto.createHash("md5").update(directory).digest("hex")
    }
    invalidateCache(directory) {
        const hash = this.getFolderHash(directory);
        this.cacheManager.cache.delete(hash);
        logger.info('mediascanner', `Cache invalidated for: ${directory}`);
    }
    async scanDirectory(directory, event, options = {}) {
        const { force = false } = options;
        const updateProgress = (progress, currentFile = "") => {
            if (!event?.sender) return;
            try {
                event.sender.send("scan-progress", {
                    progress: Math.min(100, Math.max(0, progress)),
                    currentFile: currentFile.substring(0, 255),
                    processed: progress,
                    total: 100
                })
            } catch (err) {
            }
        };
        try {
            await this.cacheManager.load();
            const folderHash = this.getFolderHash(directory);
            const cachedData = this.cacheManager.get(folderHash);
            if (!force && cachedData) {
                updateProgress(100, "Loaded from cache");
                return cachedData
            }
            const discoveryWorker = new Worker(path.join(__dirname, "./fileDiscoveryWorker.js"));
            const { files } = await new Promise((resolve, reject) => {
                discoveryWorker.on("message", (message) => {
                    resolve(message);
                });
                discoveryWorker.on("error", reject);
                discoveryWorker.postMessage({
                    directory: directory,
                    formats: [...VIDEO_FORMATS, ...MUSIC_FORMATS]
                })
            });
            if (!files || !Array.isArray(files)) {
                throw new Error("File discovery failed: invalid files data");
            }
            const mediaItems = await this.processFilesParallel(files, updateProgress);
            discoveryWorker.terminate();
            const result = this.organizeMediaItems(mediaItems);
            await this.cacheManager.save(folderHash, result);
            updateProgress(100, "Scan complete");
            return result
        } catch (error) {
            event?.sender?.send("scan-error", error.message);
            throw error
        }
    }
    async processFilesParallel(files, updateProgress) {
        const mediaItems = [];
        const processedCount = {
            value: 0
        };
        const totalFiles = files.length;
        const processNext = async () => {
            while (this.processQueue.length > 0 && this.processingCount < this.maxConcurrent) {
                const file = this.processQueue.shift();
                this.processingCount++;
                try {
                    const result = await this.processFile(file);
                    if (result) mediaItems.push(result)
                } catch (err) {
                } finally {
                    this.processingCount--;
                    processedCount.value++;
                    updateProgress(Math.round(processedCount.value * 100 / totalFiles))
                }
            }
        };
        this.processQueue = [...files];
        const workers = Array(this.maxConcurrent).fill().map(() => processNext());
        await Promise.all(workers);
        return mediaItems
    }
    async processFile(file) {
        const ext = path.extname(file.path).toLowerCase();
        const isVideo = VIDEO_FORMATS.has(ext);
        const fileData = {
            title: path.basename(file.path, ext),
            type: isVideo ? "video" : "music",
            size: `${(file.size/(1024*1024*1024)).toFixed(2)} GB`,
            date: file.mtime.toISOString(),
            path: file.path,
            metadata: {}
        };
        if (!isVideo) {
            try {
                const metadata = await mm.parseFile(file.path, {
                    duration: true,
                    skipCovers: false
                });
                fileData.metadata = {
                    artist: metadata.common?.artist || "Unknown",
                    album: metadata.common?.album || "Unknown",
                    year: metadata.common?.year || "Unknown"
                };
                fileData.duration = metadata.format?.duration || 0;
                fileData.quality = `${metadata.format?.bitsPerSample || 16}bit / ${metadata.format?.sampleRate || 44100}kHz`;
                if (metadata.common?.title) {
                    fileData.title = metadata.common.title;
                }
                if (metadata.common?.picture && metadata.common.picture.length > 0) {
                    const picture = metadata.common.picture[0];
                    if (picture.data.length <= THUMBNAIL_SIZE_LIMIT) {
                        const resized = await sharp(picture.data).resize(200, 200, { fit: 'inside' }).jpeg().toBuffer();
                        fileData.thumbnail = {
                            data: resized.toString('base64'),
                            format: 'image/jpeg'
                        };
                    }
                }
            } catch (err) {
            }
        } else {
            try {
                const thumbnailPath = path.join(app.getPath("temp"), `thumb_${crypto.randomBytes(8).toString('hex')}.png`);
                await new Promise((resolve, reject) => {
                    const proc = spawn(ffmpegPath, [
                        '-ss', '1',
                        '-i', file.path,
                        '-vframes', '1',
                        '-vf', 'scale=320:240',
                        '-y',
                        thumbnailPath
                    ]);
                    proc.on('close', (code) => code === 0 ? resolve() : reject(new Error(`ffmpeg exited with code ${code}`)));
                    proc.on('error', reject);
                });
                const thumbData = await fs.readFile(thumbnailPath);
                const resized = await sharp(thumbData).resize(200, 200, { fit: 'inside' }).jpeg().toBuffer();
                fileData.thumbnail = {
                    data: resized.toString('base64'),
                    format: 'image/jpeg'
                };
                await fs.unlink(thumbnailPath);
            } catch (err) {
            }
        }
        return fileData
    }
    organizeMediaItems(mediaItems) {
        const albumMap = new Map;
        const videos = [];
        const trackPathSets = new Map; // album key → Set of track paths
        for (const item of mediaItems) {
            if (item.type === "video") {
                videos.push(item);
                continue
            }
            const normAlbum = (item.metadata.album || "Unknown").trim().toLowerCase().replace(/\s+/g, ' ');
            const normArtist = (item.metadata.artist || "Unknown").trim().toLowerCase().replace(/\s+/g, ' ');
            const key = `${normAlbum}::${normArtist}`;
            if (!albumMap.has(key)) {
                albumMap.set(key, {
                    type: "music",
                    album: item.metadata.album || "Unknown",
                    artist: item.metadata.artist || "Unknown",
                    year: item.metadata.year,
                    thumbnail: item.thumbnail,
                    tracks: []
                });
                trackPathSets.set(key, new Set());
            }
            const album = albumMap.get(key);
            const pathSet = trackPathSets.get(key);
            if (!pathSet.has(item.path)) {
                pathSet.add(item.path);
                album.tracks.push(item);
            }
            if (!album.thumbnail && item.thumbnail) {
                album.thumbnail = item.thumbnail;
            }
        }
        return [...videos, ...albumMap.values()]
    }
}
module.exports = {
    MediaScanner: MediaScanner,
    VIDEO_FORMATS: VIDEO_FORMATS,
    MUSIC_FORMATS: MUSIC_FORMATS
};

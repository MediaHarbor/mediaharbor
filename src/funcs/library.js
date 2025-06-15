const fs = require('fs').promises;
const path = require('path');
const crypto = require('crypto');
const { Worker } = require('worker_threads');
const mm = require('music-metadata');

// Multi-level cache system
class MediaCache {
    constructor(userDataPath) {
        this.cacheDir = path.join(userDataPath, 'media-cache');
        this.mainCache = path.join(this.cacheDir, 'main.json');
        this.metaCache = path.join(this.cacheDir, 'metadata');
        this.thumbCache = path.join(this.cacheDir, 'thumbnails');
        this.statsCache = new Map(); // In-memory stats cache
        this.init();
    }

    async init() {
        await fs.mkdir(this.cacheDir, { recursive: true });
        await fs.mkdir(this.metaCache, { recursive: true });
        await fs.mkdir(this.thumbCache, { recursive: true });
    }

    getFileHash(filePath, stats) {
        return crypto.createHash('md5')
            .update(`${filePath}:${stats.mtime.getTime()}:${stats.size}`)
            .digest('hex');
    }

    async loadMainCache(folderHash) {
        try {
            const data = await fs.readFile(this.mainCache, 'utf8');
            const cache = JSON.parse(data);
            if (cache.version === '2.0' && cache.folderHash === folderHash) {
                return cache.data;
            }
        } catch {}
        return null;
    }

    async saveMainCache(folderHash, data) {
        await fs.writeFile(this.mainCache, JSON.stringify({
            version: '2.0',
            folderHash,
            timestamp: Date.now(),
            data
        }));
    }

    async loadMetadata(fileHash) {
        try {
            const data = await fs.readFile(path.join(this.metaCache, `${fileHash}.json`), 'utf8');
            return JSON.parse(data);
        } catch {}
        return null;
    }

    async saveMetadata(fileHash, metadata) {
        await fs.writeFile(
            path.join(this.metaCache, `${fileHash}.json`),
            JSON.stringify(metadata)
        );
    }

    async loadThumbnail(fileHash) {
        try {
            return await fs.readFile(path.join(this.thumbCache, `${fileHash}.jpg`));
        } catch {}
        return null;
    }

    async saveThumbnail(fileHash, thumbnailData) {
        await fs.writeFile(
            path.join(this.thumbCache, `${fileHash}.jpg`),
            thumbnailData
        );
    }
}

// Worker pool for parallel processing
class WorkerPool {
    constructor(size = require('os').cpus().length) {
        this.workers = [];
        this.queue = [];
        this.activeJobs = 0;
        this.size = size;
    }

    async init() {
        for (let i = 0; i < this.size; i++) {
            const worker = new Worker(`
                const { parentPort } = require('worker_threads');
                const fs = require('fs').promises;
                const mm = require('music-metadata');
                
                parentPort.on('message', async ({ id, filePath, type }) => {
                    try {
                        let result = { id, filePath };
                        
                        if (type === 'metadata') {
                            const metadata = await mm.parseFile(filePath, { 
                                duration: true,
                                skipCovers: false,
                                skipPostHeaders: true 
                            });
                            
                            result.metadata = {
                                artist: metadata.common?.artist || 'Unknown',
                                album: metadata.common?.album || 'Unknown',
                                year: metadata.common?.year || 'Unknown',
                                duration: metadata.format?.duration || 0
                            };
                            
                            if (metadata.common?.picture?.[0]) {
                                const pic = metadata.common.picture[0];
                                if (pic.data.length < 300000) { // 300KB limit
                                    result.thumbnail = {
                                        format: pic.format,
                                        data: pic.data.toString('base64')
                                    };
                                }
                            }
                        }
                        
                        parentPort.postMessage({ success: true, ...result });
                    } catch (error) {
                        parentPort.postMessage({ 
                            success: false, 
                            id, 
                            error: error.message 
                        });
                    }
                });
            `, { eval: true });

            this.workers.push({
                worker,
                busy: false,
                jobs: new Map()
            });
        }
    }

    async process(filePath, type) {
        return new Promise((resolve, reject) => {
            const id = crypto.randomUUID();
            const job = { id, filePath, type, resolve, reject };

            const freeWorker = this.workers.find(w => !w.busy);
            if (freeWorker) {
                this.executeJob(freeWorker, job);
            } else {
                this.queue.push(job);
            }
        });
    }

    executeJob(workerInfo, job) {
        workerInfo.busy = true;
        workerInfo.jobs.set(job.id, job);

        const onMessage = (result) => {
            if (result.id === job.id) {
                workerInfo.worker.off('message', onMessage);
                workerInfo.busy = false;
                workerInfo.jobs.delete(job.id);

                if (result.success) {
                    job.resolve(result);
                } else {
                    job.reject(new Error(result.error));
                }

                // Process next job in queue
                if (this.queue.length > 0) {
                    const nextJob = this.queue.shift();
                    this.executeJob(workerInfo, nextJob);
                }
            }
        };

        workerInfo.worker.on('message', onMessage);
        workerInfo.worker.postMessage(job);
    }

    async destroy() {
        await Promise.all(this.workers.map(w => w.worker.terminate()));
    }
}

// Ultra-fast file scanner with streaming
async function* scanFilesStream(directory, formats) {
    const stack = [directory];

    while (stack.length > 0) {
        const currentDir = stack.pop();

        try {
            const entries = await fs.readdir(currentDir, { withFileTypes: true });
            const batch = [];

            for (const entry of entries) {
                const fullPath = path.join(currentDir, entry.name);

                if (entry.isDirectory()) {
                    stack.push(fullPath);
                } else if (entry.isFile()) {
                    const ext = path.extname(entry.name).toLowerCase();
                    if (formats.has(ext)) {
                        batch.push(fullPath);

                        // Yield in batches for better performance
                        if (batch.length >= 100) {
                            yield batch.splice(0);
                        }
                    }
                }
            }

            if (batch.length > 0) {
                yield batch;
            }
        } catch (error) {
            console.error(`Scan error in ${currentDir}:`, error);
        }
    }
}

module.exports = {
    MediaCache,
    WorkerPool,
    scanFilesStream,

    // Utility function to generate a unique file hash
    generateFileHash: (filePath, stats) => {
        return new Promise((resolve, reject) => {
            try {
                // Create a unique hash based on file properties
                const hashInput = `${filePath}|${stats.size}|${stats.mtimeMs}|${stats.birthtimeMs}`;
                const hash = crypto.createHash('md5').update(hashInput).digest('hex');
                resolve(hash);
            } catch (error) {
                reject(error);
            }
        });
    },

    // Utility function to parse metadata
    parseMetadata: async (filePath) => {
        try {
            const metadata = await mm.parseFile(filePath, {
                duration: true,
                skipCovers: false,
                skipPostHeaders: true
            });
            return metadata;
        } catch (error) {
            console.error(`Error parsing metadata for ${filePath}:`, error);
            return null;
        }
    }
};
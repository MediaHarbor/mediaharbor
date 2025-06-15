// fileDiscoveryWorker.js
const { parentPort } = require('worker_threads');
const fs = require('fs/promises');
const path = require('path');

async function findMediaFiles(dir, formats) {
    const results = [];
    async function scan(directory) {
        const items = await fs.readdir(directory);
        for (const item of items) {
            const fullPath = path.join(directory, item);
            const stat = await fs.stat(fullPath);
            if (stat.isDirectory()) {
                await scan(fullPath);
            } else if (formats.has(path.extname(item).toLowerCase())) {
                results.push({ path: fullPath, size: stat.size, mtime: stat.mtime });
            }
        }
    }
    await scan(dir);
    return results;
}

parentPort.on('message', async ({ directory, formats }) => {
    try {
        const files = await findMediaFiles(directory, new Set(formats));
        parentPort.postMessage({ files });
    } catch (error) {
        parentPort.postMessage({ error: error.message });
    }
});
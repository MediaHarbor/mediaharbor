'use strict';

const https = require('https');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { decryptDeezerBuffer } = require('./crypto');

function downloadFile(url, destPath, headers = {}, onProgress = null) {
    return new Promise((resolve, reject) => {
        const parsed = new URL(url);
        const transport = parsed.protocol === 'https:' ? https : http;

        const options = {
            hostname: parsed.hostname,
            port: parsed.port || (parsed.protocol === 'https:' ? 443 : 80),
            path: parsed.pathname + parsed.search,
            method: 'GET',
            headers: {
                'User-Agent': 'Mozilla/5.0',
                ...headers,
            },
        };

        const req = transport.request(options, (res) => {
            if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                return downloadFile(res.headers.location, destPath, headers, onProgress)
                    .then(resolve)
                    .catch(reject);
            }
            if (res.statusCode !== 200) {
                return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
            }

            const totalBytes = parseInt(res.headers['content-length'] || '0', 10);
            let downloaded = 0;
            const out = fs.createWriteStream(destPath);

            res.on('data', (chunk) => {
                downloaded += chunk.length;
                if (onProgress) onProgress(downloaded, totalBytes);
            });
            res.pipe(out);
            out.on('finish', () => resolve(destPath));
            out.on('error', reject);
            res.on('error', reject);
        });

        req.on('error', reject);
        req.end();
    });
}

function downloadAndDecryptDeezer(url, trackId, destPath, headers = {}, onProgress = null) {
    return new Promise((resolve, reject) => {
        const parsed = new URL(url);
        const transport = parsed.protocol === 'https:' ? https : http;

        const options = {
            hostname: parsed.hostname,
            port: parsed.port || (parsed.protocol === 'https:' ? 443 : 80),
            path: parsed.pathname + parsed.search,
            method: 'GET',
            headers: { 'User-Agent': 'Mozilla/5.0', ...headers },
        };

        const req = transport.request(options, (res) => {
            if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
                return downloadAndDecryptDeezer(
                    res.headers.location, trackId, destPath, headers, onProgress
                ).then(resolve).catch(reject);
            }
            if (res.statusCode !== 200) {
                return reject(new Error(`HTTP ${res.statusCode} for Deezer stream`));
            }

            const totalBytes = parseInt(res.headers['content-length'] || '0', 10);
            let downloaded = 0;
            const chunks = [];

            res.on('data', (chunk) => {
                chunks.push(chunk);
                downloaded += chunk.length;
                if (onProgress) onProgress(downloaded, totalBytes);
            });

            res.on('end', () => {
                try {
                    const encryptedBuf = Buffer.concat(chunks);
                    const decrypted = decryptDeezerBuffer(trackId, encryptedBuf);
                    fs.writeFile(destPath, decrypted, (err) => {
                        if (err) reject(err);
                        else resolve(destPath);
                    });
                } catch (err) {
                    reject(err);
                }
            });

            res.on('error', reject);
        });

        req.on('error', reject);
        req.end();
    });
}

function streamAndDecryptDeezer(url, trackId, headers = {}) {
    const { PassThrough } = require('stream');
    const output = new PassThrough();

    const parsed = new URL(url);
    const transport = parsed.protocol === 'https:' ? https : http;

    const options = {
        hostname: parsed.hostname,
        port: parsed.port || (parsed.protocol === 'https:' ? 443 : 80),
        path: parsed.pathname + parsed.search,
        method: 'GET',
        headers: { 'User-Agent': 'Mozilla/5.0', ...headers },
    };

    const req = transport.request(options, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
            const redirected = streamAndDecryptDeezer(res.headers.location, trackId, headers);
            redirected.pipe(output);
            return;
        }
        if (res.statusCode !== 200) {
            output.destroy(new Error(`HTTP ${res.statusCode} for Deezer stream`));
            return;
        }

        const { generateDeezerBlowfishKey, decryptDeezerChunk } = require('./crypto');
        const key = generateDeezerBlowfishKey(trackId);
        const CHUNK = 6144;
        const ENCRYPTED = 2048;
        let buf = Buffer.alloc(0);

        res.on('data', (data) => {
            buf = Buffer.concat([buf, data]);
            while (buf.length >= CHUNK) {
                const block = buf.slice(0, CHUNK);
                buf = buf.slice(CHUNK);
                const decryptedPart = decryptDeezerChunk(key, block.slice(0, ENCRYPTED));
                output.write(Buffer.concat([decryptedPart, block.slice(ENCRYPTED)]));
            }
        });

        res.on('end', () => {
            if (buf.length > 0) {
                if (buf.length >= ENCRYPTED) {
                    const decryptedPart = decryptDeezerChunk(key, buf.slice(0, ENCRYPTED));
                    output.write(Buffer.concat([decryptedPart, buf.slice(ENCRYPTED)]));
                } else {
                    output.write(buf);
                }
            }
            output.end();
        });

        res.on('error', (err) => output.destroy(err));
    });

    req.on('error', (err) => output.destroy(err));
    req.end();

    return output;
}

async function downloadSegments(segmentUrls, destPath, headers = {}, onProgress = null) {
    const tmpFiles = [];
    const totalSegments = segmentUrls.length;

    for (let i = 0; i < totalSegments; i++) {
        const tmp = path.join(os.tmpdir(), `mh_seg_${Date.now()}_${i}`);
        await downloadFile(segmentUrls[i], tmp, headers, null);
        tmpFiles.push(tmp);
        if (onProgress) onProgress(i + 1, totalSegments);
    }

    const out = fs.createWriteStream(destPath);
    for (const tmp of tmpFiles) {
        await new Promise((resolve, reject) => {
            const src = fs.createReadStream(tmp);
            src.pipe(out, { end: false });
            src.on('end', resolve);
            src.on('error', reject);
        });
        fs.unlink(tmp, () => {});
    }

    await new Promise((resolve, reject) => {
        out.end(resolve);
        out.on('error', reject);
    });

    return destPath;
}

module.exports = {
    downloadFile,
    downloadAndDecryptDeezer,
    streamAndDecryptDeezer,
    downloadSegments,
};

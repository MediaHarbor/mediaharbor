const { spawn } = require('child_process');
const os = require('os');
const { getVenvPython, isVenvReady } = require('../venvManager');

const _cache = new Map();
const CACHE_TTL_MS = 5 * 60 * 60 * 1000;

function extractVideoId(youtubeUrl) {
    try {
        return new URL(youtubeUrl).searchParams.get('v') || null;
    } catch {
        return null;
    }
}

function _buildCandidates() {
    const list = [
        { cmd: 'yt-dlp', args: [] },
    ];
    // Prefer the MediaHarbor venv's python -m yt_dlp if the venv is set up
    if (isVenvReady()) {
        list.push({ cmd: getVenvPython(), args: ['-m', 'yt_dlp'] });
    }
    list.push(
        { cmd: 'python',  args: ['-m', 'yt_dlp'] },
        { cmd: 'python3', args: ['-m', 'yt_dlp'] },
        { cmd: 'py',      args: ['-m', 'yt_dlp'] },
    );
    return list;
}

const YTDLP_FLAGS = [
    '-f', 'bestaudio/best',
    '--get-url',
    '--no-playlist',
    '--no-warnings',
    '--quiet',
];

const YTDLP_VIDEO_FLAGS = [
    '-f', 'bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best',
    '--get-url',
    '--no-playlist',
    '--no-warnings',
    '--quiet',
];

function _spawnYtDlp(cmd, extraArgs, url, flags) {
    const ytFlags = flags || YTDLP_FLAGS;
    return new Promise((resolve, reject) => {
        const proc = spawn(cmd, [...extraArgs, ...ytFlags, url]);
        let stdout = '';
        let stderr = '';
        let killed = false;
        proc.stdout.on('data', (d) => { stdout += d.toString(); });
        proc.stderr.on('data', (d) => { stderr += d.toString(); });

        const timer = setTimeout(() => { killed = true; proc.kill(); reject(new Error('yt-dlp timed out — check your internet connection')); }, 60_000);

        proc.on('close', (code, signal) => {
            clearTimeout(timer);
            if (killed) return;
            // Return all non-empty lines (bestvideo+bestaudio yields 2 URLs)
            const lines = stdout.trim().split('\n').map(l => l.trim()).filter(Boolean);
            if (code === 0 && lines.length) return resolve(lines);

            const err = stderr.trim();
            if (signal === 'SIGTERM' || signal === 'SIGKILL' || code === null) {
                return reject(new Error('Playback was cancelled'));
            }
            if (err.includes('No video formats found') || err.includes('is not a valid URL') || err.includes('Unsupported URL')) {
                return reject(new Error(`Unsupported or invalid URL: ${url}`));
            }
            if (err.includes('HTTP Error 403') || err.includes('HTTP Error 429')) {
                return reject(new Error('Blocked by server — try again later'));
            }
            if (err.includes('getaddrinfo') || err.includes('network') || err.includes('URLError') || err.includes('Connection refused') || err.includes('timed out')) {
                return reject(new Error('No internet connection or server unreachable'));
            }
            reject(new Error(err || `yt-dlp failed with exit code ${code}`));
        });

        proc.on('error', (err) => {
            clearTimeout(timer);
            const tagged = new Error(err.message);
            tagged.code = err.code;
            reject(tagged);
        });
    });
}

async function _resolve(youtubeUrl) {
    let lastError;
    for (const { cmd, args } of _buildCandidates()) {
        try {
            const lines = await _spawnYtDlp(cmd, args, youtubeUrl);
            return lines[0]; // audio: just the first URL
        } catch (err) {
            lastError = err;
            if (err.code === 'ENOENT' || /no module named/i.test(err.message)) continue;
            throw err;
        }
    }
    throw new Error(
        'yt-dlp is not installed or not found. ' +
        'Install it from the MediaHarbor settings page (Dependencies → yt-dlp).'
    );
}

// Returns { videoUrl, audioUrl } or { videoUrl } if already combined
async function _resolveVideo(youtubeUrl) {
    let lastError;
    for (const { cmd, args } of _buildCandidates()) {
        try {
            const lines = await _spawnYtDlp(cmd, args, youtubeUrl, YTDLP_VIDEO_FLAGS);
            if (lines.length >= 2) return { videoUrl: lines[0], audioUrl: lines[1] };
            return { videoUrl: lines[0] }; // combined format
        } catch (err) {
            lastError = err;
            if (err.code === 'ENOENT' || /no module named/i.test(err.message)) continue;
            throw err;
        }
    }
    throw lastError || new Error('yt-dlp not found');
}

const _inflight = new Map();

function _normalizeWatchUrl(youtubeUrl) {
    try {
        const u = new URL(youtubeUrl);
        if (u.hostname === 'music.youtube.com') u.hostname = 'www.youtube.com';
        if (u.hostname === 'm.youtube.com') u.hostname = 'www.youtube.com';
        return u.toString();
    } catch {
        return youtubeUrl;
    }
}

function _extractPlayerResponse(html) {
    const match = html.match(/var ytInitialPlayerResponse\s*=\s*(\{.+?\});/s)
        || html.match(/ytInitialPlayerResponse\s*=\s*(\{.+?\});/s);
    if (!match) throw new Error('Could not extract YouTube player response');
    return JSON.parse(match[1]);
}

function _pickNativeStreamUrl(playerResponse) {
    const streamingData = playerResponse?.streamingData || {};
    const formats = [
        ...(streamingData.adaptiveFormats || []),
        ...(streamingData.formats || []),
    ].filter((format) => typeof format?.url === 'string' && format.url);

    const score = (format) => {
        const mime = String(format.mimeType || '');
        let s = Number(format.bitrate || format.averageBitrate || 0);
        if (/^audio\//.test(mime)) s += 5_000_000;
        else if (format.audioQuality || format.audioChannels) s += 2_500_000;
        return s;
    };

    const best = formats.sort((a, b) => score(b) - score(a))[0];
    if (best?.url) return best.url;

    if (typeof streamingData.serverAbrStreamingUrl === 'string' && streamingData.serverAbrStreamingUrl) {
        return streamingData.serverAbrStreamingUrl;
    }

    throw new Error('No native stream URL found in YouTube player response');
}

async function _resolveNative(youtubeUrl) {
    const watchUrl = _normalizeWatchUrl(youtubeUrl);
    const res = await fetch(watchUrl, {
        headers: {
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36',
            'Accept-Language': 'en-US,en;q=0.9',
        },
    });
    if (!res.ok) throw new Error(`YouTube watch page failed (HTTP ${res.status})`);
    const html = await res.text();
    const playerResponse = _extractPlayerResponse(html);

    const status = playerResponse?.playabilityStatus?.status;
    if (status && status !== 'OK') {
        const reason = playerResponse?.playabilityStatus?.reason || 'Video unavailable';
        throw new Error(reason);
    }

    return _pickNativeStreamUrl(playerResponse);
}

async function getStreamUrl(youtubeUrl, options = {}) {
    const videoId = extractVideoId(youtubeUrl) || youtubeUrl;

    const cached = _cache.get(videoId);
    if (cached && cached.expiresAt > Date.now()) return cached.url;

    if (_inflight.has(videoId)) return _inflight.get(videoId);

    const promise = (async () => {
        try {
            return await _resolveNative(youtubeUrl);
        } catch (nativeErr) {
            if (options.nativeOnly) throw nativeErr;
            return _resolve(youtubeUrl);
        }
    })().then((url) => {
        _cache.set(videoId, { url, expiresAt: Date.now() + CACHE_TTL_MS });
        _inflight.delete(videoId);
        return url;
    }).catch((err) => {
        _inflight.delete(videoId);
        throw err;
    });

    _inflight.set(videoId, promise);
    return promise;
}

function preResolve(youtubeUrls) {
    for (const url of youtubeUrls) {
        getStreamUrl(url).catch(() => {});
    }
}

/**
 * Resolve a YTMusic (or YouTube) URL to a working stream using yt-dlp (deciphers n-param),
 * then fetches that URL in Node.js and returns { stream, contentType } for local proxying.
 *
 * This avoids the HTTP 403 that native-scraped URLs get (n-parameter not deciphered).
 */
async function getYTMusicAudioStream(youtubeUrl) {
    const https = require('https');
    const http = require('http');
    const { PassThrough } = require('stream');
    const { URL } = require('url');

    const directUrl = await _resolve(youtubeUrl);

    // Parse mime type from URL params (e.g. mime=audio%2Fwebm)
    let contentType = 'audio/webm';
    try {
        const parsed = new URL(directUrl);
        const mime = parsed.searchParams.get('mime');
        if (mime) contentType = mime;
    } catch {}

    return new Promise((resolve, reject) => {
        const proto = directUrl.startsWith('https://') ? https : http;
        proto.get(directUrl, { headers: { 'User-Agent': 'Mozilla/5.0', Range: 'bytes=0-' } }, (res) => {
            if (res.statusCode >= 400) {
                res.resume();
                return reject(new Error(`YTMusic stream fetch failed: HTTP ${res.statusCode}`));
            }
            const pass = new PassThrough();
            res.pipe(pass);
            resolve({ stream: pass, contentType });
        }).on('error', reject);
    });
}

/**
 * Get a merged video+audio stream for a YouTube video via ffmpeg.
 * yt-dlp fetches the best video and audio URLs; ffmpeg merges them into
 * a streamable fragmented MP4 piped to stdout.
 */
async function getYouTubeVideoStream(youtubeUrl) {
    const { PassThrough } = require('stream');

    const { videoUrl, audioUrl } = await _resolveVideo(youtubeUrl);

    const pass = new PassThrough();
    const contentType = 'video/mp4';

    if (!audioUrl) {
        // Already a combined stream — just fetch and proxy it
        const https = require('https');
        const http = require('http');
        const proto = videoUrl.startsWith('https://') ? https : http;
        proto.get(videoUrl, { headers: { 'User-Agent': 'Mozilla/5.0', Range: 'bytes=0-' } }, (res) => {
            if (res.statusCode >= 400) { res.resume(); pass.destroy(new Error(`YouTube video fetch failed: HTTP ${res.statusCode}`)); return; }
            res.pipe(pass);
        }).on('error', (e) => pass.destroy(e));
        return { stream: pass, contentType };
    }

    // Merge separate video+audio tracks with ffmpeg
    const ffmpegArgs = [
        '-loglevel', 'error',
        '-i', videoUrl,
        '-i', audioUrl,
        '-c', 'copy',
        '-f', 'mp4',
        '-movflags', 'frag_keyframe+empty_moov+default_base_moof',
        'pipe:1',
    ];

    const ffmpeg = spawn('ffmpeg', ffmpegArgs);
    ffmpeg.stdout.pipe(pass);
    ffmpeg.stderr.on('data', () => {}); // suppress stderr
    ffmpeg.on('error', (e) => pass.destroy(e));
    ffmpeg.on('close', (code) => {
        if (code !== 0 && !pass.destroyed) pass.destroy(new Error(`ffmpeg exited with code ${code}`));
    });

    return { stream: pass, contentType };
}

module.exports = { getStreamUrl, preResolve, getYTMusicAudioStream, getYouTubeVideoStream };

'use strict';

const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

const VALID_SAMPLING_RATES = [44100, 48000, 88200, 96000, 176400, 192000];

const FLAC_LIB = 'flac';

const CODEC_MAP = {
    FLAC:   { lib: 'flac',       container: 'flac', lossless: true  },
    ALAC:   { lib: 'alac',       container: 'm4a',  lossless: true  },
    MP3:    { lib: 'libmp3lame', container: 'mp3',  lossless: false },
    AAC:    { lib: 'aac',        container: 'm4a',  lossless: false },
    OPUS:   { lib: 'libopus',    container: 'opus', lossless: false },
    VORBIS: { lib: 'libvorbis',  container: 'ogg',  lossless: false },
};

let _ffmpegPath = null;
function getFfmpegPath() {
    if (_ffmpegPath) return _ffmpegPath;
    try { _ffmpegPath = require('ffmpeg-static'); return _ffmpegPath; } catch {}
    try { _ffmpegPath = require('@ffmpeg-installer/ffmpeg').path; return _ffmpegPath; } catch {}
    const localPaths = process.platform === 'win32'
        ? [path.join(os.homedir(), 'ffmpeg', 'bin', 'ffmpeg.exe')]
        : [path.join(os.homedir(), '.local', 'bin', 'ffmpeg')];
    for (const p of localPaths) {
        if (fs.existsSync(p)) { _ffmpegPath = p; return _ffmpegPath; }
    }
    _ffmpegPath = 'ffmpeg';
    return _ffmpegPath;
}

function buildAformat(maxSamplingRate, maxBitDepth) {
    const parts = [];

    if (maxSamplingRate != null) {
        const hz = parseInt(maxSamplingRate, 10);
        const allowed = VALID_SAMPLING_RATES.filter(r => r <= hz);
        if (allowed.length > 0) {
            parts.push(`sample_rates=${allowed.join('|')}`);
        }
    }

    if (maxBitDepth != null) {
        const depth = parseInt(maxBitDepth, 10);
        const fmts = depth >= 24 ? ['s32p', 's32', 's16p', 's16'] : ['s16p', 's16'];
        parts.push(`sample_fmts=${fmts.join('|')}`);
    }

    return parts.length > 0 ? `aformat=${parts.join(':')}` : null;
}

function getLossyBitrateArgs(codec, kbps) {
    const rate = parseInt(kbps, 10) || 320;
    if (codec === 'MP3') {
        const vbr = [[245,0],[225,1],[190,2],[175,3],[165,4],[130,5],[115,6],[100,7],[85,8],[65,9]];
        if (rate >= 320) return ['-b:a', '320k'];
        for (const [threshold, q] of vbr) {
            if (rate >= threshold) return ['-q:a', String(q)];
        }
        return ['-q:a', '9'];
    }
    return ['-b:a', `${rate}k`];
}

async function convertAudio(inputPath, settings) {
    if (!settings.conversion_check) return inputPath;

    const codec = (settings.conversion_codec || 'FLAC').toUpperCase();
    const info = CODEC_MAP[codec];
    if (!info) throw new Error(`converter: unknown codec "${codec}"`);

    const inputExt = path.extname(inputPath).toLowerCase().slice(1);
    const aformat = buildAformat(settings.conversion_sampling_rate, settings.conversion_bit_depth);

    if (info.lossless && codec === 'FLAC' && inputExt === 'flac' && !aformat) return inputPath;

    const ffmpeg = getFfmpegPath();
    const outputPath = inputPath.replace(/\.[^/.]+$/, '.' + info.container);
    const tmpPath = outputPath + '.conv.tmp.' + info.container;

    const args = ['-y', '-loglevel', 'error', '-i', inputPath];

    if (info.lossless) {
        args.push('-c:a', info.lib);
        if (aformat) args.push('-af', aformat);
        args.push('-c:v', 'copy');
    } else {
        args.push('-c:a', info.lib);
        args.push(...getLossyBitrateArgs(codec, settings.conversion_lossy_bitrate || 320));
        if (aformat) args.push('-af', aformat);
        args.push('-vn');
    }

    args.push(tmpPath);

    await new Promise((resolve, reject) => {
        const proc = spawn(ffmpeg, args, { windowsHide: true });
        let stderr = '';
        proc.stderr.on('data', d => { stderr += d.toString(); });
        proc.on('close', code => {
            if (code !== 0) {
                try { fs.unlink(tmpPath, () => {}); } catch {}
                reject(new Error(`converter: ffmpeg failed (${code}): ${stderr.slice(0, 300)}`));
            } else {
                resolve();
            }
        });
        proc.on('error', reject);
    });

    if (outputPath !== inputPath) {
        try { fs.unlinkSync(inputPath); } catch {}
    }
    fs.renameSync(tmpPath, outputPath);
    return outputPath;
}

module.exports = { convertAudio };


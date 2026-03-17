'use strict';

const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

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

async function downloadCoverArt(coverUrl) {
    if (!coverUrl) return null;
    const { downloadFile } = require('./downloader');
    const tmpPath = path.join(os.tmpdir(), `mh_cover_${Date.now()}.jpg`);
    try {
        await downloadFile(coverUrl, tmpPath);
        const stat = fs.statSync(tmpPath);
        if (stat.size === 0) {
            fs.unlink(tmpPath, () => {});
            return null;
        }
        return tmpPath;
    } catch (e) {
        try { fs.unlink(tmpPath, () => {}); } catch {}
        return null;
    }
}

async function tagFile(filePath, metadata = {}, coverPath = null, excludeFields = []) {
    const ffmpeg = getFfmpegPath();
    const ext = path.extname(filePath).toLowerCase().slice(1);
    const tmpOut = filePath + '.tagged.tmp' + path.extname(filePath);

    const metaArgs = [];

    const trackNum = metadata.tracknumber != null ? String(metadata.tracknumber) : null;
    const trackTotal = metadata.tracktotal != null ? String(metadata.tracktotal) : null;
    const discNum = metadata.discnumber != null ? String(metadata.discnumber) : null;
    const discTotal = metadata.disctotal != null ? String(metadata.disctotal) : null;
    const trackStr = trackNum ? (trackTotal ? `${trackNum}/${trackTotal}` : trackNum) : null;
    const discStr = discNum ? (discTotal ? `${discNum}/${discTotal}` : discNum) : null;

    const fieldMap = {
        title:         'title',
        artist:        'artist',
        album:         'album',
        albumartist:   'album_artist',
        date:          'date',
        genre:         'genre',
        isrc:          'ISRC',
        comment:       'comment',
        description:   'description',
        purchase_date: 'purchase_date',
        grouping:      'grouping',
        compilation:   'compilation',
        composer:      'composer',
        lyricist:      'lyricist',
        producer:      'producer',
        engineer:      'engineer',
        mixer:         'mixer',
        copyright:     'copyright',
        lyrics:        'LYRICS',
        bpm:           'BPM',
        label:         'organization',
        upc:           'BARCODE',
        language:      'language',
        mood:          'mood',
    };

    const excluded = new Set(Array.isArray(excludeFields) ? excludeFields.map(f => String(f).toLowerCase()) : []);

    for (const [key, ffKey] of Object.entries(fieldMap)) {
        if (excluded.has(key.toLowerCase())) continue;
        if (metadata[key] != null && metadata[key] !== '') {
            metaArgs.push('-metadata', `${ffKey}=${String(metadata[key])}`);
        }
    }

    if (trackStr && !excluded.has('tracknumber')) metaArgs.push('-metadata', `track=${trackStr}`);
    if (discStr  && !excluded.has('discnumber'))  metaArgs.push('-metadata', `disc=${discStr}`);

    const args = ['-y', '-loglevel', 'error', '-i', filePath];

    if (coverPath) {
        args.push('-i', coverPath);
        args.push('-map', '0:a');
        args.push('-map', '1:v');
        if (ext === 'mp3') {
            args.push('-c:a', 'copy', '-c:v', 'copy', '-id3v2_version', '3');
            args.push('-metadata:s:v', 'title=Album cover', '-metadata:s:v', 'comment=Cover (front)');
        } else if (ext === 'flac') {
            args.push('-c:a', 'copy', '-c:v', 'mjpeg', '-q:v', '2');
            args.push('-disposition:v', 'attached_pic');
            args.push('-metadata:s:v', 'title=Album cover', '-metadata:s:v', 'comment=Cover (front)');
        } else if (ext === 'ogg' || ext === 'opus') {
            args.push('-c:a', 'copy', '-c:v', 'copy');
            args.push('-disposition:v', 'attached_pic');
            args.push('-metadata:s:v', 'title=Album cover', '-metadata:s:v', 'comment=Cover (front)');
        } else {
            args.push('-c:a', 'copy', '-c:v', 'copy');
        }
    } else {
        args.push('-map', '0:a', '-c:a', 'copy');
    }

    args.push(...metaArgs, tmpOut);

    await new Promise((resolve, reject) => {
        const proc = spawn(ffmpeg, args, { windowsHide: true });
        let stderr = '';
        proc.stderr.on('data', (d) => { stderr += d.toString(); });
        proc.on('close', (code) => {
            if (code !== 0) {
                try { fs.unlink(tmpOut, () => {}); } catch {}
                reject(new Error(`ffmpeg tagger failed (${code}): ${stderr.slice(0, 300)}`));
            } else {
                resolve();
            }
        });
        proc.on('error', reject);
    });

    fs.unlinkSync(filePath);
    fs.renameSync(tmpOut, filePath);

    if (coverPath) {
        fs.unlink(coverPath, () => {});
    }
}

module.exports = { tagFile, downloadCoverArt };

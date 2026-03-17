const { PassThrough } = require('stream');
const { spawn } = require('child_process');
const https = require('https');
const fs = require('fs');
const path = require('path');
const { getVenvPython } = require('../venvManager');
const { decryptMp4, createDecryptState, decryptSegmentBuf } = require('./mp4decrypt');

const APPLE_MUSIC_HOMEPAGE = 'https://music.apple.com';
const AMP_API_URL = 'https://amp-api.music.apple.com';
const WEBPLAYBACK_API_URL = 'https://play.itunes.apple.com/WebObjects/MZPlay.woa/wa/webPlayback';
const LICENSE_API_URL = 'https://play.itunes.apple.com/WebObjects/MZPlay.woa/wa/acquireWebPlaybackLicense';

const APPLE_MUSIC_URL_RE = /https:\/\/(?:classical\.)?music\.apple\.com\/([a-z]{2})\/(artist|album|playlist|song|music-video|post)\/[^/]*(?:\/([^/?]*))?(?:\?i=)?([0-9a-z]*)?/;

function parseAppleMusicUrl(url) {
    const match = url.match(APPLE_MUSIC_URL_RE);
    if (!match) return null;
    const storefront = match[1];
    const resourceType = match[2];
    const primaryId = match[3];
    const subId = match[4];
    const id = subId || primaryId;
    return { storefront, id, resourceType };
}

function getMediaUserToken(cookiesPath) {
    if (!cookiesPath || !fs.existsSync(cookiesPath)) {
        throw new Error('Apple Music cookies file not found. Go to Settings → Apple → set Cookies Path.');
    }
    const content = fs.readFileSync(cookiesPath, 'utf8');
    const lines = content.split('\n');
    // gamdl uses APPLE_MUSIC_COOKIE_DOMAIN = ".music.apple.com" — must match exactly
    // First pass: prefer .music.apple.com domain
    for (const line of lines) {
        if (line.startsWith('#') || line.trim() === '') continue;
        const parts = line.split('\t');
        if (parts.length >= 7 && parts[5] === 'media-user-token') {
            const domain = parts[0]; // e.g. ".music.apple.com"
            if (domain === '.music.apple.com' || domain === 'music.apple.com') {
                return parts[6].trim();
            }
        }
    }
    // Second pass: accept any apple.com media-user-token
    for (const line of lines) {
        if (line.startsWith('#') || line.trim() === '') continue;
        const parts = line.split('\t');
        if (parts.length >= 7 && parts[5] === 'media-user-token' && parts[0].includes('apple.com')) {
            return parts[6].trim();
        }
    }
    throw new Error('media-user-token not found in cookies file. Make sure you exported cookies from music.apple.com while logged in.');
}

/**
 * Returns cookies from the cookies file matching .music.apple.com as a Cookie header string.
 * gamdl uses APPLE_MUSIC_COOKIE_DOMAIN = ".music.apple.com" — only those cookies should be sent.
 */
function buildAppleCookieHeader(cookiesPath) {
    if (!cookiesPath || !fs.existsSync(cookiesPath)) return '';
    try {
        const content = fs.readFileSync(cookiesPath, 'utf8');
        const pairs = [];
        for (const line of content.split('\n')) {
            if (line.startsWith('#') || line.trim() === '') continue;
            const parts = line.split('\t');
            if (parts.length < 7) continue;
            const domain = parts[0];
            const name = parts[5];
            const value = parts[6].trim();
            if (domain === '.music.apple.com' || domain === 'music.apple.com') {
                pairs.push(`${name}=${value}`);
            }
        }
        return pairs.join('; ');
    } catch (_) {
        return '';
    }
}

/**
 * HTTPS POST using Node.js native https module.
 * Electron's global fetch uses Chromium's network stack which treats Cookie as a
 * forbidden header and may strip it. Node's https module has no such restriction.
 */
function httpsPost(urlStr, headers, bodyObj) {
    return new Promise((resolve, reject) => {
        const body = JSON.stringify(bodyObj);
        const parsed = new URL(urlStr);
        const options = {
            hostname: parsed.hostname,
            port: 443,
            path: parsed.pathname + parsed.search,
            method: 'POST',
            headers: {
                ...headers,
                'Content-Type': 'application/json',
                'Content-Length': Buffer.byteLength(body),
            },
        };
        const req = https.request(options, (res) => {
            const chunks = [];
            res.on('data', chunk => chunks.push(chunk));
            res.on('end', () => {
                const data = Buffer.concat(chunks).toString('utf8');
                resolve({
                    ok: res.statusCode >= 200 && res.statusCode < 300,
                    status: res.statusCode,
                    json: () => JSON.parse(data),
                    text: () => data,
                });
            });
        });
        req.on('error', reject);
        req.write(body);
        req.end();
    });
}

/** GET using Node.js native https — bypasses Electron's Cookie header stripping. */
function httpsGet(urlStr, headers) {
    return new Promise((resolve, reject) => {
        const parsed = new URL(urlStr);
        const options = {
            hostname: parsed.hostname,
            port: 443,
            path: parsed.pathname + parsed.search,
            method: 'GET',
            headers: headers || {},
        };
        const req = https.request(options, (res) => {
            // Follow redirects (max 5)
            if ((res.statusCode === 301 || res.statusCode === 302 || res.statusCode === 307 || res.statusCode === 308) && res.headers.location) {
                return httpsGet(res.headers.location, headers).then(resolve, reject);
            }
            const chunks = [];
            res.on('data', chunk => chunks.push(chunk));
            res.on('end', () => {
                const raw = Buffer.concat(chunks);
                resolve({
                    ok: res.statusCode >= 200 && res.statusCode < 300,
                    status: res.statusCode,
                    text: () => raw.toString('utf8'),
                    arrayBuffer: () => raw.buffer.slice(raw.byteOffset, raw.byteOffset + raw.byteLength),
                    buffer: () => raw,
                });
            });
        });
        req.on('error', reject);
        req.end();
    });
}

async function getDeveloperToken() {
    const UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36';
    const resp = await httpsGet(APPLE_MUSIC_HOMEPAGE, { 'User-Agent': UA });
    if (!resp.ok) throw new Error(`Apple Music homepage returned ${resp.status}`);
    const html = resp.text();

    const jsMatch = html.match(/\/(assets\/index-legacy[~-][^/"]+\.js)/);
    if (!jsMatch) throw new Error('Could not find index.js URI in Apple Music homepage');

    const jsResp = await httpsGet(`${APPLE_MUSIC_HOMEPAGE}/${jsMatch[1]}`, { 'User-Agent': UA });
    if (!jsResp.ok) throw new Error(`Apple Music index.js returned ${jsResp.status}`);
    const js = jsResp.text();

    const tokenMatch = js.match(/(?=eyJh)(.*?)(?=")/);
    if (!tokenMatch) throw new Error('Could not extract developer token from Apple Music JS');
    return tokenMatch[1];
}

async function getAccountStorefront(devToken, mediaUserToken) {
    const resp = await httpsGet(`${AMP_API_URL}/v1/me/account`, {
        'Authorization': `Bearer ${devToken}`,
        'Media-User-Token': mediaUserToken,
        'Origin': APPLE_MUSIC_HOMEPAGE,
        'Referer': APPLE_MUSIC_HOMEPAGE,
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
    });
    if (!resp.ok) return null;
    try {
        const data = JSON.parse(resp.text());
        return data?.meta?.subscription?.storefront || null;
    } catch (_) {
        return null;
    }
}

async function getMusicVideoHlsUrl(videoId, devToken, mediaUserToken, cookiesPath) {
    const cookieHeader = buildAppleCookieHeader(cookiesPath);
    const resp = await httpsPost(`${WEBPLAYBACK_API_URL}?l=en-US`, {
        'Authorization': `Bearer ${devToken}`,
        'Media-User-Token': mediaUserToken,
        'Cookie': cookieHeader,
        'Origin': APPLE_MUSIC_HOMEPAGE,
        'Referer': APPLE_MUSIC_HOMEPAGE,
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
        'accept': '*/*',
        'accept-language': 'en-US',
        'priority': 'u=1, i',
        'sec-ch-ua': '"Google Chrome";v="137", "Chromium";v="137", "Not/A)Brand";v="24"',
        'sec-ch-ua-mobile': '?0',
        'sec-ch-ua-platform': '"Windows"',
        'sec-fetch-dest': 'empty',
        'sec-fetch-mode': 'cors',
        'sec-fetch-site': 'same-site',
    }, {
        salableAdamId: videoId,
        language: 'en-US',
    });
    if (!resp.ok) {
        throw new Error(`Apple Music WebPlayback API returned ${resp.status}: ${resp.text().substring(0, 200)}`);
    }
    const data = resp.json();
    if (data.dialog || data.failureType) {
        const msg = data.customerMessage || data.failureType || 'subscription may be inactive';
        throw new Error(`Apple Music WebPlayback returned failure: ${msg}`);
    }
    const hlsUrl = data?.songList?.[0]?.['hls-playlist-url'];
    if (!hlsUrl) throw new Error('No HLS playlist URL in Apple Music WebPlayback response for music video');
    return hlsUrl;
}

async function getSongMetadata(songId, storefront, devToken, mediaUserToken) {
    const url = `${AMP_API_URL}/v1/catalog/${storefront}/songs/${songId}?extend=extendedAssetUrls&include=albums`;
    const resp = await httpsGet(url, {
        'Authorization': `Bearer ${devToken}`,
        'Media-User-Token': mediaUserToken,
        'Origin': APPLE_MUSIC_HOMEPAGE,
        'Referer': APPLE_MUSIC_HOMEPAGE,
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
    });
    if (!resp.ok) {
        throw new Error(`Apple Music API song request failed: ${resp.status} — ${resp.text().substring(0, 200)}`);
    }
    try {
        const data = JSON.parse(resp.text());
        return data.data?.[0];
    } catch (e) {
        throw new Error(`Apple Music API song response parse failed: ${e.message}`);
    }
}

async function getWebPlayback(songId, devToken, mediaUserToken, cookiesPath) {
    const cookieHeader = buildAppleCookieHeader(cookiesPath);
    const resp = await httpsPost(`${WEBPLAYBACK_API_URL}?l=en-US`, {
        'Authorization': `Bearer ${devToken}`,
        'Media-User-Token': mediaUserToken,
        'Cookie': cookieHeader,
        'Origin': APPLE_MUSIC_HOMEPAGE,
        'Referer': APPLE_MUSIC_HOMEPAGE,
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
        'accept': '*/*',
        'accept-language': 'en-US',
        'priority': 'u=1, i',
        'sec-ch-ua': '"Google Chrome";v="137", "Chromium";v="137", "Not/A)Brand";v="24"',
        'sec-ch-ua-mobile': '?0',
        'sec-ch-ua-platform': '"Windows"',
        'sec-fetch-dest': 'empty',
        'sec-fetch-mode': 'cors',
        'sec-fetch-site': 'same-site',
    }, {
        salableAdamId: songId,
        language: 'en-US',
    });
    if (!resp.ok) throw new Error(`WebPlayback API returned ${resp.status}`);
    const data = resp.json();
    if (data.dialog || data.failureType) {
        const msg = data.customerMessage || data.failureType || 'subscription may be inactive';
        throw new Error(`WebPlayback returned failure: ${msg}`);
    }
    return data;
}

async function parseHlsForStream(m3u8Url, songId, devToken, mediaUserToken) {
    const headers = {
        'Authorization': `Bearer ${devToken}`,
        'Media-User-Token': mediaUserToken,
        // gamdl sends media-user-token as a cookie on ALL requests (including manifest fetches).
        // Without it, Apple omits the Widevine PSSH from EXT-X-SESSION-DATA.
        // Use httpsGet (not Electron fetch) so Cookie header is NOT stripped.
        'Cookie': `media-user-token=${mediaUserToken}`,
        'Origin': APPLE_MUSIC_HOMEPAGE,
        'Referer': APPLE_MUSIC_HOMEPAGE,
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
        'accept': '*/*',
        'accept-language': 'en-US',
        'priority': 'u=1, i',
        'sec-ch-ua': '"Google Chrome";v="137", "Chromium";v="137", "Not/A)Brand";v="24"',
        'sec-ch-ua-mobile': '?0',
        'sec-ch-ua-platform': '"Windows"',
        'sec-fetch-dest': 'empty',
        'sec-fetch-mode': 'cors',
        'sec-fetch-site': 'cross-site',
    };
    const resp = await httpsGet(m3u8Url, headers);
    if (!resp.ok) throw new Error(`HLS master fetch failed: ${resp.status}`);
    const m3u8Text = resp.text();

    const lines = m3u8Text.split('\n');
    let bestStream = null;
    let bestBandwidth = 0;

    for (let i = 0; i < lines.length; i++) {
        const line = lines[i].trim();
        if (line.startsWith('#EXT-X-STREAM-INF:')) {
            const bwMatch = line.match(/AVERAGE-BANDWIDTH=(\d+)/);
            const audioMatch = line.match(/AUDIO="([^"]+)"/);
            const bandwidth = bwMatch ? parseInt(bwMatch[1]) : 0;
            const audio = audioMatch ? audioMatch[1] : '';

            if (audio.match(/^audio-stereo-\d+$/) && bandwidth > bestBandwidth) {
                bestBandwidth = bandwidth;
                const nextLine = lines[i + 1]?.trim();
                if (nextLine && !nextLine.startsWith('#')) {
                    bestStream = nextLine;
                }
            }
        }
    }

    if (!bestStream) {
        for (let i = 0; i < lines.length; i++) {
            const line = lines[i].trim();
            if (line.startsWith('#EXT-X-STREAM-INF:')) {
                const bwMatch = line.match(/AVERAGE-BANDWIDTH=(\d+)/);
                const bandwidth = bwMatch ? parseInt(bwMatch[1]) : 0;
                if (bandwidth > bestBandwidth) {
                    bestBandwidth = bandwidth;
                    const nextLine = lines[i + 1]?.trim();
                    if (nextLine && !nextLine.startsWith('#')) {
                        bestStream = nextLine;
                    }
                }
            }
        }
    }

    if (!bestStream) throw new Error('No stream found in HLS manifest');

    const baseUrl = m3u8Url.substring(0, m3u8Url.lastIndexOf('/') + 1);
    const streamUrl = bestStream.startsWith('http') ? bestStream : baseUrl + bestStream;

    let widevinePssh = null;

    // Widevine system ID bytes: edef8ba9-79d6-4ace-a3c8-27dcd51d21ed
    const WIDEVINE_SYSTEM_ID = Buffer.from('edef8ba979d64acea3c827dcd51d21ed', 'hex');

    // Apple Music HLS manifests include both a "prefetch" PSSH (KID = "s1/e1", used for
    // desc_index=0 init samples, decrypted by DEFAULT_SONG_DECRYPTION_KEY) and per-track
    // PSHHs for the actual audio content. We must pick the TRACK PSSH, never the prefetch.
    function isPrefetchPssh(uri) {
        // Prefetch KID = 000000000000000073312f6531202020 ("s1/e1   " in ASCII)
        try {
            const b64 = uri.startsWith('data:') ? uri.split(',').pop() : uri;
            return Buffer.from(b64, 'base64').includes(Buffer.from('73312f6531', 'hex'));
        } catch (_) { return false; }
    }

    // Scan MP4 boxes recursively for a non-prefetch Widevine PSSH box, return base64-encoded box
    function extractPsshFromMp4(buf) {
        const results = [];
        function scan(start, end) {
            let off = start;
            while (off + 8 <= end) {
                const size = buf.readUInt32BE(off);
                if (size < 8 || off + size > end) break;
                const type = buf.slice(off + 4, off + 8).toString('ascii');
                if (type === 'pssh' && size >= 28) {
                    const sysId = buf.slice(off + 12, off + 28);
                    if (sysId.equals(WIDEVINE_SYSTEM_ID)) {
                        results.push(buf.slice(off, off + size).toString('base64'));
                    }
                }
                if (['moov', 'trak', 'mdia', 'minf', 'stbl', 'udta', 'moof', 'traf'].includes(type)) {
                    scan(off + 8, off + size);
                }
                off += size;
            }
        }
        scan(0, buf.length);
        // Prefer the first non-prefetch PSSH; fall back to any PSSH
        return results.find(b64 => !isPrefetchPssh('data:text/plain;base64,' + b64)) || results[0] || null;
    }

    // Helper: extract a SESSION-DATA VALUE by DATA-ID, regardless of attribute order
    function getSessionDataValue(text, dataId) {
        for (const line of text.split('\n')) {
            const t = line.trim();
            if (!t.startsWith('#EXT-X-SESSION-DATA:')) continue;
            if (!t.includes(dataId)) continue;
            const m = t.match(/VALUE="([^"]+)"/i);
            if (m) return m[1];
        }
        return null;
    }

    // Helper: scan key lines in a manifest for the first non-prefetch Widevine URI
    function findWidevinePsshInManifest(text) {
        for (const line of text.split('\n')) {
            const t = line.trim();
            if (!t.startsWith('#EXT-X-KEY') && !t.startsWith('#EXT-X-SESSION-KEY')) continue;
            const lower = t.toLowerCase();
            if (lower.includes('edef8ba9') || lower.includes('com.widevine')) {
                const m = t.match(/URI="([^"]+)"/i);
                if (m && !isPrefetchPssh(m[1])) return m[1];
            }
        }
        return null;
    }

    // Helper: scan sessionKeyInfo for the first non-prefetch Widevine URI entry
    function findWidevineInSessionKeyInfo(sessionKeyInfo) {
        for (const drmMap of Object.values(sessionKeyInfo)) {
            if (!drmMap || typeof drmMap !== 'object') continue;
            for (const [urnKey, entry] of Object.entries(drmMap)) {
                if (urnKey.toLowerCase().includes('edef8ba9') && entry?.URI && !isPrefetchPssh(entry.URI))
                    return entry.URI;
            }
        }
        return null;
    }

    // 1. Primary: SESSION-DATA AudioSessionKeyInfo (attribute-order-independent)
    const sessionKeyInfoB64 = getSessionDataValue(m3u8Text, 'com.apple.hls.AudioSessionKeyInfo');
    if (sessionKeyInfoB64) {
        try {
            const sessionKeyInfo = JSON.parse(Buffer.from(sessionKeyInfoB64, 'base64').toString('utf8'));

            // Use audioAssetMetadata to find the track PSSH for the selected stream.
            // Key "1" in AUDIO-SESSION-KEY-IDS is always the prefetch — skip it.
            const assetMetaB64 = getSessionDataValue(m3u8Text, 'com.apple.hls.audioAssetMetadata');
            if (assetMetaB64 && !widevinePssh) {
                try {
                    const assetMetadata = JSON.parse(Buffer.from(assetMetaB64, 'base64').toString('utf8'));
                    // Sort so the selected stream's metadata entry is tried first
                    const bestBase = bestStream ? bestStream.replace(/\.m3u8$/, '').split('/').pop() : '';
                    const allMeta = Object.values(assetMetadata);
                    if (bestBase) allMeta.sort((a, b) => {
                        const aM = (a?.['FIRST-SEGMENT-URI'] || '').includes(bestBase) ? 0 : 1;
                        const bM = (b?.['FIRST-SEGMENT-URI'] || '').includes(bestBase) ? 0 : 1;
                        return aM - bM;
                    });
                    outer: for (const meta of allMeta) {
                        const keyIds = meta?.['AUDIO-SESSION-KEY-IDS'] || meta?.['audio-session-key-ids'];
                        if (!Array.isArray(keyIds)) continue;
                        for (const drmId of keyIds) {
                            const drmMap = sessionKeyInfo[drmId] || {};
                            for (const [urnKey, entry] of Object.entries(drmMap)) {
                                if (urnKey.toLowerCase().includes('edef8ba9') && entry?.URI && !isPrefetchPssh(entry.URI)) {
                                    widevinePssh = entry.URI;
                                    break outer;
                                }
                            }
                        }
                    }
                } catch (_) {}
            }

            // Fallback: scan all entries in sessionKeyInfo directly
            if (!widevinePssh) widevinePssh = findWidevineInSessionKeyInfo(sessionKeyInfo);
        } catch (_) {}
    }

    // 2. Check EXT-X-KEY lines in master manifest
    if (!widevinePssh) widevinePssh = findWidevinePsshInManifest(m3u8Text);

    // 3. Collect audio rendition URIs from EXT-X-MEDIA to check their playlists
    const audioRenditionUrls = [];
    for (const line of lines) {
        const t = line.trim();
        if (!t.startsWith('#EXT-X-MEDIA:')) continue;
        if (!t.toLowerCase().includes('type=audio')) continue;
        const uriMatch = t.match(/URI="([^"]+)"/i);
        if (uriMatch) {
            const u = uriMatch[1];
            audioRenditionUrls.push(u.startsWith('http') ? u : baseUrl + u);
        }
    }

    // Helper: find EXT-X-MAP init segment URL in a playlist text
    function getInitSegmentUrl(playlistText, playlistBaseUrl) {
        for (const line of playlistText.split('\n')) {
            const mapMatch = line.trim().match(/#EXT-X-MAP:URI="([^"]+)"/i);
            if (mapMatch) {
                const u = mapMatch[1];
                return u.startsWith('http') ? u : playlistBaseUrl + u;
            }
        }
        return null;
    }

    // 4. Fetch stream + audio rendition manifests: check EXT-X-KEY and init segments for PSSH
    if (!widevinePssh) {
        const fetchTargets = [streamUrl, ...audioRenditionUrls.slice(0, 3)];
        for (const targetUrl of fetchTargets) {
            if (widevinePssh) break;
            try {
                const r = await httpsGet(targetUrl, headers);
                if (!r.ok) continue;
                const text = r.text();
                const tBaseUrl = targetUrl.substring(0, targetUrl.lastIndexOf('/') + 1);

                // Check EXT-X-KEY in this playlist
                widevinePssh = findWidevinePsshInManifest(text);
                if (widevinePssh) break;

                // Fetch and parse the CMAF init segment for embedded PSSH boxes
                const initUrl = getInitSegmentUrl(text, tBaseUrl);
                if (initUrl) {
                    const initResp = await httpsGet(initUrl, {
                        'User-Agent': headers['User-Agent'],
                    });
                    if (initResp.ok) {
                        const initBuf = initResp.buffer();
                        const psshB64 = extractPsshFromMp4(initBuf);
                        if (psshB64) widevinePssh = `data:text/plain;base64,${psshB64}`;
                    }
                }
            } catch (_) {}
        }
    }

    // 5. (No Python fallback — legacy stream path handles FairPlay-only tracks)

    return { streamUrl, widevinePssh, mediaId: songId };
}

/**
 * Native JS replacement for getLegacyKeyViaPython.
 * Fetches the legacy webplayback stream info and extracts the Widevine PSSH URI
 * from the m3u8 keys[0].uri — no Python involved.
 * The actual CDM key exchange is done separately by getWidevineKeyViaPython.
 */
async function getLegacyStreamInfo(songId, devToken, mediaUserToken, cookiesPath) {
    const webplayback = await getWebPlayback(songId, devToken, mediaUserToken, cookiesPath);
    const assets = webplayback?.songList?.[0]?.assets;
    if (!Array.isArray(assets) || assets.length === 0) {
        throw new Error(`No assets in webplayback for legacy stream (keys: ${Object.keys(webplayback || {}).join(',')})`);
    }

    const asset = assets.find(a => a.flavor === '28:ctrp256')
        || assets.find(a => a.flavor === '32:ctrp64')
        || assets.find(a => a.URL);
    if (!asset) {
        const flavors = assets.map(a => a.flavor).join(', ');
        throw new Error(`No legacy asset found (available flavors: ${flavors})`);
    }

    const streamUrl = asset.URL;
    if (!streamUrl) throw new Error(`Legacy asset has no URL (flavor: ${asset.flavor})`);

    const m3u8Resp = await httpsGet(streamUrl, {
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
    });
    if (!m3u8Resp.ok) throw new Error(`Legacy m3u8 fetch failed: ${m3u8Resp.status}`);
    const m3u8Text = m3u8Resp.text();

    // Scan both EXT-X-KEY and EXT-X-SESSION-KEY for a URI (any key — legacy stream is Widevine-only)
    for (const line of m3u8Text.split('\n')) {
        const t = line.trim();
        if (!t.startsWith('#EXT-X-KEY:') && !t.startsWith('#EXT-X-SESSION-KEY:')) continue;
        const uriMatch = t.match(/URI="([^"]+)"/i);
        if (uriMatch) {
            return { streamUrl, widevinePssh: uriMatch[1], legacy: true };
        }
    }
    // Log a snippet of the manifest to help diagnose format issues
    const snippet = m3u8Text.substring(0, 500).replace(/\n/g, '\\n');
    throw new Error(`No EXT-X-KEY found in legacy m3u8. Manifest snippet: ${snippet}`);
}

function getWidevineKeyViaPython(pssh, wvdPath, songId, devToken, mediaUserToken, cookiesPath) {
    const pyScript = `
import sys, json, base64, asyncio
from pywidevine import PSSH, Cdm, Device
from gamdl.api.apple_music_api import AppleMusicApi

args = json.loads(sys.stdin.readline())

async def get_key():
    api = await AppleMusicApi.create_from_netscape_cookies(args["cookies_path"])

    device = Device.load(args["wvd_path"])
    cdm = Cdm.from_device(device)
    session_id = cdm.open()

    try:
        pssh_b64 = args["pssh"].split(",")[-1]
        try:
            decoded = base64.b64decode(pssh_b64 + '==')
            if len(decoded) < 32:
                raise ValueError("raw key ID")
            pssh_obj = PSSH(pssh_b64)
        except Exception:
            # Legacy format: raw 16-byte key ID, not a full PSSH box
            from pywidevine.license_protocol_pb2 import WidevinePsshData
            key_id_bytes = base64.b64decode(pssh_b64)
            widevine_pssh_data = WidevinePsshData()
            widevine_pssh_data.algorithm = 1
            widevine_pssh_data.key_ids.append(key_id_bytes)
            pssh_obj = PSSH(widevine_pssh_data.SerializeToString())
        challenge = cdm.get_license_challenge(session_id, pssh_obj)
        challenge_b64 = base64.b64encode(challenge).decode()

        license_data = await api.get_license_exchange(
            track_id=str(args["song_id"]),
            track_uri=args["pssh"],
            challenge=challenge_b64,
        )

        cdm.parse_license(session_id, license_data["license"])
        keys = [k for k in cdm.get_keys(session_id) if k.type == "CONTENT"]

        if not keys:
            return {"error": "No content keys in license response"}

        kid_hex = keys[0].kid.hex if isinstance(keys[0].kid.hex, str) else keys[0].kid.hex()
        key_hex = keys[0].key.hex() if callable(keys[0].key.hex) else keys[0].key.hex
        return {"key": key_hex, "kid": kid_hex}
    finally:
        cdm.close(session_id)

try:
    result = asyncio.run(get_key())
    sys.stdout.write(json.dumps(result) + "\\n")
except Exception as e:
    sys.stdout.write(json.dumps({"error": str(e)}) + "\\n")
    sys.exit(1)
`;

    return new Promise((resolve, reject) => {
        const child = spawn(getVenvPython(), ['-c', pyScript]);
        let stderr = '';
        child.stderr.on('data', (d) => { stderr += d; });
        child.on('error', (err) => reject(new Error(`pywidevine spawn failed: ${err.message}`)));

        let buf = '';
        const lines = [];
        child.stdout.on('data', (chunk) => {
            buf += chunk.toString();
            let nl;
            while ((nl = buf.indexOf('\n')) !== -1) {
                lines.push(buf.slice(0, nl));
                buf = buf.slice(nl + 1);
            }
        });

        child.stdin.write(JSON.stringify({
            pssh,
            wvd_path: wvdPath,
            song_id: songId,
            cookies_path: cookiesPath,
        }) + '\n');
        child.stdin.end();

        child.on('close', (code) => {
            if (buf.trim()) lines.push(buf.trim());
            const lastLine = lines.pop();
            if (!lastLine) return reject(new Error(`pywidevine exited ${code} with no output. stderr: ${stderr.substring(0, 300)}`));
            try {
                const result = JSON.parse(lastLine);
                if (result.error) return reject(new Error(result.error));
                resolve({ kid: result.kid, key: result.key });
            } catch (e) {
                reject(new Error(`Failed to parse pywidevine output: ${lastLine.substring(0, 200)}`));
            }
        });
    });
}



let _cachedDevToken = null;
let _devTokenExpiry = 0;

async function getCachedDevToken() {
    const now = Date.now();
    if (_cachedDevToken && now < _devTokenExpiry) return _cachedDevToken;
    _cachedDevToken = await getDeveloperToken();
    _devTokenExpiry = now + 3600000;
    return _cachedDevToken;
}

function isConfigured(settings) {
    return !!settings.apple_cookies_path;
}

async function getTrackStream(url, settings) {
    const parsed = parseAppleMusicUrl(url);
    if (!parsed || !parsed.id) throw new Error('Could not parse Apple Music URL');
    const songId = parsed.id;
    const urlStorefront = parsed.storefront || 'us';

    const mediaUserToken = getMediaUserToken(settings.apple_cookies_path);
    const devToken = await getCachedDevToken();

    const accountStorefront = await getAccountStorefront(devToken, mediaUserToken);
    const storefront = accountStorefront || urlStorefront;

    if (parsed.resourceType === 'music-video') {
        const m3u8Url = await getMusicVideoHlsUrl(songId, devToken, mediaUserToken, settings.apple_cookies_path);
        const hlsInfo = await parseHlsForStream(m3u8Url, songId, devToken, mediaUserToken);
        if (!hlsInfo.widevinePssh) throw new Error('No Widevine PSSH found in Apple Music music-video HLS manifest');
        let wvdPath = settings.apple_wvd_path || await getGamdlWvdPath();
        if (!wvdPath || !fs.existsSync(wvdPath)) throw new Error('WVD file required for Apple Music. Set it in Settings → Apple → WVD Path.');
        return decryptAndStreamHls(hlsInfo, wvdPath, songId, devToken, mediaUserToken, settings.apple_cookies_path, 0, 'video/mp4');
    }

    const songMetadata = await getSongMetadata(songId, storefront, devToken, mediaUserToken);
    if (!songMetadata) throw new Error('Apple Music returned no song metadata');

    const durationMs = songMetadata.attributes?.durationInMillis || 0;
    const m3u8Url = songMetadata.attributes?.extendedAssetUrls?.enhancedHls;

    if (!m3u8Url) {
        throw new Error('No HLS stream URL available. Check that your Apple Music subscription is active.');
    }

    let wvdPath = settings.apple_wvd_path;
    if (!wvdPath) wvdPath = await getGamdlWvdPath();
    if (!wvdPath || !fs.existsSync(wvdPath)) {
        throw new Error('WVD file required for Apple Music. Set it in Settings → Apple → WVD Path, or ensure gamdl is installed.');
    }

    // Fire legacy stream info in parallel with enhanced HLS parsing (native JS, fast).
    // getLegacyStreamInfo only does HTTP calls — no Python. CDM (Python) is deferred.
    let legacyStreamError = null;
    const legacyStreamInfoPromise = getLegacyStreamInfo(songId, devToken, mediaUserToken, settings.apple_cookies_path)
        .catch((e) => { legacyStreamError = e; return null; });

    const hlsInfo = await parseHlsForStream(m3u8Url, songId, devToken, mediaUserToken);

    if (!hlsInfo.widevinePssh) {
        const legacyInfo = await legacyStreamInfoPromise;
        if (!legacyInfo) {
            const reason = legacyStreamError ? `: ${legacyStreamError.message}` : '';
            throw new Error(`No Widevine PSSH found in HLS manifest — cannot decrypt (legacy fallback failed${reason})`);
        }
        return decryptAndStreamHls(
            legacyInfo,
            wvdPath, songId, devToken, mediaUserToken, settings.apple_cookies_path, durationMs
        );
    }

    return decryptAndStreamHls(hlsInfo, wvdPath, songId, devToken, mediaUserToken, settings.apple_cookies_path, durationMs);
}

async function decryptAndStreamHls(hlsInfo, wvdPath, songId, devToken, mediaUserToken, cookiesPath, durationMs, contentType = 'audio/mp4') {
    const isVideo = contentType.startsWith('video/');
    const output = new PassThrough();

    const segFetchHeaders = {
        'Authorization': `Bearer ${devToken}`,
        'Media-User-Token': mediaUserToken,
        'Cookie': `media-user-token=${mediaUserToken}`,
        'Origin': APPLE_MUSIC_HOMEPAGE,
        'Referer': APPLE_MUSIC_HOMEPAGE,
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
        'accept': '*/*',
        'accept-language': 'en-US',
        'priority': 'u=1, i',
    };
    const plainHeaders = {
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
    };

    // Fire key exchange and segment manifest in parallel immediately
    const keyExchangePromise = getWidevineKeyViaPython(
        hlsInfo.widevinePssh, wvdPath, songId, devToken, mediaUserToken, cookiesPath
    );

    const segmentListPromise = (async () => {
        const streamM3u8Resp = await httpsGet(hlsInfo.streamUrl, segFetchHeaders);
        if (!streamM3u8Resp.ok) throw new Error(`Failed to fetch stream m3u8: ${streamM3u8Resp.status}`);
        const streamM3u8 = streamM3u8Resp.text();
        const baseUrl = hlsInfo.streamUrl.substring(0, hlsInfo.streamUrl.lastIndexOf('/') + 1);
        let initSegmentUrl = null;
        const mediaSegmentUrls = [];
        for (const line of streamM3u8.split('\n')) {
            const trimmed = line.trim();
            const mapMatch = trimmed.match(/#EXT-X-MAP:URI="([^"]+)"/);
            if (mapMatch) {
                initSegmentUrl = mapMatch[1].startsWith('http') ? mapMatch[1] : baseUrl + mapMatch[1];
                continue;
            }
            if (trimmed && !trimmed.startsWith('#')) {
                mediaSegmentUrls.push(trimmed.startsWith('http') ? trimmed : baseUrl + trimmed);
            }
        }
        if (!initSegmentUrl || mediaSegmentUrls.length === 0)
            throw new Error(`No segments found in m3u8 (init: ${!!initSegmentUrl}, segments: ${mediaSegmentUrls.length})`);
        return { initSegmentUrl, mediaSegmentUrls };
    })();

    // Start the async pipeline — return the stream immediately so the player can connect
    ;(async () => {
        try {
            // Wait for key + segment list + init segment in parallel
            const [{ key: decryptKeyHex }, { initSegmentUrl, mediaSegmentUrls }] = await Promise.all([
                keyExchangePromise, segmentListPromise,
            ]);

            const initResp = await httpsGet(initSegmentUrl, plainHeaders);
            if (!initResp.ok) throw new Error(`Init segment fetch failed: ${initResp.status}`);
            const initBuf = initResp.buffer();

            const isLegacy = !!hlsInfo.legacy;

            if (isVideo) {
                // Video: must buffer all segments first for ffmpeg
                const segBuffers = [];
                const BATCH = 10;
                for (let i = 0; i < mediaSegmentUrls.length; i += BATCH) {
                    const batch = mediaSegmentUrls.slice(i, i + BATCH);
                    const resps = await Promise.all(batch.map(u => httpsGet(u, plainHeaders)));
                    for (let j = 0; j < resps.length; j++) {
                        if (!resps[j].ok) throw new Error(`Segment ${i+j} fetch failed: ${resps[j].status}`);
                        segBuffers.push(resps[j].buffer());
                    }
                }
                const fragBuf = decryptMp4(Buffer.concat([initBuf, ...segBuffers]), decryptKeyHex, isLegacy);
                const { app } = require('electron');
                const tempDir = path.join(app.getPath('temp'), 'mediaharbor');
                if (!fs.existsSync(tempDir)) fs.mkdirSync(tempDir, { recursive: true });
                const decFile = path.join(tempDir, `apple_dec_${songId}.mp4`);
                fs.writeFileSync(decFile, fragBuf);
                const ffmpeg = spawn('ffmpeg', [
                    '-y', '-loglevel', 'error',
                    '-i', decFile,
                    '-c', 'copy', '-f', 'mpegts', 'pipe:1',
                ]);
                ffmpeg.stdout.pipe(output);
                ffmpeg.stderr.on('data', (d) => { console.warn('[apple] ffmpeg video:', d.toString().trim()); });
                ffmpeg.on('error', (err) => { output.destroy(err); });
                ffmpeg.on('close', () => { try { fs.unlinkSync(decFile); } catch (_) {} output.end(); });
                return;
            }

            // Audio: live streaming — set up decrypt state from init segment, write header
            const state = createDecryptState(initBuf, decryptKeyHex, isLegacy);
            output.write(state.header); // ftyp + moov → player can start immediately

            // Fetch and decrypt segments in small batches, writing each immediately
            const BATCH = 4;
            for (let i = 0; i < mediaSegmentUrls.length; i += BATCH) {
                const batch = mediaSegmentUrls.slice(i, i + BATCH);
                const resps = await Promise.all(batch.map(u => httpsGet(u, plainHeaders)));
                for (let j = 0; j < resps.length; j++) {
                    if (!resps[j].ok) throw new Error(`Segment ${i+j} fetch failed: ${resps[j].status}`);
                    const decrypted = decryptSegmentBuf(state, resps[j].buffer());
                    if (decrypted.length > 0) output.write(decrypted);
                }
            }

            console.log(`[apple] stream complete: ${mediaSegmentUrls.length} segments`);
            output.end();
        } catch (err) {
            console.error('[apple] stream error:', err.message);
            output.destroy(err);
        }
    })();

    return { stream: output, contentType: isVideo ? 'video/mp2t' : 'audio/mp4', durationMs };
}

let _gamdlWvdPath = null;
async function getGamdlWvdPath() {
    if (_gamdlWvdPath && fs.existsSync(_gamdlWvdPath)) return _gamdlWvdPath;

    try {
        const { app } = require('electron');
        const tempDir = path.join(app.getPath('temp'), 'mediaharbor');
        if (!fs.existsSync(tempDir)) fs.mkdirSync(tempDir, { recursive: true });
        const wvdFile = path.join(tempDir, 'apple_music.wvd');

        if (fs.existsSync(wvdFile)) {
            _gamdlWvdPath = wvdFile;
            return wvdFile;
        }

        const { execSync } = require('child_process');
        const b64 = execSync(
            `"${getVenvPython()}" -c "from gamdl.downloader.hardcoded_wvd import HARDCODED_WVD; print(HARDCODED_WVD.strip())"`,
            { encoding: 'utf8', timeout: 10000 }
        ).trim();

        if (b64) {
            fs.writeFileSync(wvdFile, Buffer.from(b64, 'base64'));
            _gamdlWvdPath = wvdFile;
            return wvdFile;
        }
    } catch (e) {
    }
    return null;
}

module.exports = { parseAppleMusicUrl, isConfigured, getTrackStream };

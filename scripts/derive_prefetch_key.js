/**
 * Derives the Apple Music DEFAULT_SONG_DECRYPTION_KEY (prefetch key) using a given WVD.
 * The prefetch PSSH (KID = s1/e1 = 000000000000000073312f6531202020) appears in every
 * Apple Music HLS master manifest as a fixed #EXT-X-SESSION-KEY entry.
 * 
 * Usage: node scripts/derive_prefetch_key.js
 * 
 * On success, prints the key and patches src/funcs/apis/mp4decrypt.js automatically.
 */
'use strict';
const https = require('https');
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');

const COOKIES_PATH = 'D:\\Downloads\\cookies (1).txt';
const WVD_PATH = 'D:\\Documents\\CDRM DEVICES [DRMLAB]\\L3DRM\\samsung_sm-g525f_16.1.1@006_8816b781_28919_l3.wvd';
const TEST_SONG_ID = '1440857797';
const APPLE_MUSIC_HOMEPAGE = 'https://music.apple.com';
const AMP_API_URL = 'https://amp-api.music.apple.com';
const MP4DECRYPT_PATH = path.join(__dirname, '..', 'src', 'funcs', 'apis', 'mp4decrypt.js');

// Prefetch PSSH KID bytes: ASCII "s1/e1   " = 73 31 2f 65 31 20 20 20
// This KID is hardcoded by Apple for the P000000000/s1/e1 prefetch content.
// We look for the matching #EXT-X-SESSION-KEY in the live manifest to get the exact PSSH.
const PREFETCH_KID_HEX = '000000000000000073312f6531202020';
const WIDEVINE_SYSTEM_ID = Buffer.from('edef8ba979d64acea3c827dcd51d21ed', 'hex');

// ── HTTP helpers ──────────────────────────────────────────────────────────────

function httpsGet(urlStr, headers, redirectCount = 0) {
    return new Promise((resolve, reject) => {
        if (redirectCount > 10) return reject(new Error('Too many redirects'));
        const parsed = new URL(urlStr);
        const opts = {
            hostname: parsed.hostname, port: 443,
            path: parsed.pathname + parsed.search,
            method: 'GET', headers: headers || {},
        };
        const req = https.request(opts, (res) => {
            if ([301, 302, 307, 308].includes(res.statusCode) && res.headers.location) {
                const next = res.headers.location.startsWith('http')
                    ? res.headers.location
                    : `https://${parsed.hostname}${res.headers.location}`;
                return resolve(httpsGet(next, headers, redirectCount + 1));
            }
            const chunks = [];
            res.on('data', c => chunks.push(c));
            res.on('end', () => {
                const buf = Buffer.concat(chunks);
                resolve({
                    ok: res.statusCode >= 200 && res.statusCode < 300,
                    status: res.statusCode,
                    text: () => buf.toString('utf8'),
                    buffer: () => buf,
                });
            });
        });
        req.on('error', reject);
        req.end();
    });
}

// ── Cookie helpers ────────────────────────────────────────────────────────────

function getMediaUserToken(cookiesPath) {
    const lines = fs.readFileSync(cookiesPath, 'utf8').split('\n');
    for (const line of lines) {
        if (line.startsWith('#') || !line.trim()) continue;
        const p = line.split('\t');
        if (p.length >= 7 && p[5] === 'media-user-token' &&
            (p[0] === '.music.apple.com' || p[0] === 'music.apple.com')) return p[6].trim();
    }
    for (const line of lines) {
        if (line.startsWith('#') || !line.trim()) continue;
        const p = line.split('\t');
        if (p.length >= 7 && p[5] === 'media-user-token' && p[0].includes('apple.com')) return p[6].trim();
    }
    throw new Error('media-user-token not found');
}

function buildCookieHeader(cookiesPath) {
    const lines = fs.readFileSync(cookiesPath, 'utf8').split('\n');
    const pairs = [];
    for (const line of lines) {
        if (line.startsWith('#') || !line.trim()) continue;
        const p = line.split('\t');
        if (p.length >= 7 && (p[0] === '.music.apple.com' || p[0] === 'music.apple.com')) {
            pairs.push(`${p[5]}=${p[6].trim()}`);
        }
    }
    return pairs.join('; ');
}

// ── Dev token ─────────────────────────────────────────────────────────────────

async function getDeveloperToken() {
    const r = await httpsGet(APPLE_MUSIC_HOMEPAGE, {
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
    });
    const html = r.text();
    const jsUri = html.match(/\/(assets\/index-legacy[~-][^/"]+\.js)/)?.[1];
    if (!jsUri) throw new Error('Could not find index JS on music.apple.com');
    const jsR = await httpsGet(`${APPLE_MUSIC_HOMEPAGE}/${jsUri}`, {
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
    });
    const tok = jsR.text().match(/(?=eyJh)(.*?)(?=")/)?.[1];
    if (!tok) throw new Error('Could not extract dev token JWT');
    return tok;
}

// ── Get PSSH for prefetch KID from live manifest ──────────────────────────────

async function getPrefetchPsshFromManifest(mediaUserToken, cookieHeader, devToken) {
    // Fetch song metadata to get HLS URL
    const storefront = 'us';
    const songR = await httpsGet(
        `${AMP_API_URL}/v1/catalog/${storefront}/songs/${TEST_SONG_ID}?extend=extendedAssetUrls`,
        {
            'Authorization': `Bearer ${devToken}`,
            'Media-User-Token': mediaUserToken,
            'Cookie': cookieHeader,
            'Origin': APPLE_MUSIC_HOMEPAGE,
            'Referer': APPLE_MUSIC_HOMEPAGE,
            'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
        }
    );
    if (!songR.ok) throw new Error(`Song API ${songR.status}`);
    const hlsUrl = JSON.parse(songR.text())?.data?.[0]?.attributes?.extendedAssetUrls?.enhancedHls;
    if (!hlsUrl) throw new Error('No enhancedHls URL');

    const masterR = await httpsGet(hlsUrl, {
        'Authorization': `Bearer ${devToken}`,
        'Media-User-Token': mediaUserToken,
        'Cookie': cookieHeader,
        'Origin': APPLE_MUSIC_HOMEPAGE,
        'Referer': APPLE_MUSIC_HOMEPAGE,
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36',
        'accept': '*/*',
        'accept-language': 'en-US',
        'priority': 'u=1, i',
    });
    if (!masterR.ok) throw new Error(`HLS master ${masterR.status}`);

    const m3u8 = masterR.text();
    // Find all Widevine #EXT-X-SESSION-KEY entries.
    // Parse each tag line that contains the Widevine KEYFORMAT UUID.
    const psshs = [];
    for (const line of m3u8.split('\n')) {
        if (!line.startsWith('#EXT-X-SESSION-KEY:')) continue;
        if (!line.includes('urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed')) continue;
        const uriMatch = line.match(/URI="([^"]+)"/);
        if (uriMatch) psshs.push(uriMatch[1]);
    }

    if (psshs.length === 0) throw new Error('No Widevine SESSION-KEYs in manifest');
    console.log(`  Found ${psshs.length} Widevine PSHHs in manifest`);

    // Return ALL PSHHs so the caller can try each one
    // Also identify which one contains the prefetch KID
    const prefetchKid = Buffer.from(PREFETCH_KID_HEX, 'hex');
    let prefetchPssh = null;
    let trackPssh = null;

    for (const uri of psshs) {
        const b64 = uri.split(',').pop();
        let psshBox;
        try { psshBox = Buffer.from(b64, 'base64'); } catch { continue; }
        if (psshBox.length < 32) continue;
        const dataLen = psshBox.readUInt32BE(28);
        const data = psshBox.slice(32, 32 + dataLen);
        if (data.includes(prefetchKid)) {
            prefetchPssh = uri;
            console.log(`  [prefetch PSSH] KID s1/e1: ${uri.substring(0, 80)}...`);
        } else {
            if (!trackPssh) trackPssh = uri;
            console.log(`  [track PSSH]: ${uri.substring(0, 80)}...`);
        }
    }

    // Prefer the track PSSH (not prefetch) because Apple's license server
    // returns ALL content keys including the prefetch key in one license response
    return { prefetchPssh, trackPssh: trackPssh || prefetchPssh, allPsshs: psshs };
}

// ── Python CDM call ───────────────────────────────────────────────────────────

function callPywidevine(pssh, wvdPath, songId, cookiesPath) {
    const pyScript = `
import sys, json, base64, asyncio
from pywidevine import PSSH, Cdm, Device
from gamdl.api.apple_music_api import AppleMusicApi

args = json.loads(sys.stdin.readline())

async def get_keys():
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

        results = []
        for k in keys:
            kid_hex = k.kid.hex if isinstance(k.kid.hex, str) else k.kid.hex()
            key_hex = k.key.hex() if callable(k.key.hex) else k.key.hex
            results.append({"kid": kid_hex, "key": key_hex})

        return {"keys": results}
    finally:
        cdm.close(session_id)

try:
    result = asyncio.run(get_keys())
    sys.stdout.write(json.dumps(result) + "\\n")
except Exception as e:
    sys.stdout.write(json.dumps({"error": str(e)}) + "\\n")
    sys.exit(1)
`;

    return new Promise((resolve, reject) => {
        const venvPy = path.join(__dirname, '..', '.venv', 'Scripts', 'python.exe');
        const pyExe = fs.existsSync(venvPy) ? venvPy : 'python';
        const child = spawn(pyExe, ['-c', pyScript]);
        let stderr = '';
        child.stderr.on('data', d => { stderr += d; process.stderr.write('[py] ' + d); });
        child.on('error', e => reject(new Error(`spawn failed: ${e.message}`)));
        let out = '';
        child.stdout.on('data', d => { out += d; });
        child.on('close', code => {
            const last = out.trim().split('\n').pop();
            try {
                const r = JSON.parse(last);
                if (r.error) return reject(new Error(r.error));
                resolve(r);
            } catch (e) { reject(new Error(`parse error: ${last?.substring(0, 200)}\nstderr: ${stderr.substring(0, 300)}`)); }
        });
        child.stdin.write(JSON.stringify({ pssh, wvd_path: wvdPath, song_id: songId, cookies_path: cookiesPath }) + '\n');
        child.stdin.end();
    });
}

// ── Patch mp4decrypt.js ───────────────────────────────────────────────────────

function patchMp4Decrypt(newKeyHex) {
    const src = fs.readFileSync(MP4DECRYPT_PATH, 'utf8');
    const patched = src.replace(
        /const DEFAULT_SONG_DECRYPTION_KEY = Buffer\.from\('[0-9a-f]{32}', 'hex'\);/,
        `const DEFAULT_SONG_DECRYPTION_KEY = Buffer.from('${newKeyHex}', 'hex');`
    );
    if (patched === src) throw new Error('Could not find DEFAULT_SONG_DECRYPTION_KEY line to patch');
    fs.writeFileSync(MP4DECRYPT_PATH, patched, 'utf8');
}

// ── Main ──────────────────────────────────────────────────────────────────────

async function main() {
    console.log('\n=== Apple Music Prefetch Key Derivation ===\n');

    if (!fs.existsSync(COOKIES_PATH)) { console.error(`Cookies not found: ${COOKIES_PATH}`); process.exit(1); }
    if (!fs.existsSync(WVD_PATH)) { console.error(`WVD not found: ${WVD_PATH}`); process.exit(1); }

    console.log(`WVD: ${path.basename(WVD_PATH)}`);
    console.log(`Cookies: ${COOKIES_PATH}\n`);

    console.log('[1] Loading credentials...');
    const mediaUserToken = getMediaUserToken(COOKIES_PATH);
    const cookieHeader = buildCookieHeader(COOKIES_PATH);
    console.log(`  media-user-token: ${mediaUserToken.substring(0, 30)}...`);

    console.log('[2] Fetching developer token...');
    const devToken = await getDeveloperToken();
    console.log(`  devToken: ${devToken.substring(0, 50)}...`);

    console.log('[3] Fetching PSHHs from Apple Music HLS manifest...');
    const { prefetchPssh, trackPssh, allPsshs } = await getPrefetchPsshFromManifest(mediaUserToken, cookieHeader, devToken);

    // Try each PSSH until we get a valid license response containing the prefetch KID
    let prefetchKey = null;
    const psshsToTry = [trackPssh, prefetchPssh, ...allPsshs].filter((v, i, a) => v && a.indexOf(v) === i);

    for (const pssh of psshsToTry) {
        console.log(`\n[4] Calling Widevine CDM with Samsung WVD...`);
        console.log(`    PSSH: ${pssh.substring(0, 80)}...`);
        let result;
        try {
            result = await callPywidevine(pssh, WVD_PATH, TEST_SONG_ID, COOKIES_PATH);
        } catch (e) {
            console.log(`    ⚠ CDM failed: ${e.message}`);
            if (e.message.includes('disconnected') || e.message.includes('rejected') || e.message.includes('revoked')) {
                console.log('    Samsung WVD appears to be rejected by Apple\'s license server for this PSSH.');
            }
            continue;
        }

        console.log(`\n  All keys returned by CDM:`);
        for (const k of result.keys) {
            console.log(`    KID: ${k.kid}  KEY: ${k.key}`);
        }

        const entry = result.keys.find(k => k.kid === PREFETCH_KID_HEX);
        if (entry) {
            prefetchKey = entry.key;
            console.log(`\n  ✅ Prefetch key found in license response!`);
            break;
        } else {
            console.log(`  ⚠ Prefetch KID not in this license — trying next PSSH...`);
        }
    }

    if (!prefetchKey) {
        console.error('\n❌ Could not obtain prefetch key from Samsung WVD.');
        console.error('   Apple\'s license server may have rejected this device (common for leaked WVDs).');
        console.error('   The key 32b8ade1769e26b1ffb8986352793fc6 was derived from gamdl\'s source');
        console.error('   and is correct — it\'s a fixed Apple CDM constant, the same for all valid devices.');
        process.exit(1);
    }

    const currentMatch = fs.readFileSync(MP4DECRYPT_PATH, 'utf8')
        .match(/DEFAULT_SONG_DECRYPTION_KEY = Buffer\.from\('([0-9a-f]{32})', 'hex'\)/);
    const currentKey = currentMatch?.[1];

    if (currentKey === prefetchKey) {
        console.log('\n  ✅ mp4decrypt.js already has the correct key — no change needed.');
    } else {
        console.log(`\n  Updating mp4decrypt.js:`);
        console.log(`    Old: ${currentKey}`);
        console.log(`    New: ${prefetchKey}`);
        patchMp4Decrypt(prefetchKey);
        console.log('  ✅ mp4decrypt.js patched successfully.');
    }

    console.log('\n=== Done ===\n');
}

main().catch(e => { console.error('\n❌ Error:', e.message); process.exit(1); });

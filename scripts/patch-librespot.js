/**
 * Patches librespot npm package to fix Spotify login.
 *
 * Spotify's v4 login endpoint now returns error code 9 when using the
 * passwordV4 proto field (109). This patch switches to the v3 endpoint
 * with the password field (101), which still works.
 *
 * Run automatically via postinstall, or manually: node scripts/patch-librespot.js
 */

const fs = require('fs');
const path = require('path');

const login5Path = path.join(__dirname, '..', 'node_modules', 'librespot', 'build', 'login5.js');
const loginReqPath = path.join(__dirname, '..', 'node_modules', 'librespot', 'build', 'messages', 'LoginRequest.js');

function patchFile(filePath, replacements) {
    if (!fs.existsSync(filePath)) {
        console.log(`[patch-librespot] Skipping ${path.basename(filePath)} (not found)`);
        return;
    }
    let content = fs.readFileSync(filePath, 'utf8');
    let patched = false;
    for (const [search, replace] of replacements) {
        if (content.includes(search)) {
            content = content.replace(search, replace);
            patched = true;
        }
    }
    if (patched) {
        fs.writeFileSync(filePath, content, 'utf8');
        console.log(`[patch-librespot] Patched ${path.basename(filePath)}`);
    } else {
        console.log(`[patch-librespot] ${path.basename(filePath)} already patched or unchanged`);
    }
}

// 1. login5.js: Remove v4 interaction block for password login
patchFile(login5Path, [
    [
        `        if (credentials.password) {\n            params.interaction = {\n                uri: 'https://auth-callback.spotify.com/r/android/music/login',\n                nonce: this.uuidv4(),\n                ui_locales: 'en'\n            };\n        }`,
        `        // Patched: v4 interaction removed — Spotify v4 returns error 9.\n        // Using v3 with password field (101) instead of passwordV4 (109).`
    ]
]);

// 2. LoginRequest.js: Use password (field 101) instead of passwordV4 (field 109)
patchFile(loginReqPath, [
    ['this.payload.passwordV4 =', 'this.payload.password =']
]);

console.log('[patch-librespot] Done');

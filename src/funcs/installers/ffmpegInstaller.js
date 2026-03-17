const os = require('os');
const path = require('path');
const fs = require('fs');
const axios = require('axios');
const decompress = require('decompress');
const decompressUnzip = require('decompress-unzip');
const { spawn, execFileSync } = require('child_process');
const xml2js = require('xml2js');

const HTTP_TIMEOUT = 60_000;
const DOWNLOAD_TIMEOUT = 600_000;
const MAX_RETRIES = 3;
const RETRY_DELAY_MS = 2_000;

const DOWNLOAD_URLS = {
    win32: {
        x64: 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip',
    },
    linux: {
        x64: 'https://johnvansickle.com/ffmpeg/builds/ffmpeg-git-amd64-static.tar.xz',
        arm64: 'https://johnvansickle.com/ffmpeg/builds/ffmpeg-git-arm64-static.tar.xz',
        arm: 'https://johnvansickle.com/ffmpeg/builds/ffmpeg-git-armhf-static.tar.xz',
    },
};

function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

async function withRetry(fn, retries = MAX_RETRIES, delay = RETRY_DELAY_MS) {
    let lastError;
    for (let attempt = 1; attempt <= retries; attempt++) {
        try {
            return await fn();
        } catch (error) {
            lastError = error;
            if (attempt < retries) {
                await sleep(delay);
            }
        }
    }
    throw lastError;
}

function formatBytes(bytes, decimals = 2) {
    if (!bytes || bytes === 0) return '0 Bytes';
    const k = 1024;
    const dm = Math.max(0, decimals);
    const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dm))} ${sizes[i]}`;
}

async function getFileSize(url) {
    try {
        const headResponse = await axios.head(url, { timeout: HTTP_TIMEOUT });
        const contentLength = headResponse.headers['content-length'];
        return contentLength ? parseInt(contentLength, 10) : null;
    } catch {
        return null;
    }
}

function sendProgress(mainWin, percent, status) {
    mainWin.webContents.send('installation-progress', { percent, status });
}

function sendError(mainWin, message) {
    mainWin.webContents.send('installation-error', message);
}

async function extractTarXz(filePath, destination) {
    return new Promise((resolve, reject) => {
        const tarProcess = spawn('tar', ['-xJf', filePath, '-C', destination]);
        let stderr = '';

        tarProcess.stderr.on('data', (data) => {
            stderr += data.toString();
        });

        tarProcess.on('error', (err) => {
            reject(new Error(`Failed to spawn tar: ${err.message}`, { cause: err }));
        });

        tarProcess.on('close', (code) => {
            if (code === 0) {
                resolve();
            } else {
                reject(new Error(`Tar extraction failed (exit code ${code}): ${stderr}`));
            }
        });
    });
}

async function downloadFile(url, destPath, onProgress) {
    const totalLength = await getFileSize(url);
    let downloadedLength = 0;

    const response = await axios({
        method: 'GET',
        url,
        responseType: 'stream',
        timeout: DOWNLOAD_TIMEOUT,
    });

    const writer = fs.createWriteStream(destPath);

    return new Promise((resolve, reject) => {
        const cleanup = (err) => {
            response.data.destroy();
            writer.destroy();
            reject(err);
        };

        response.data.on('error', cleanup);
        writer.on('error', cleanup);

        response.data.on('data', (chunk) => {
            downloadedLength += chunk.length;
            if (totalLength) {
                onProgress(downloadedLength / totalLength);
            }
        });

        writer.on('finish', resolve);

        response.data.pipe(writer);
    });
}

async function fetchLatestFromRSS(feedUrl, label) {
    return withRetry(async () => {
        try {
            const response = await axios.get(feedUrl, { timeout: HTTP_TIMEOUT });
            const parser = new xml2js.Parser();
            const result = await parser.parseStringPromise(response.data);

            const channel = result?.rss?.channel?.[0];
            const latestItem = channel?.item?.[0];

            if (!latestItem?.title?.[0] || !latestItem?.link?.[0]) {
                throw new Error(`Malformed RSS response for ${label}`);
            }

            return {
                version: latestItem.title[0].replace('.zip', ''),
                url: latestItem.link[0],
            };
        } catch (error) {
            throw new Error(`Failed to fetch latest ${label} version`, { cause: error });
        }
    });
}

function getLatestFFmpegVersion() {
    return fetchLatestFromRSS('https://evermeet.cx/ffmpeg/rss.xml', 'FFmpeg');
}

function getLatestFFprobeVersion() {
    return fetchLatestFromRSS('https://evermeet.cx/ffmpeg/ffprobe-rss.xml', 'FFprobe');
}

function escapePowerShellSingleQuote(str) {
    return str.replace(/'/g, "''");
}

async function updateSystemPath(ffmpegDir, mainWin) {
    const platform = os.platform();
    process.env.PATH = `${ffmpegDir}${path.delimiter}${process.env.PATH}`;

    try {
        if (platform === 'win32') {
            const currentPath = execFileSync('powershell.exe', [
                '-NoProfile',
                '-Command',
                "[Environment]::GetEnvironmentVariable('Path', 'User')",
            ]).toString().trim();

            if (!currentPath.includes(ffmpegDir)) {
                const newPath = `${ffmpegDir}${path.delimiter}${currentPath}`;
                const safeNewPath = escapePowerShellSingleQuote(newPath);

                execFileSync('powershell.exe', [
                    '-NoProfile',
                    '-Command',
                    `[Environment]::SetEnvironmentVariable('Path', '${safeNewPath}', 'User')`,
                ]);
            }
        } else {
            const homeDir = os.homedir();
            const shell = process.env.SHELL || '';
            const safePath = ffmpegDir.replace(/"/g, '\\"');
            const exportLine = `\nexport PATH="${safePath}:$PATH"\n`;

            let configFile;
            if (shell.includes('zsh')) {
                configFile = path.join(homeDir, '.zshrc');
            } else if (shell.includes('bash')) {
                configFile = path.join(homeDir, '.bashrc');
                if (platform === 'darwin' && !fs.existsSync(configFile)) {
                    configFile = path.join(homeDir, '.bash_profile');
                }
            } else if (shell.includes('fish')) {
                const fishConfigDir = path.join(homeDir, '.config', 'fish');
                fs.mkdirSync(fishConfigDir, { recursive: true });
                configFile = path.join(fishConfigDir, 'config.fish');
                const fishLine = `\nset -gx PATH "${safePath}" $PATH\n`;
                const existing = fs.existsSync(configFile)
                    ? fs.readFileSync(configFile, 'utf8')
                    : '';
                if (!existing.includes(ffmpegDir)) {
                    fs.appendFileSync(configFile, fishLine);
                }
                return true;
            } else {
                configFile = path.join(homeDir, '.profile');
            }

            const content = fs.existsSync(configFile)
                ? fs.readFileSync(configFile, 'utf8')
                : '';

            if (!content.includes(ffmpegDir)) {
                fs.appendFileSync(configFile, exportLine);
            }
        }

        return true;
    } catch (error) {
        sendProgress(
            mainWin,
            85,
            `PATH update issue: ${error.message}. FFmpeg will work after restart.`
        );
        return false;
    }
}

function testFFmpegAccess(ffmpegDir) {
    try {
        const env = { ...process.env, PATH: `${ffmpegDir}${path.delimiter}${process.env.PATH}` };
        execFileSync('ffmpeg', ['-version'], { env, stdio: 'pipe' });
        return true;
    } catch {
        return false;
    }
}

function getDownloadUrl(platform, arch) {
    const platformUrls = DOWNLOAD_URLS[platform];
    if (!platformUrls) return null;
    return platformUrls[arch] || platformUrls.x64 || null;
}

function resolveArch() {
    const arch = os.arch();
    if (arch === 'x64' || arch === 'x86_64') return 'x64';
    if (arch === 'arm64' || arch === 'aarch64') return 'arm64';
    if (arch === 'arm') return 'arm';
    return arch;
}

async function installMacOS(ffmpegDir, mainWin) {
    const ffmpegLatest = await getLatestFFmpegVersion();
    const ffmpegTempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ffmpeg-'));

    try {
        const ffmpegFileName = path.basename(ffmpegLatest.url);
        const ffmpegTempFile = path.join(ffmpegTempDir, ffmpegFileName);

        await downloadFile(ffmpegLatest.url, ffmpegTempFile, (fraction) => {
            sendProgress(mainWin, Math.round(fraction * 25), `Downloading FFmpeg… ${Math.round(fraction * 100)}%`);
        });

        sendProgress(mainWin, 25, 'Extracting FFmpeg…');

        await decompress(ffmpegTempFile, ffmpegDir, { plugins: [decompressUnzip()] });

        const ffmpegBin = path.join(ffmpegDir, 'ffmpeg');
        if (!fs.existsSync(ffmpegBin)) {
            throw new Error('FFmpeg binary not found after extraction');
        }
        fs.chmodSync(ffmpegBin, 0o755);
    } finally {
        fs.rmSync(ffmpegTempDir, { recursive: true, force: true });
    }

    const ffprobeLatest = await getLatestFFprobeVersion();
    const ffprobeTempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ffprobe-'));

    try {
        const ffprobeFileName = path.basename(ffprobeLatest.url);
        const ffprobeTempFile = path.join(ffprobeTempDir, ffprobeFileName);

        await downloadFile(ffprobeLatest.url, ffprobeTempFile, (fraction) => {
            const percent = 25 + Math.round(fraction * 25);
            sendProgress(mainWin, percent, `Downloading FFprobe… ${Math.round(fraction * 100)}%`);
        });

        sendProgress(mainWin, 50, 'Extracting FFprobe…');

        await decompress(ffprobeTempFile, ffmpegDir, { plugins: [decompressUnzip()] });

        const ffprobeBin = path.join(ffmpegDir, 'ffprobe');
        if (!fs.existsSync(ffprobeBin)) {
            throw new Error('FFprobe binary not found after extraction');
        }
        fs.chmodSync(ffprobeBin, 0o755);
    } finally {
        fs.rmSync(ffprobeTempDir, { recursive: true, force: true });
    }
}

async function installWindows(downloadUrl, ffmpegDir, mainWin) {
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ffmpeg-'));

    try {
        const fileName = path.basename(downloadUrl);
        const tempFilePath = path.join(tempDir, fileName);

        await downloadFile(downloadUrl, tempFilePath, (fraction) => {
            sendProgress(mainWin, Math.round(fraction * 50), `Downloading FFmpeg… ${Math.round(fraction * 100)}%`);
        });

        sendProgress(mainWin, 50, 'Extracting FFmpeg…');

        await decompress(tempFilePath, tempDir, { plugins: [decompressUnzip()] });

        const extractedFiles = fs.readdirSync(tempDir);
        let binDir = null;

        for (const file of extractedFiles) {
            const fullPath = path.join(tempDir, file);
            if (fs.statSync(fullPath).isDirectory()) {
                const children = fs.readdirSync(fullPath);
                if (children.includes('bin')) {
                    binDir = path.join(fullPath, 'bin');
                    break;
                }
            }
        }

        if (!binDir) {
            throw new Error('Could not find bin directory in extracted FFmpeg archive');
        }

        for (const exe of ['ffmpeg.exe', 'ffprobe.exe']) {
            const src = path.join(binDir, exe);
            const dest = path.join(ffmpegDir, exe);
            if (!fs.existsSync(src)) {
                throw new Error(`Expected binary ${exe} not found in archive`);
            }
            fs.copyFileSync(src, dest);
        }
    } finally {
        fs.rmSync(tempDir, { recursive: true, force: true });
    }
}

async function installLinux(downloadUrl, ffmpegDir, mainWin) {
    const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ffmpeg-'));

    try {
        const fileName = path.basename(downloadUrl);
        const tempFilePath = path.join(tempDir, fileName);

        await downloadFile(downloadUrl, tempFilePath, (fraction) => {
            sendProgress(mainWin, Math.round(fraction * 50), `Downloading FFmpeg… ${Math.round(fraction * 100)}%`);
        });

        sendProgress(mainWin, 50, 'Extracting FFmpeg…');

        await extractTarXz(tempFilePath, tempDir);

        const extractedFiles = fs.readdirSync(tempDir);
        const ffmpegFolder = extractedFiles.find((f) => f.includes('ffmpeg') && f !== fileName);
        if (!ffmpegFolder) {
            throw new Error('FFmpeg folder not found after extraction');
        }

        const extractedDir = path.join(tempDir, ffmpegFolder);
        for (const binary of ['ffmpeg', 'ffprobe']) {
            const src = path.join(extractedDir, binary);
            const dest = path.join(ffmpegDir, binary);
            if (!fs.existsSync(src)) {
                throw new Error(`Expected binary "${binary}" not found in archive`);
            }
            fs.copyFileSync(src, dest);
            fs.chmodSync(dest, 0o755);
        }
    } finally {
        fs.rmSync(tempDir, { recursive: true, force: true });
    }
}

async function downloadAndInstallFFmpeg(mainWin) {
    try {
        const platform = os.platform();
        const arch = resolveArch();

        sendProgress(mainWin, 0, 'Starting FFmpeg download…');

        const homeDir = os.homedir();
        let ffmpegDir;

        if (platform === 'win32') {
            ffmpegDir = path.join(homeDir, 'ffmpeg', 'bin');
        } else {
            ffmpegDir = path.join(homeDir, '.local', 'bin');
        }

        fs.mkdirSync(ffmpegDir, { recursive: true });

        if (platform === 'darwin') {
            await installMacOS(ffmpegDir, mainWin);
        } else if (platform === 'win32' || platform === 'linux') {
            const downloadUrl = getDownloadUrl(platform, arch);
            if (!downloadUrl) {
                throw new Error(
                    `Unsupported platform/architecture combination: ${platform}/${arch}`
                );
            }

            if (platform === 'win32') {
                await installWindows(downloadUrl, ffmpegDir, mainWin);
            } else {
                await installLinux(downloadUrl, ffmpegDir, mainWin);
            }
        } else {
            throw new Error(`Unsupported platform: ${platform}`);
        }

        sendProgress(mainWin, 80, 'Updating system PATH…');
        await updateSystemPath(ffmpegDir, mainWin);

        sendProgress(mainWin, 90, 'Verifying installation…');
        const accessible = testFFmpegAccess(ffmpegDir);

        if (!accessible) {
            sendProgress(
                mainWin,
                95,
                'FFmpeg installed but not yet in current PATH. It will be available after terminal restart.'
            );
        }

        sendProgress(
            mainWin,
            100,
            'FFmpeg installed successfully! Please restart your terminal if FFmpeg commands are not working.'
        );
    } catch (error) {
        sendError(mainWin, error.message);
    }
}

module.exports = { downloadAndInstallFFmpeg };
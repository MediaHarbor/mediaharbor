const os = require('os');
const path = require('path');
const fs = require('fs');
const axios = require('axios');
const decompress = require('decompress');
const decompressUnzip = require('decompress-unzip');
const { spawn, execSync } = require('child_process');
const xml2js = require('xml2js');

// Add the extractTarXz function from one.js
async function extractTarXz(filePath, destination) {
    return new Promise((resolve, reject) => {
        console.log(`Extracting ${filePath} to ${destination}`);
        const tarProcess = spawn('tar', ['-xJf', filePath, '-C', destination]);

        let stderr = '';

        tarProcess.stderr.on('data', (data) => {
            stderr += data.toString();
            console.log('Tar stderr:', stderr);
        });

        tarProcess.on('close', (code) => {
            console.log(`Tar process exited with code ${code}`);
            if (code === 0) {
                resolve();
            } else {
                reject(new Error(`Tar extraction failed: ${stderr}`));
            }
        });
    });
}

async function getLatestFFmpegVersion() {
    try {
        const response = await axios.get('https://evermeet.cx/ffmpeg/rss.xml');
        const parser = new xml2js.Parser();
        const result = await parser.parseStringPromise(response.data);
        const latestItem = result.rss.channel[0].item[0];
        return {
            version: latestItem.title[0].replace('.zip', ''),
            url: latestItem.link[0]
        };
    } catch (error) {
        throw new Error('Failed to fetch latest FFmpeg version');
    }
}

async function getLatestFFprobeVersion() {
    try {
        const response = await axios.get('https://evermeet.cx/ffmpeg/ffprobe-rss.xml');
        const parser = new xml2js.Parser();
        const result = await parser.parseStringPromise(response.data);
        const latestItem = result.rss.channel[0].item[0];
        return {
            version: latestItem.title[0].replace('.zip', ''),
            url: latestItem.link[0]
        };
    } catch (error) {
        throw new Error('Failed to fetch latest FFprobe version');
    }
}

async function getFileSize(url) {
    try {
        const headResponse = await axios.head(url);
        const contentLength = headResponse.headers['content-length'];
        return contentLength ? parseInt(contentLength, 10) : null;
    } catch (error) {
        console.warn('HEAD request failed, cannot determine file size:', error.message);
        return null;
    }
}

// Update system PATH permanently (platform-specific)
async function updateSystemPath(ffmpegPath, mainWin) {
    const platform = os.platform();

    // Update current process.env.PATH (session only)
    process.env.PATH = `${ffmpegPath}${path.delimiter}${process.env.PATH}`;
    console.log(`Updated current process PATH: ${process.env.PATH}`);

    try {
        if (platform === 'win32') {
            // Windows: update using PowerShell
            const currentPath = execSync('powershell -command "[Environment]::GetEnvironmentVariable(\'Path\', \'User\')"').toString().trim();
            if (!currentPath.includes(ffmpegPath)) {
                const newPath = `${ffmpegPath}${path.delimiter}${currentPath}`;
                execSync(`powershell -command "[Environment]::SetEnvironmentVariable('Path', '${newPath}', 'User')"`); 
                console.log('Updated Windows User PATH environment variable');
            }
        } else {
            // For macOS and Linux update the shell profile(s)
            const homeDir = os.homedir();
            // For macOS using zsh, update both .zshrc and .zprofile
            const shellConfigs = ['.zshrc', '.zprofile', '.bashrc', '.bash_profile', '.profile'];
            const exportLine = `\n# FFmpeg PATH\nexport PATH="${ffmpegPath}:$PATH"\n`;

            shellConfigs.forEach(configFile => {
                const configPath = path.join(homeDir, configFile);
                if (fs.existsSync(configPath)) {
                    const content = fs.readFileSync(configPath, 'utf8');
                    if (!content.includes(ffmpegPath)) {
                        fs.appendFileSync(configPath, exportLine);
                        console.log(`Updated ${configFile} with FFmpeg PATH`);
                    }
                } else {
                    // If the file does not exist, create it with our export line.
                    fs.writeFileSync(configPath, exportLine);
                    console.log(`Created ${configFile} with FFmpeg PATH`);
                }
            });
        }

        return true;
    } catch (error) {
        console.error('Failed to update system PATH:', error);
        mainWin.webContents.send('installation-progress', {
            percent: 85,
            status: `PATH update issue: ${error.message}. FFmpeg will work after restart.`
        });
        return false;
    }
}

// Test if FFmpeg is accessible with the current PATH
function testFFmpegAccess(ffmpegDir) {
    try {
        // Clone current process.env and add ffmpegDir to PATH
        const env = { ...process.env };
        env.PATH = `${ffmpegDir}${path.delimiter}${env.PATH}`;

        // Try to run ffmpeg -version with the updated PATH
        execSync('ffmpeg -version', { env, stdio: 'pipe' });
        return true;
    } catch (error) {
        console.log('FFmpeg not accessible via PATH:', error.message);
        return false;
    }
}

async function downloadAndInstallFFmpeg(mainWin) {
    try {
        const platform = os.platform();
        let downloadUrl;

        if (platform === 'win32') {
            downloadUrl = 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip';
        } else if (platform === 'linux') {
            downloadUrl = 'https://johnvansickle.com/ffmpeg/builds/ffmpeg-git-amd64-static.tar.xz';
        }

        // Send initial progress update
        mainWin.webContents.send('installation-progress', {
            percent: 0,
            status: 'Starting FFmpeg download...'
        });

        // Define installation directory (for all platforms)
        const homeDir = os.homedir();
        let ffmpegDir;

        if (platform === 'win32') {
            ffmpegDir = path.join(homeDir, 'ffmpeg', 'bin');
        } else {
            // macOS/Linux: Use ~/.local/bin which is often in PATH
            ffmpegDir = path.join(homeDir, '.local', 'bin');
        }

        console.log('Creating directory:', ffmpegDir);
        fs.mkdirSync(ffmpegDir, { recursive: true });

        if (platform === 'darwin') {
            // Download and extract FFmpeg binary
            const ffmpegLatest = await getLatestFFmpegVersion();
            const ffmpegDownloadUrl = ffmpegLatest.url;
            console.log('FFmpeg download URL:', ffmpegDownloadUrl);

            // Download FFmpeg
            const ffmpegTempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ffmpeg-'));
            const ffmpegFileName = path.basename(ffmpegDownloadUrl);
            const ffmpegTempFilePath = path.join(ffmpegTempDir, ffmpegFileName);

            const totalLength = await getFileSize(ffmpegDownloadUrl);
            let downloadedLength = 0;

            const ffmpegWriter = fs.createWriteStream(ffmpegTempFilePath);
            const ffmpegResponse = await axios({
                method: 'GET',
                url: ffmpegDownloadUrl,
                responseType: 'stream'
            });

            // Listen for data events to track progress
            ffmpegResponse.data.on('data', (chunk) => {
                downloadedLength += chunk.length;
                // Map the fraction of download completion into the 0-30% range
                const downloadPercent = Math.round((downloadedLength / totalLength) * 30);
                mainWin.webContents.send('installation-progress', {
                    percent: downloadPercent,
                    status: `Downloading FFmpeg... ${downloadPercent}%`
                });
            });

            ffmpegResponse.data.pipe(ffmpegWriter);
            await new Promise((resolve, reject) => {
                ffmpegWriter.on('finish', resolve);
                ffmpegWriter.on('error', reject);
            });

            mainWin.webContents.send('installation-progress', {
                percent: 30,
                status: 'Extracting FFmpeg...'
            });

            await decompress(ffmpegTempFilePath, ffmpegDir, {
                plugins: [decompressUnzip()]
            });
            const ffmpegPath = path.join(ffmpegDir, 'ffmpeg');
            if (!fs.existsSync(ffmpegPath)) {
                throw new Error('FFmpeg binary not found after extraction');
            }
            fs.chmodSync(ffmpegPath, 0o755);
            fs.rmSync(ffmpegTempDir, { recursive: true, force: true });

            // Download and extract FFprobe binary
            const ffprobeLatest = await getLatestFFprobeVersion();
            const ffprobeDownloadUrl = ffprobeLatest.url;
            console.log('FFprobe download URL:', ffprobeDownloadUrl);

            const ffprobeTempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ffprobe-'));
            const ffprobeFileName = path.basename(ffprobeDownloadUrl);
            const ffprobeTempFilePath = path.join(ffprobeTempDir, ffprobeFileName);

            const ffprobeTotalLength = await getFileSize(ffprobeDownloadUrl);
            let ffprobeDownloadedLength = 0;

            const ffprobeWriter = fs.createWriteStream(ffprobeTempFilePath);
            const ffprobeResponse = await axios({
                method: 'GET',
                url: ffprobeDownloadUrl,
                responseType: 'stream'
            });

            ffprobeResponse.data.on('data', (chunk) => {
                ffprobeDownloadedLength += chunk.length;
                // Map the fraction of download completion into the 30-50% range.
                let base = 30; // start progress for FFprobe download
                const downloadPercent = base + Math.round((ffprobeDownloadedLength / ffprobeTotalLength) * 20);
                mainWin.webContents.send('installation-progress', {
                    percent: downloadPercent,
                    status: `Downloading FFprobe... ${downloadPercent}%`
                });
            });

            ffprobeResponse.data.pipe(ffprobeWriter);
            await new Promise((resolve, reject) => {
                ffprobeWriter.on('finish', resolve);
                ffprobeWriter.on('error', reject);
            });

            mainWin.webContents.send('installation-progress', {
                percent: 50,
                status: 'Extracting FFprobe...'
            });

            await decompress(ffprobeTempFilePath, ffmpegDir, {
                plugins: [decompressUnzip()]
            });
            const ffprobePath = path.join(ffmpegDir, 'ffprobe');
            if (!fs.existsSync(ffprobePath)) {
                throw new Error('FFprobe binary not found after extraction');
            }
            fs.chmodSync(ffprobePath, 0o755);
            fs.rmSync(ffprobeTempDir, { recursive: true, force: true });

        } else if (platform === 'win32' || platform === 'linux') {
            // Common download code for Windows and Linux
            const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ffmpeg-'));
            const fileName = path.basename(downloadUrl);
            const tempFilePath = path.join(tempDir, fileName);

            // Download file
            const totalLength = await getFileSize(downloadUrl);
            let downloadedLength = 0;

            const writer = fs.createWriteStream(tempFilePath);
            const response = await axios({
                method: 'GET',
                url: downloadUrl,
                responseType: 'stream'
            });

            // Listen for data events to track progress
            response.data.on('data', (chunk) => {
                downloadedLength += chunk.length;
                const downloadPercent = Math.round((downloadedLength / totalLength) * 30);
                mainWin.webContents.send('installation-progress', {
                    percent: downloadPercent,
                    status: `Downloading FFmpeg... ${downloadPercent}%`
                });
            });

            response.data.pipe(writer);
            await new Promise((resolve, reject) => {
                writer.on('finish', resolve);
                writer.on('error', reject);
            });

            mainWin.webContents.send('installation-progress', {
                percent: 30,
                status: 'Extracting FFmpeg...'
            });

            if (platform === 'win32') {
                await decompress(tempFilePath, tempDir, {
                    plugins: [decompressUnzip()]
                });

                // Windows FFmpeg zip has a nested directory structure
                const extractedFiles = fs.readdirSync(tempDir);
                console.log('Extracted files:', extractedFiles);

                // Find the bin directory that contains ffmpeg.exe
                let binDir = null;
                for (const file of extractedFiles) {
                    const fullPath = path.join(tempDir, file);
                    if (fs.statSync(fullPath).isDirectory()) {
                        // Look for nested bin directory
                        const nestedDirs = fs.readdirSync(fullPath);
                        const hasBin = nestedDirs.includes('bin');
                        if (hasBin) {
                            binDir = path.join(fullPath, 'bin');
                            break;
                        }
                    }
                }

                if (!binDir) {
                    throw new Error('Could not find bin directory in extracted FFmpeg');
                }

                // Copy the executables
                const exes = ['ffmpeg.exe', 'ffprobe.exe'];
                for (const exe of exes) {
                    const sourcePath = path.join(binDir, exe);
                    const targetPath = path.join(ffmpegDir, exe);
                    if (fs.existsSync(sourcePath)) {
                        fs.copyFileSync(sourcePath, targetPath);
                        console.log(`Copied ${exe} to ${targetPath}`);
                    } else {
                        console.warn(`${exe} not found in extracted files`);
                    }
                }
            } else if (platform === 'linux') {
                console.log('Extracting on Linux...');
                await extractTarXz(tempFilePath, tempDir);

                // Find the extracted directory
                const extractedFiles = fs.readdirSync(tempDir);
                console.log('Extracted files:', extractedFiles);

                // Find the ffmpeg directory (it usually contains 'ffmpeg' in the name)
                const ffmpegFolder = extractedFiles.find(f => f.includes('ffmpeg'));
                if (!ffmpegFolder) {
                    throw new Error('FFmpeg folder not found after extraction');
                }

                const extractedDir = path.join(tempDir, ffmpegFolder);
                console.log('Extracted directory:', extractedDir);

                // Copy FFmpeg executables to bin directory
                const binaries = ['ffmpeg', 'ffprobe'];
                for (const binary of binaries) {
                    const sourcePath = path.join(extractedDir, binary);
                    const targetPath = path.join(ffmpegDir, binary);

                    console.log(`Copying ${sourcePath} to ${targetPath}`);
                    fs.copyFileSync(sourcePath, targetPath);
                    fs.chmodSync(targetPath, 0o755); // Ensure executable permissions
                    console.log(`Set permissions for ${targetPath}`);
                }
            }

            // Clean up temp directory
            fs.rmSync(tempDir, { recursive: true, force: true });
        }

        // Update PATH to include FFmpeg directory
        mainWin.webContents.send('installation-progress', {
            percent: 80,
            status: 'Updating system PATH...'
        });

        await updateSystemPath(ffmpegDir, mainWin);

        // Verify the installation using the updated PATH
        console.log('Verifying installation...');
        const ffmpegAccessible = testFFmpegAccess(ffmpegDir);

        if (!ffmpegAccessible) {
            console.log('FFmpeg not immediately accessible, but was installed to:', ffmpegDir);
            mainWin.webContents.send('installation-progress', {
                percent: 95,
                status: 'FFmpeg installed but not in current PATH. It will be available after terminal restart.'
            });
        } else {
            console.log('FFmpeg successfully installed and accessible via PATH!');
        }

        mainWin.webContents.send('installation-progress', {
            percent: 100,
            status: 'FFmpeg installed successfully! Please restart your terminal if FFmpeg commands are not working.'
        });

    } catch (error) {
        mainWin.webContents.send('installation-error', error.message);
        console.error('Installation failed:', error);
    }
}

function formatBytes(bytes, decimals = 2) {
    if (bytes === 0) return '0 Bytes';
    const k = 1024,
        dm = decimals < 0 ? 0 : decimals,
        sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB'],
        i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + sizes[i];
}

module.exports = { downloadAndInstallFFmpeg };
// Updated old one.js
const os = require('os');
const path = require('path');
const fs = require('fs');
const axios = require('axios');
const decompress = require('decompress');
const decompressUnzip = require('decompress-unzip');
const { spawn } = require('child_process');
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

// Replace the existing getLatestFFmpegVersion if necessary (optional)
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

async function downloadAndInstallFFmpeg(mainWin) {
    try {
        const platform = os.platform();
        let downloadUrl;

        // Determine download URL based on platform
        if (platform === 'darwin') {
            const latest = await getLatestFFmpegVersion();
            downloadUrl = latest.url;
        } else if (platform === 'win32') {
            downloadUrl = 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip';
        } else if (platform === 'linux') {
            downloadUrl = 'https://johnvansickle.com/ffmpeg/builds/ffmpeg-git-amd64-static.tar.xz';
        } else {
            throw new Error(`Unsupported platform: ${platform}`);
        }

        // Send initial progress update
        mainWin.webContents.send('installation-progress', {
            percent: 0,
            status: 'Starting FFmpeg download...'
        });

        // Get file size using HEAD request
        const fileSize = await getFileSize(downloadUrl);
        if (fileSize) {
            console.log(`File size: ${fileSize} bytes`);
        } else {
            console.log('File size not available.');
        }

        // Download FFmpeg
        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'ffmpeg-'));
        const fileName = path.basename(downloadUrl);
        const tempFilePath = path.join(tempDir, fileName);

        const writer = fs.createWriteStream(tempFilePath);
        const response = await axios({
            method: 'GET',
            url: downloadUrl,
            responseType: 'stream',
        });

        const totalLength = fileSize || response.headers['content-length'];

        let downloaded = 0;

        response.data.on('data', (chunk) => {
            downloaded += chunk.length;
            if (totalLength) {
                const progress = Math.floor((downloaded / totalLength) * 100);
                mainWin.webContents.send('installation-progress', {
                    percent: Math.floor(progress * 0.4), // 40% of total progress
                    status: `Downloading FFmpeg: ${progress}%`
                });
            } else {
                // If total length is not available, show downloaded bytes
                mainWin.webContents.send('installation-progress', {
                    percent: Math.floor((downloaded / (100 * 1024 * 1024)) * 100), // Optional: Adjust max expected size
                    status: `Downloading FFmpeg: ${formatBytes(downloaded)} downloaded`
                });
            }
        });

        response.data.pipe(writer);
        await new Promise((resolve, reject) => {
            writer.on('finish', resolve);
            writer.on('error', reject);
        });

        // Extract FFmpeg
        mainWin.webContents.send('installation-progress', {
            percent: 40,
            status: 'Extracting FFmpeg...'
        });

        const homeDir = os.homedir();
        const ffmpegDir = path.join(homeDir, '.local', 'bin'); // Updated installation directory

        console.log('Creating directory:', ffmpegDir);
        fs.mkdirSync(ffmpegDir, { recursive: true });

        if (platform === 'win32' || platform === 'darwin') {
            await decompress(tempFilePath, ffmpegDir, {
                plugins: [decompressUnzip()]
            });
        } else if (platform === 'linux') {
            // Use the improved extraction method
            console.log('Extracting on Linux...');
            await extractTarXz(tempFilePath, tempDir);

            // Find the extracted directory
            const extractedFiles = fs.readdirSync(tempDir);
            console.log('Extracted files:', extractedFiles);

            // Find the ffmpeg directory (it usually contains 'ffmpeg' in the name)
            const ffmpegFolder = extractedFiles.find(f => f.toLowerCase().includes('ffmpeg'));
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

            // Update shell configuration files
            mainWin.webContents.send('installation-progress', {
                percent: 80,
                status: 'Updating system PATH...'
            });

            const shellConfigs = ['.bashrc', '.zshrc', '.profile'];
            const exportCommand = `\n# FFmpeg PATH\nexport PATH="$PATH:${ffmpegDir}"\n`;

            for (const config of shellConfigs) {
                const configPath = path.join(homeDir, config);
                try {
                    if (fs.existsSync(configPath)) {
                        const content = fs.readFileSync(configPath, 'utf8');
                        if (!content.includes(ffmpegDir)) {
                            console.log(`Updating ${config}`);
                            fs.appendFileSync(configPath, exportCommand);
                        }
                    }
                } catch (err) {
                    console.warn(`Failed to update ${config}:`, err);
                }
            }

            // Verify installation
            try {
                console.log('Verifying installation...');
                const ffmpegPath = path.join(ffmpegDir, 'ffmpeg');
                if (fs.existsSync(ffmpegPath)) {
                    const stats = fs.statSync(ffmpegPath);
                    console.log('FFmpeg exists:', stats.mode);

                    // Test execution
                    const result = spawn(ffmpegPath, ['-version']);
                    result.on('error', (err) => {
                        console.error('Execution test failed:', err);
                        throw err;
                    });

                    await new Promise((resolve, reject) => {
                        result.on('close', (code) => {
                            if (code === 0) {
                                resolve();
                            } else {
                                reject(new Error(`FFmpeg execution failed with code ${code}`));
                            }
                        });
                    });
                } else {
                    throw new Error('FFmpeg binary not found after installation');
                }
            } catch (err) {
                console.error('Verification failed:', err);
                throw new Error('FFmpeg installation verification failed: ' + err.message);
            }
        }

        mainWin.webContents.send('installation-progress', {
            percent: 80,
            status: 'Configuring FFmpeg...'
        });

        // Configure FFmpeg path for non-Windows platforms
        if (platform !== 'win32') {
            mainWin.webContents.send('installation-progress', {
                percent: 90,
                status: 'Adding FFmpeg to system PATH...'
            });
            const shell = process.env.SHELL || '/bin/bash';
            let rcFile;

            if (shell.includes('zsh')) {
                rcFile = path.join(homeDir, '.zshrc');
            } else if (shell.includes('bash')) {
                rcFile = path.join(homeDir, '.bashrc');
            } else {
                rcFile = path.join(homeDir, '.profile');
            }

            const exportCommand = `\n# Add FFmpeg to PATH\nexport PATH="${ffmpegDir}:$PATH"\n`;

            if (fs.existsSync(rcFile)) {
                const rcFileContent = fs.readFileSync(rcFile, 'utf8');
                if (!rcFileContent.includes(ffmpegDir)) {
                    fs.appendFileSync(rcFile, exportCommand, 'utf8');
                }
            } else {
                fs.writeFileSync(rcFile, exportCommand, 'utf8');
            }
        }

        // Clean up and finish
        fs.rmSync(tempDir, { recursive: true, force: true });

        mainWin.webContents.send('installation-progress', {
            percent: 100,
            status: 'FFmpeg installed successfully! Please restart your terminal to use FFmpeg.'
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

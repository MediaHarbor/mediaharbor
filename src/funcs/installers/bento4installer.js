const os = require("os");
const path = require("path");
const fs = require("fs");
const axios = require("axios");
const ProgressBar = require("progress");
const unzipper = require("unzipper");
const { spawn } = require("child_process");

const config = {
    version: "1-6-0-641",
    baseUrl: "https://www.bok.net/Bento4/binaries",
    installDir: process.env.BENTO4_INSTALL_DIR || null,
};

function getDownloadInfo(platform, arch, version) {
    let downloadUrl;
    let defaultInstallDir;

    const versionedPath = `Bento4-SDK-${version}`;

    if (platform === 'win32') {
        const archSuffix = arch === 'x64' ? 'x86_64-microsoft-win32' : 'x86-microsoft-win32';
        downloadUrl = `${config.baseUrl}/${versionedPath}.${archSuffix}.zip`;
        defaultInstallDir = path.join(os.homedir(), 'Bento4');
    } else if (platform === 'darwin') {
        downloadUrl = `${config.baseUrl}/${versionedPath}.universal-apple-macosx.zip`;
        defaultInstallDir = path.join('/Applications', 'Bento4');
    } else if (platform === 'linux') {
        const archSuffix = arch === 'x64' ? 'x86_64-unknown-linux' : `${arch}-unknown-linux`;
        downloadUrl = `${config.baseUrl}/${versionedPath}.${archSuffix}.zip`;
        defaultInstallDir = path.join(os.homedir(), 'Bento4');
    } else {
        throw new Error(`Unsupported platform: ${platform}`);
    }

    return { downloadUrl, defaultInstallDir };
}

/**
 * Logs messages to the UI or console.
 */
function createLogger(mainWin) {
    return (message) => {
        if (mainWin && typeof mainWin.webContents.send === 'function') {
            mainWin.webContents.send('log', message);
        } else {
            console.log(message);
        }
    };
}

/**
 * Adds Bento4 'bin' directory to system PATH.
 */
async function addBento4ToPath(binDir, platform, log, mainWin) {
    if (platform === 'darwin') {

        const files = fs.readdirSync(binDir);
        for (const file of files) {
            const filePath = path.join(binDir, file);
            try {

                fs.chmodSync(filePath, '755');
                log(`Set executable permissions for ${file}`);
            } catch (error) {
                log(`Error setting permissions for ${file}: ${error.message}`);
            }
        }

        const shellFiles = [
            path.join(os.homedir(), '.zshrc'),
            path.join(os.homedir(), '.bash_profile')
        ];

        for (const rcFile of shellFiles) {
            try {
                const exportCommand = `\n# Added by Bento4 installer\nexport PATH="${binDir}:$PATH"\n`;

                if (fs.existsSync(rcFile)) {
                    const rcContent = fs.readFileSync(rcFile, 'utf8');
                    if (!rcContent.includes(binDir)) {
                        fs.appendFileSync(rcFile, exportCommand);
                        log(`Added Bento4 to PATH in ${rcFile}`);
                    } else {
                        log(`Bento4 already in PATH in ${rcFile}`);
                    }
                } else {
                    fs.writeFileSync(rcFile, exportCommand);
                    log(`Created ${rcFile} with Bento4 PATH`);
                }
            } catch (error) {
                log(`Error updating ${rcFile}: ${error.message}`);
            }
        }

        const localBinDir = '/usr/local/bin';
        try {
            if (!fs.existsSync(localBinDir)) {
                fs.mkdirSync(localBinDir, { recursive: true });
            }

            files.forEach(file => {
                const sourcePath = path.join(binDir, file);
                const targetPath = path.join(localBinDir, file);

                try {
                    if (fs.existsSync(targetPath)) {
                        fs.unlinkSync(targetPath);
                    }
                    fs.symlinkSync(sourcePath, targetPath);
                    log(`Created symlink for ${file} in ${localBinDir}`);
                } catch (error) {
                    log(`Error creating symlink for ${file}: ${error.message}`);
                }
            });
        } catch (error) {
            log(`Error accessing ${localBinDir}: ${error.message}`);
        }

        return;
    }

    if (platform === 'win32') {
        return new Promise((resolve, reject) => {
            const normalizedBinDir = binDir.replace(/\
            const psCommand = `
                $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
                $binDir = '${normalizedBinDir}'

                if ($userPath -split ';' -notcontains $binDir) {
                    $newPath = $userPath + ';' + $binDir
                    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
                    Write-Output "Added Bento4 to PATH"
                } else {
                    Write-Output "Bento4 already in PATH"
                }
            `;

            const psProcess = spawn('powershell.exe', [
                '-NoProfile',
                '-NonInteractive',
                '-Command',
                psCommand
            ]);

            let errorOutput = '';
            let output = '';

            psProcess.stdout.on('data', (data) => {
                output += data.toString();
            });

            psProcess.stderr.on('data', (data) => {
                errorOutput += data.toString();
                mainWin.webContents.send('installation-progress', {
                    percent: 90,
                    status: 'Configuring system PATH...'
                });
            });

            psProcess.on('close', (code) => {
                if (code === 0) {
                    log(output.trim());
                    const executableExtensions = ['.exe', ''];
                    fs.readdirSync(normalizedBinDir).forEach(file => {
                        const ext = path.extname(file);
                        const baseName = path.basename(file, ext);

                        if (executableExtensions.includes(ext)) {
                            const cmdPath = path.join(normalizedBinDir, `${baseName}.cmd`);
                            const cmdContent = `@echo off\n"%~dp0${file}" %*`;
                            fs.writeFileSync(cmdPath, cmdContent);
                        }
                    });
                    resolve();
                } else {
                    reject(new Error(`Failed to update PATH: ${errorOutput || 'Unknown error'}`));
                }
            });
        });
    } else {

        const shell = process.env.SHELL || '/bin/bash';
        let rcFile;
        if (shell.includes('zsh')) {
            rcFile = path.join(os.homedir(), '.zshrc');
        } else if (shell.includes('bash')) {
            rcFile = path.join(os.homedir(), '.bashrc');
        } else {
            rcFile = path.join(os.homedir(), '.profile');
        }

        const exportCommand = `\n# Added by Bento4 installer\nexport PATH="${binDir}:$PATH"\n`;

        if (fs.existsSync(rcFile)) {
            const rcFileContent = fs.readFileSync(rcFile, 'utf8');
            if (!rcFileContent.includes(binDir)) {
                fs.appendFileSync(rcFile, exportCommand, 'utf8');
                log(`Added Bento4 'bin' directory to PATH in ${rcFile}.`);
            } else {
                log(`'${binDir}' is already in PATH within ${rcFile}.`);
            }
        } else {
            fs.writeFileSync(rcFile, exportCommand, 'utf8');
            log(`Created ${rcFile} and added Bento4 'bin' directory to PATH.`);
        }
    }
}

async function downloadFile(url, destPath, log, mainWin) {
    log(`Starting download from ${url}`);
    const writer = fs.createWriteStream(destPath);

    const response = await axios({
        method: 'GET',
        url: url,
        responseType: 'stream',
    });

    const totalLength = parseInt(response.headers['content-length'], 10);
    let downloaded = 0;

    response.data.on('data', (chunk) => {
        downloaded += chunk.length;

        const progress = Math.floor((downloaded / totalLength) * 100);

        if (mainWin && typeof mainWin.webContents.send === 'function') {
            mainWin.webContents.send('installation-progress', {
                percent: Math.floor(progress * 0.4),
                status: `Downloading Bento4: ${progress}%`
            });
        }
    });

    response.data.pipe(writer);
    return new Promise((resolve, reject) => {
        writer.on('finish', () => {
            log('Download completed.');
            resolve();
        });
        writer.on('error', (err) => {
            reject(err);
        });
    });
}

async function extractZip(zipPath, extractTo, log, mainWin) {
    log(`Extracting ${zipPath} to ${extractTo}`);

    if (mainWin && typeof mainWin.webContents.send === 'function') {
        mainWin.webContents.send('installation-progress', {
            percent: 40,
            status: 'Extracting Bento4...'
        });
    }

    let extractionProgress = 0;
    const updateInterval = setInterval(() => {
        extractionProgress += 5;
        if (extractionProgress <= 35) {
            if (mainWin && typeof mainWin.webContents.send === 'function') {
                mainWin.webContents.send('installation-progress', {
                    percent: 40 + extractionProgress,
                    status: `Extracting Bento4: ${Math.min(100, Math.floor(extractionProgress * 100 / 35))}%`
                });
            }
        }
    }, 500);

    await fs.createReadStream(zipPath)
        .pipe(unzipper.Extract({ path: extractTo }))
        .promise();

    clearInterval(updateInterval);

    if (mainWin && typeof mainWin.webContents.send === 'function') {
        mainWin.webContents.send('installation-progress', {
            percent: 75,
            status: 'Extraction completed'
        });
    }

    log(`Extraction completed to ${extractTo}`);
}

/**
 * Main function to download and install Bento4.
 */
async function downloadAndInstallBento4(mainWin, options = {}) {
    const log = createLogger(mainWin);

    try {
        log('Starting Bento4 installation...');
        const platform = os.platform();
        const arch = os.arch();
        const version = options.version || config.version;
        const { downloadUrl, defaultInstallDir } = getDownloadInfo(platform, arch, version);
        const installDir = options.installDir || config.installDir || defaultInstallDir;

        log(`Detected platform: ${platform}`);
        log(`Architecture: ${arch}`);
        log(`Bento4 version: ${version}`);
        log(`Download URL: ${downloadUrl}`);
        log(`Installation directory: ${installDir}`);

        if (!fs.existsSync(installDir)) {
            fs.mkdirSync(installDir, { recursive: true });
            log(`Created installation directory at ${installDir}`);
        } else {
            log(`Installation directory exists at ${installDir}`);
        }
        if (mainWin && typeof mainWin.webContents.send === 'function') {
            mainWin.webContents.send('installation-progress', {
                percent: 0,
                status: 'Starting Bento4 installation...'
            });
        }

        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'bento4-'));
        const zipPath = path.join(tempDir, 'Bento4-SDK.zip');

        await downloadFile(downloadUrl, zipPath, log, mainWin);

        await extractZip(zipPath, tempDir, log, mainWin);

        function findBinDir(startPath) {
            if (!fs.existsSync(startPath)) return null;

            const items = fs.readdirSync(startPath);

            if (items.includes('bin')) {
                return path.join(startPath, 'bin');
            }

            for (const item of items) {
                const itemPath = path.join(startPath, item);
                if (fs.statSync(itemPath).isDirectory()) {
                    const binPath = findBinDir(itemPath);
                    if (binPath) return binPath;
                }
            }

            return null;
        }

        const binDir = findBinDir(tempDir);

        if (!binDir) {
            throw new Error('Bento4 bin directory not found after extraction');
        }

        if (!fs.existsSync(installDir)) {
            fs.mkdirSync(installDir, { recursive: true });
        }

        const sdkDir = path.dirname(binDir);
        const finalBinDir = path.join(installDir, 'bin');

        if (fs.existsSync(finalBinDir)) {
            fs.rmSync(finalBinDir, { recursive: true, force: true });
        }

        fs.cpSync(binDir, finalBinDir, { recursive: true });

        if (platform === 'darwin') {
            log('Setting up permissions for macOS...');

            fs.chmodSync(finalBinDir, '755');
            log('Set permissions for bin directory');
        }

        log('Adding Bento4 to system PATH...');
        if (mainWin && typeof mainWin.webContents.send === 'function') {
            mainWin.webContents.send('installation-progress', {
                percent: 80,
                status: 'Configuring system PATH...'
            });
        }

        await addBento4ToPath(finalBinDir, platform, log, mainWin);

        fs.rmSync(tempDir, { recursive: true, force: true });
        log('Cleaned up temporary files.');

        if (mainWin && typeof mainWin.webContents.send === 'function') {
            mainWin.webContents.send('installation-progress', {
                percent: 100,
                status: 'Bento4 installed successfully! Please restart your terminal to use Bento4.'
            });
        }

        log('Bento4 has been successfully downloaded and installed. Please restart your terminal or system for the PATH changes to take effect.');
    } catch (error) {
        log(`Installation failed: ${error.message}`);
        if (error.stack) {
            log(`Stack trace: ${error.stack}`);
        }
        if (mainWin && typeof mainWin.webContents.send === 'function') {
            mainWin.webContents.send('installation-error', error.message);
        }
        throw error;
    }
}

module.exports = { downloadAndInstallBento4 };
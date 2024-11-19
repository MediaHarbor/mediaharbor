const os = require("os");
const path = require("path");
const fs = require("fs");
const axios = require("axios");
const ProgressBar = require("progress");
const unzipper = require("unzipper");
const { spawn } = require("child_process");

const config = {
    version: "1-6-0-641", // If they update version
    baseUrl: "https://www.bok.net/Bento4/binaries", //If they move website
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
    if (platform === 'win32') {
        return new Promise((resolve, reject) => {
            const normalizedBinDir = binDir.replace(/\//g, '\\');
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
        // Unix-like systems
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

    const bar = new ProgressBar('Downloading [:bar] :percent :etas', {
        width: 40,
        complete: '=',
        incomplete: ' ',
        total: totalLength,
    });

    response.data.on('data', (chunk) => {
        downloaded += chunk.length;
        bar.tick(chunk.length);
        const progress = Math.floor((downloaded / totalLength) * 100);
        const adjustedProgress = Math.min(Math.floor(progress * 0.4), 40);
        mainWin.webContents.send('installation-progress', {
            percent: adjustedProgress,
            status: `Downloading Bento4: ${progress}%`
        });
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
    mainWin.webContents.send('installation-progress', {
        percent: 40,
        status: 'Extracting Bento4...'
    });

    await fs.createReadStream(zipPath)
        .pipe(unzipper.Extract({ path: extractTo }))
        .promise();

    log(`Extraction completed to ${extractTo}`);
}

/**
 * Main function to download and install Bento4.
 */
async function downloadAndInstallBento4(mainWin, options = {}) {
    try {
        const platform = os.platform();
        const arch = os.arch();
        const version = options.version || config.version;
        const { downloadUrl, defaultInstallDir } = getDownloadInfo(platform, arch, version);
        const installDir = options.installDir || config.installDir || defaultInstallDir;

        const log = createLogger(mainWin);

        log(`Detected platform: ${platform}`);
        log(`Architecture: ${arch}`);
        log(`Bento4 version: ${version}`);
        log(`Download URL: ${downloadUrl}`);
        log(`Installation directory: ${installDir}`);

        // Create installation directory if it doesn't exist
        if (!fs.existsSync(installDir)) {
            fs.mkdirSync(installDir, { recursive: true });
            log(`Created installation directory at ${installDir}`);
        } else {
            log(`Installation directory exists at ${installDir}`);
        }

        const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'bento4-'));
        const zipPath = path.join(tempDir, 'Bento4-SDK.zip');

        await downloadFile(downloadUrl, zipPath, log, mainWin);

        await extractZip(zipPath, installDir, log, mainWin);

        const zipBaseName = `Bento4-SDK-${version}`;
        const extractedFolderPath = path.join(installDir, zipBaseName);
        const binDir = path.join(extractedFolderPath, 'bin');

        if (!fs.existsSync(binDir)) {
            throw new Error('Bento4 bin directory not found after extraction');
        }

        // Add to PATH
        log('Adding Bento4 to system PATH...');
        mainWin.webContents.send('installation-progress', {
            percent: 80,
            status: 'Configuring system PATH...'
        });

        await addBento4ToPath(binDir, platform, log, mainWin);

        // Clean up
        fs.rmSync(tempDir, { recursive: true, force: true });
        log('Cleaned up temporary files.');

        // Final Progress Update
        mainWin.webContents.send('installation-progress', {
            percent: 100,
            status: 'Bento4 installed successfully! Please restart your terminal to use Bento4.'
        });

        log('Bento4 has been successfully downloaded and installed. Please restart your terminal or system for the PATH changes to take effect.');
    } catch (error) {
        const log = createLogger(mainWin);
        if (mainWin && typeof mainWin.webContents.send === 'function') {
            mainWin.webContents.send('installation-error', error.message);
        }
        log(`An error occurred: ${error.message}`);
        throw error; // Re-throw the error after logging
    }
}

module.exports = { downloadAndInstallBento4 };

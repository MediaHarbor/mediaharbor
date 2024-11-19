const { dialog } = require('electron');
const https = require('https');
const fs = require('fs');
const path = require('path');
const { exec, spawn } = require('child_process');
const os = require('os');
const { URL } = require('url');
const sudo = require('sudo-prompt'); // Added sudo-prompt

/**
 * Fetch available Python versions from the official Python API.
 * This allows dynamic retrieval of versions instead of hardcoding.
 */
async function fetchPythonVersions() {
    return new Promise((resolve, reject) => {
        const url = 'https://www.python.org/api/v2/downloads/release/?is_published=true&format=json';
        https.get(url, (res) => {
            let data = '';
            res.on('data', chunk => data += chunk);
            res.on('end', () => {
                try {
                    const releases = JSON.parse(data).results;
                    // Filter for major versions and latest patch releases
                    const versionMap = {};
                    releases.forEach(release => {
                        const versionMatch = release.version.match(/^Python (\d+\.\d+)\.\d+$/);
                        if (versionMatch) {
                            const majorMinor = versionMatch[1];
                            if (!versionMap[majorMinor] || release.python_version > versionMap[majorMinor]) {
                                versionMap[majorMinor] = release.version;
                            }
                        }
                    });
                    resolve(versionMap);
                } catch (error) {
                    reject(new Error('Failed to parse Python versions.'));
                }
            });
        }).on('error', (err) => reject(err));
    });
}

/**
 * Display a dialog for the user to select a Python version to install.
 */
async function selectPythonVersion(win, pythonVersions) {
    const versionKeys = Object.keys(pythonVersions).sort((a, b) => {
        const [aMajor, aMinor] = a.split('.').map(Number);
        const [bMajor, bMinor] = b.split('.').map(Number);
        return aMajor !== bMajor ? bMajor - aMajor : bMinor - aMinor;
    });

    const { response } = await dialog.showMessageBox(win, {
        type: 'question',
        buttons: [...versionKeys.map(v => `Python ${pythonVersions[v]}`), 'Cancel'],
        title: 'Select Python Version',
        message: 'Choose the Python version you want to install:',
        defaultId: 0,
        cancelId: versionKeys.length,
    });

    if (response === versionKeys.length) {
        throw new Error('User canceled the installation.');
    }

    const selectedVersionKey = versionKeys[response];
    const fullVersion = pythonVersions[selectedVersionKey];
    console.log(`Selected Python version: ${fullVersion}`);
    return { selectedVersionKey, fullVersion };
}

/**
 * Determine the download URL and installer path based on the OS and Python version.
 */
function getDownloadDetails(fullVersion) {
    const platform = os.platform();
    const arch = os.arch();
    let downloadUrl;
    let installerPath;

    if (platform === 'win32') {
        downloadUrl = `https://www.python.org/ftp/python/${fullVersion}/python-${fullVersion}-amd64.exe`;
        installerPath = path.join(os.tmpdir(), `python-${fullVersion}-amd64.exe`);
    } else if (platform === 'darwin') {
        const pkgSuffix = compareVersions(fullVersion, '3.9.13') >= 0 ? 'macos11' : 'macosx10.9';
        downloadUrl = `https://www.python.org/ftp/python/${fullVersion}/python-${fullVersion}-${pkgSuffix}.pkg`;
        installerPath = path.join(os.tmpdir(), path.basename(downloadUrl));
    } else if (platform === 'linux') {
        throw new Error(`Automatic installation is not supported on ${platform}. Please install Python ${fullVersion} using your package manager.`);
    } else {
        throw new Error(`Your platform (${platform}) is not supported.`);
    }

    return { downloadUrl, installerPath, platform };
}

function compareVersions(v1, v2) {
    const v1Parts = v1.split('.').map(Number);
    const v2Parts = v2.split('.').map(Number);
    for (let i = 0; i < Math.max(v1Parts.length, v2Parts.length); i++) {
        const a = v1Parts[i] || 0;
        const b = v2Parts[i] || 0;
        if (a > b) return 1;
        if (a < b) return -1;
    }
    return 0;
}

/**
 * Download a file with progress updates.
 */
function downloadFile(url, dest, win) {
    return new Promise((resolve, reject) => {
        const file = fs.createWriteStream(dest);
        const parsedUrl = new URL(url);

        const request = https.get(parsedUrl, {
            headers: {
                'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36'
            }
        }, (response) => {
            if ([301, 302].includes(response.statusCode)) {
                downloadFile(response.headers.location, dest, win).then(resolve).catch(reject);
                return;
            }

            if (response.statusCode !== 200) {
                reject(new Error(`Failed to download: Server responded with status code ${response.statusCode}`));
                return;
            }

            const totalSize = parseInt(response.headers['content-length'], 10);
            let downloaded = 0;

            response.pipe(file);

            response.on('data', (chunk) => {
                downloaded += chunk.length;
                const progress = Math.floor((downloaded / totalSize) * 100);
                win.webContents.send('installation-progress', {
                    percent: Math.floor(progress * 0.4), // 40% allocated to download
                    status: `Downloading Python: ${progress}%`
                });
            });

            file.on('finish', () => {
                file.close(() => resolve());
            });
        });

        request.on('error', (err) => {
            fs.unlink(dest, () => reject(err));
        });

        file.on('error', (err) => {
            fs.unlink(dest, () => reject(err));
        });
    });
}

/**
 * Execute the installer based on the platform.
 */
function runInstaller(installerPath, platform, win) {
    return new Promise((resolve, reject) => {
        win.webContents.send('installation-progress', {
            percent: 40,
            status: 'Running Python installer...'
        });

        if (platform === 'win32') {
            const args = ['/quiet', 'InstallAllUsers=1', 'PrependPath=1'];
            const options = { shell: true };
            const installerProcess = spawn(installerPath, args, options);

            installerProcess.stdout.on('data', (data) => {
                console.log(`Installer stdout: ${data}`);
            });

            installerProcess.stderr.on('data', (data) => {
                console.error(`Installer stderr: ${data}`);
            });

            installerProcess.on('close', (code) => {
                if (code === 0) {
                    win.webContents.send('installation-progress', {
                        percent: 80,
                        status: 'Installation completed, configuring system...'
                    });
                    resolve();
                } else {
                    reject(new Error(`Installer exited with code ${code}`));
                }
            });
        } else if (platform === 'darwin') {
            const command = `installer -pkg "${installerPath}" -target /`;
            const options = {
                name: 'Python Installer',
            };
            sudo.exec(command, options, (error, stdout, stderr) => {
                if (error) {
                    console.error('Installer error:', error);
                    reject(new Error(`Installer failed: ${stderr || error.message}`));
                    return;
                }

                // Since sudo-prompt doesn't provide real-time progress, we send a fixed progress update
                win.webContents.send('installation-progress', {
                    percent: 80,
                    status: 'Installation completed, configuring system...'
                });
                resolve();
            });
        } else {
            reject(new Error('Unsupported platform for installer execution.'));
        }
    });
}

/**
 * Update the system PATH environment variable to include Python.
 */
function updatePath(versionKey, fullVersion, platform, win) {
    return new Promise((resolve, reject) => {
        win.webContents.send('installation-progress', {
            percent: 90,
            status: 'Updating system PATH...'
        });

        let pythonPath;
        let scriptsPath;

        if (platform === 'win32') {
            const userHomeDir = process.env.USERPROFILE || process.env.HOME;
            const formattedVersion = versionKey.replace('.', '');

            pythonPath = path.join(userHomeDir, 'AppData', 'Local', 'Programs', 'Python', `Python${formattedVersion}`);
            scriptsPath = path.join(pythonPath, 'Scripts');

            console.log(`Adding to PATH: ${pythonPath};${scriptsPath}`);

            // Update system PATH using setx
            exec(`setx PATH "%PATH%;${pythonPath};${scriptsPath}"`, (error) => {
                if (error) {
                    console.error('Error updating PATH:', error);
                    reject(error);
                    return;
                }
                resolve();
            });
        } else if (platform === 'darwin') {
            pythonPath = `/Library/Frameworks/Python.framework/Versions/${fullVersion}/bin`;
            scriptsPath = pythonPath;

            const shell = process.env.SHELL || '/bin/bash';
            let profilePath = '';

            if (shell.includes('zsh')) {
                profilePath = path.join(os.homedir(), '.zshrc');
            } else {
                profilePath = path.join(os.homedir(), '.bash_profile');
            }

            const exportCommand = `export PATH="${pythonPath}:${scriptsPath}:$PATH"\n`;

            fs.appendFile(profilePath, exportCommand, (err) => {
                if (err) {
                    reject(err);
                    return;
                }
                resolve();
            });
        } else {
            reject(new Error('Unsupported platform for PATH update.'));
        }

        // Notify completion
        win.webContents.send('installation-progress', {
            percent: 100,
            status: 'Python installation completed successfully!'
        });
    });
}

/**
 * Relaunch the Electron application to apply PATH changes.
 */
function relaunchAppIfPossible(app) {
    if (app && typeof app.relaunch === 'function') {
        app.relaunch();
        app.exit(0);
    }
}

/**
 * Main function to handle the download and installation of Python.
 */
async function downloadAndInstallPython(win, app) {
    try {
        const pythonVersions = await fetchPythonVersions();

        const { selectedVersionKey, fullVersion } = await selectPythonVersion(win, pythonVersions);

        const { downloadUrl, installerPath, platform } = getDownloadDetails(fullVersion);

        win.webContents.send('installation-progress', {
            percent: 0,
            status: 'Starting Python installation...'
        });

        console.log(`Downloading from: ${downloadUrl}`);
        await downloadFile(downloadUrl, installerPath, win);
        console.log(`Downloaded installer to ${installerPath}`);

        await runInstaller(installerPath, platform, win);
        console.log('Python installation initiated.');

        await updatePath(selectedVersionKey, fullVersion, platform, win);
        console.log('Updated system PATH.');

        await dialog.showMessageBox(win, {
            type: 'info',
            buttons: ['OK'],
            title: 'Installation Complete',
            message: 'Python has been installed successfully. The application will now reload.',
        });

        relaunchAppIfPossible(app);

    } catch (error) {
        console.error('Error in downloadAndInstallPython:', error);
        dialog.showErrorBox('Installation Error', `An error occurred: ${error.message}`);
    }
}

module.exports = {
    downloadAndInstallPython,
};

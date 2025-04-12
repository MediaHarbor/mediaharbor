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
            res.on('end', async () => {
                try {
                    const releases = JSON.parse(data);
                    const versionMap = {};
                    
                    for (const release of releases) {
                        const versionMatch = release.name.match(/Python (\d+)\.(\d+)\.\d+$/);
                        if (versionMatch) {
                            const major = parseInt(versionMatch[1]);
                            const minor = parseInt(versionMatch[2]);
                            const version = release.name.replace('Python ', '');

                            // Only include Python 3.10 and above
                            if (major === 3 && minor >= 10) {
                                // Check if installer exists before adding to map
                                const { downloadUrl } = getDownloadDetails(version);
                                if (await checkUrlExists(downloadUrl)) {
                                    const majorMinor = `${major}.${minor}`;
                                    if (!versionMap[majorMinor] || 
                                        (release.release_date > versionMap[majorMinor].release_date)) {
                                        versionMap[majorMinor] = release;
                                    }
                                }
                            }
                        }
                    }

                    const finalVersionMap = {};
                    Object.entries(versionMap).forEach(([key, release]) => {
                        finalVersionMap[key] = release.name.replace('Python ', '');
                    });

                    resolve(finalVersionMap);
                } catch (error) {
                    console.error('Parse error:', error);
                    reject(new Error(`Failed to parse Python versions: ${error.message}`));
                }
            });
        }).on('error', (err) => {
            console.error('Network error:', err);
            reject(new Error(`Failed to fetch Python versions: ${err.message}`));
        });
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
            const args = ['/quiet', 'InstallAllUsers=0', 'PrependPath=1', 'Include_test=0', 'AssociateFiles=1'];
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
 * Handles all platforms and ensures both system and user script paths are included.
 */
function updatePath(versionKey, fullVersion, platform, win) {
    return new Promise((resolve, reject) => {
        try {
            win.webContents.send('installation-progress', {
                percent: 90,
                status: 'Updating system PATH...'
            });

            if (platform === 'win32') {
                return
            }
                else if (platform === 'darwin') {
                const pythonPath = `/Library/Frameworks/Python.framework/Versions/${versionKey}/bin`;
                const userBinPath = path.join(os.homedir(), 'Library', 'Python', versionKey, 'bin');

                console.log(`Adding to PATH: ${pythonPath}:${userBinPath}`);

                // Create directories if they don't exist
                fs.mkdirSync(userBinPath, { recursive: true });

                // Determine shell profile file
                const shell = process.env.SHELL || '/bin/bash';
                let profilePath;

                if (shell.includes('zsh')) {
                    profilePath = path.join(os.homedir(), '.zshrc');
                } else {
                    profilePath = path.join(os.homedir(), '.bash_profile');
                    // If .bash_profile doesn't exist, try .profile
                    if (!fs.existsSync(profilePath)) {
                        profilePath = path.join(os.homedir(), '.profile');
                    }
                }

                // Check if path already exists in profile to avoid duplication
                let profileContent = '';
                try {
                    if (fs.existsSync(profilePath)) {
                        profileContent = fs.readFileSync(profilePath, 'utf8');
                    }
                } catch (readErr) {
                    console.warn(`Could not read profile file ${profilePath}:`, readErr);
                }

                const exportCommand = `\n# Added by Python installer\nexport PATH="${pythonPath}:${userBinPath}:$PATH"\n`;

                if (!profileContent.includes(pythonPath) && !profileContent.includes(userBinPath)) {
                    fs.appendFile(profilePath, exportCommand, (err) => {
                        if (err) {
                            console.error(`Error updating ${profilePath}:`, err);
                            reject(new Error(`Failed to update shell profile: ${err.message}`));
                            return;
                        }

                        // Also update current process PATH
                        process.env.PATH = `${pythonPath}:${userBinPath}:${process.env.PATH}`;
                        console.log(`Updated ${profilePath} with Python paths`);
                        resolve();
                    });
                } else {
                    console.log('Python paths already exist in shell profile');
                    resolve();
                }
            } else if (platform === 'linux') {
                // For Linux, we'll update .bashrc
                const pythonPath = `/usr/local/bin`;
                const userBinPath = path.join(os.homedir(), '.local', 'bin');

                console.log(`Adding to PATH: ${pythonPath}:${userBinPath}`);

                // Create user bin directory if it doesn't exist
                fs.mkdirSync(userBinPath, { recursive: true });

                // Choose profile file based on shell
                const shell = process.env.SHELL || '/bin/bash';
                let profilePath;

                if (shell.includes('zsh')) {
                    profilePath = path.join(os.homedir(), '.zshrc');
                } else {
                    profilePath = path.join(os.homedir(), '.bashrc');
                }

                // Check if path already exists in profile
                let profileContent = '';
                try {
                    if (fs.existsSync(profilePath)) {
                        profileContent = fs.readFileSync(profilePath, 'utf8');
                    }
                } catch (readErr) {
                    console.warn(`Could not read profile file ${profilePath}:`, readErr);
                }

                const exportCommand = `\n# Added by Python installer\nexport PATH="${userBinPath}:${pythonPath}:$PATH"\n`;

                if (!profileContent.includes(userBinPath)) {
                    fs.appendFile(profilePath, exportCommand, (err) => {
                        if (err) {
                            console.error(`Error updating ${profilePath}:`, err);
                            reject(new Error(`Failed to update shell profile: ${err.message}`));
                            return;
                        }

                        // Update current process PATH
                        process.env.PATH = `${userBinPath}:${pythonPath}:${process.env.PATH}`;
                        console.log(`Updated ${profilePath} with Python paths`);
                        resolve();
                    });
                } else {
                    console.log('Python paths already exist in shell profile');
                    resolve();
                }
            } else {
                reject(new Error(`Unsupported platform: ${platform}`));
            }

            // Notify completion once PATH is updated
            win.webContents.send('installation-progress', {
                percent: 100,
                status: 'Python installation completed successfully!'
            });

        } catch (error) {
            console.error('Exception in updatePath:', error);
            reject(new Error(`Failed to update PATH: ${error.message}`));
        }
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
 * New helper function to check if a URL exists.
 */
async function checkUrlExists(url) {
    return new Promise((resolve) => {
        const options = new URL(url);
        options.method = 'HEAD';
        const req = https.request(options, (res) => {
            resolve(res.statusCode === 200);
        });
        req.on('error', () => resolve(false));
        req.end();
    });
}

/**
 * Main function to handle the download and installation of Python with fallback to previous minor version.
 */
async function downloadAndInstallPython(win, app) {
    try {
        const pythonVersions = await fetchPythonVersions();

        // Get sorted version keys (sorted descending)
        const versionKeys = Object.keys(pythonVersions).sort((a, b) => {
            const [aMajor, aMinor] = a.split('.').map(Number);
            const [bMajor, bMinor] = b.split('.').map(Number);
            return bMajor !== aMajor ? bMajor - aMajor : bMinor - aMinor;
        });

        // Show dialog and get selected version key
        const { selectedVersionKey, fullVersion } = await selectPythonVersion(win, pythonVersions);
        let currentIndex = versionKeys.indexOf(selectedVersionKey);
        if (currentIndex === -1) {
            throw new Error("Selected version not found in version list.");
        }

        let currentVersionKey = versionKeys[currentIndex];
        let currentFullVersion = pythonVersions[currentVersionKey];
        let { downloadUrl, installerPath, platform } = getDownloadDetails(currentFullVersion);

        // Loop to fallback if installer URL is not available
        while (!(await checkUrlExists(downloadUrl))) {
            currentIndex++;
            if (currentIndex >= versionKeys.length) {
                throw new Error(`No available installer found for ${currentFullVersion} or older.`);
            }
            currentVersionKey = versionKeys[currentIndex];
            currentFullVersion = pythonVersions[currentVersionKey];
            console.log(`Installer for Python ${downloadUrl} not found. Falling back to Python ${currentFullVersion}`);
            ({ downloadUrl, installerPath, platform } = getDownloadDetails(currentFullVersion));
        }

        win.webContents.send('installation-progress', {
            percent: 0,
            status: 'Starting Python installation...'
        });

        console.log(`Downloading from: ${downloadUrl}`);
        await downloadFile(downloadUrl, installerPath, win);
        console.log(`Downloaded installer to ${installerPath}`);

        await runInstaller(installerPath, platform, win);
        console.log('Python installation initiated.');

        await updatePath(currentVersionKey, currentFullVersion, platform, win);
        console.log('Updated system PATH.');

        await dialog.showMessageBox(win, {
            type: 'info',
            buttons: ['OK'],
            title: 'Installation Complete',
            message: 'Python has been installed successfully. The application will now reload.',
        });

        relaunchAppIfPossible(app);
        setTimeout(() => {
            if (app && typeof app.relaunch === 'function') {
                app.relaunch();
                app.exit(0);
            } else {
                if (win && !win.isDestroyed()) {
                    win.reload();
                }
            }
        }, 1000);

    } catch (error) {
        console.error('Error in downloadAndInstallPython:', error);
        dialog.showErrorBox('Installation Error', `An error occurred: ${error.message}`);
    }
}

module.exports = {
    downloadAndInstallPython,
};

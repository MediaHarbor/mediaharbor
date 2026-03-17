const https = require('https');
const fs = require('fs');
const path = require('path');
const { exec } = require('child_process');
const os = require('os');
const axios = require('axios');

async function fetchLatestGitWindowsUrl() {
    return new Promise((resolve, reject) => {
        const req = https.get(
            'https://api.github.com/repos/git-for-windows/git/releases/latest',
            { headers: { 'User-Agent': 'MediaHarbor/1.0' } },
            (res) => {
                let data = '';
                res.on('data', c => data += c);
                res.on('end', () => {
                    try {
                        const json = JSON.parse(data);
                        const arch = os.arch() === 'arm64' ? 'arm64' : '64-bit';
                        const asset = json.assets.find(a => a.name.endsWith(`-${arch}.exe`));
                        if (asset) resolve(asset.browser_download_url);
                        else reject(new Error('Git installer asset not found'));
                    } catch (e) { reject(e); }
                });
            }
        );
        req.on('error', reject);
        req.setTimeout(10000, () => { req.destroy(); reject(new Error('timeout')); });
    });
}

async function downloadAndInstallGit(win) {
    const platform = os.platform();
    const tempDir = os.tmpdir();
    let installerPath;
    let downloadUrl;

    try {
        if (platform === 'win32') {
            win.webContents.send('installation-progress', { percent: 0, status: 'Fetching latest Git release info…' });
            downloadUrl = await fetchLatestGitWindowsUrl();
            installerPath = path.join(tempDir, 'git-installer.exe');

            win.webContents.send('installation-progress', { percent: 5, status: 'Starting Git download…' });

            const writer = fs.createWriteStream(installerPath);
            const response = await axios({
                method: 'GET',
                url: downloadUrl,
                responseType: 'stream',
            });

            const totalLength = response.headers['content-length'];
            let downloaded = 0;

            response.data.on('data', (chunk) => {
                downloaded += chunk.length;
                const progress = Math.floor((downloaded / totalLength) * 100);
                win.webContents.send('installation-progress', {
                    percent: Math.floor(progress * 0.4),
                    status: `Downloading Git: ${progress}%`
                });
            });

            response.data.pipe(writer);
            await new Promise((resolve, reject) => {
                writer.on('finish', resolve);
                writer.on('error', reject);
            });

            win.webContents.send('installation-progress', {
                percent: 50,
                status: 'Installing Git...'
            });

            await executeCommand(`"${installerPath}" /VERYSILENT /NORESTART /NOCANCEL /SP- /CLOSEAPPLICATIONS /RESTARTAPPLICATIONS`);

        } else if (platform === 'darwin') {
            win.webContents.send('installation-progress', {
                percent: 0,
                status: 'Checking Homebrew installation...'
            });

            try {
                await executeCommand('brew --version');

                win.webContents.send('installation-progress', {
                    percent: 20,
                    status: 'Updating Homebrew...'
                });

                await executeCommand('brew update');

                win.webContents.send('installation-progress', {
                    percent: 50,
                    status: 'Installing Git via Homebrew...'
                });

                await executeCommand('brew install git');
            } catch (error) {
                win.webContents.send('installation-progress', {
                    percent: 20,
                    status: 'Installing Homebrew...'
                });

                const brewInstallCommand = '/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"';
                await executeCommand(brewInstallCommand);

                win.webContents.send('installation-progress', {
                    percent: 50,
                    status: 'Installing Git via Homebrew...'
                });

                await executeCommand('brew install git');
            }

        } else if (platform === 'linux') {
            const osRelease = fs.readFileSync('/etc/os-release', 'utf8');
            const isDebian = osRelease.includes('ID=debian') || osRelease.includes('ID=ubuntu');
            const isRedHat = osRelease.includes('ID=rhel') || osRelease.includes('ID=fedora') || osRelease.includes('ID=centos');

            win.webContents.send('installation-progress', {
                percent: 20,
                status: 'Updating package manager...'
            });

            if (isDebian) {
                await executeCommand('sudo apt-get update');

                win.webContents.send('installation-progress', {
                    percent: 50,
                    status: 'Installing Git...'
                });

                await executeCommand('sudo apt-get install -y git');
            } else if (isRedHat) {
                const hasDnf = await executeCommand('which dnf').catch(() => false);

                win.webContents.send('installation-progress', {
                    percent: 50,
                    status: 'Installing Git...'
                });

                if (hasDnf) {
                    await executeCommand('sudo dnf install -y git');
                } else {
                    await executeCommand('sudo yum install -y git');
                }
            } else {
                throw new Error('Unsupported Linux distribution');
            }
        }

        win.webContents.send('installation-progress', {
            percent: 90,
            status: 'Verifying installation...'
        });

        const version = await executeCommand('git --version');

        win.webContents.send('installation-progress', {
            percent: 100,
            status: `Git ${version} installed successfully`
        });

    } catch (error) {
        win.webContents.send('installation-error', error.message);
        throw error;
    } finally {
        if (installerPath && fs.existsSync(installerPath)) {
            fs.unlinkSync(installerPath);
        }
    }
}

function executeCommand(command) {
    return new Promise((resolve, reject) => {
        exec(command, (error, stdout, stderr) => {
            if (error) {
                reject(new Error(`Command failed: ${error.message}\n${stderr}`));
            } else {
                resolve(stdout.trim());
            }
        });
    });
}

module.exports = { downloadAndInstallGit };
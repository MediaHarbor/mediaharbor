const { app, dialog, shell } = require('electron');
const https = require('https');
const fs = require('fs');
const path = require('path');
const { exec } = require('child_process');

module.exports = class UpdateChecker {
    constructor(owner, repo, currentVersion) {
        this.owner = owner;
        this.repo = repo;
        this.currentVersion = currentVersion;
        this.apiUrl = `https://api.github.com/repos/${owner}/${repo}/releases`;
    }

    async checkForUpdates(autoUpdate = false) {
        try {
            const latestRelease = await this.getLatestRelease();

            if (!latestRelease) {
                console.log('No releases found');
                return;
            }

            const latestVersion = latestRelease.tag_name.replace('v', '');
            const currentVersion = this.currentVersion.replace('v', '');

            if (this.compareVersions(latestVersion, currentVersion) > 0) {
                if (autoUpdate) {
                    console.log('Auto-update enabled. Downloading and installing update...');
                    await this.downloadAndInstall(latestRelease);
                } else {
                    await this.showUpdateDialog(latestRelease);
                }
            } else {
                console.log('Application is up to date');
            }
        } catch (error) {
            console.error('Error checking for updates:', error);
        }
    }

    getLatestRelease() {
        return new Promise((resolve, reject) => {
            const options = {
                headers: {
                    'User-Agent': 'electron-app'
                }
            };

            https.get(this.apiUrl, options, (res) => {
                let data = '';

                res.on('data', (chunk) => {
                    data += chunk;
                });

                res.on('end', () => {
                    try {
                        const releases = JSON.parse(data);
                        const stableRelease = releases.find(release => !release.prerelease);
                        resolve(stableRelease);
                    } catch (error) {
                        reject(error);
                    }
                });
            }).on('error', reject);
        });
    }

    compareVersions(v1, v2) {
        const v1Parts = v1.split('.').map(Number);
        const v2Parts = v2.split('.').map(Number);

        for (let i = 0; i < Math.max(v1Parts.length, v2Parts.length); i++) {
            const v1Part = v1Parts[i] || 0;
            const v2Part = v2Parts[i] || 0;
            if (v1Part > v2Part) return 1;
            if (v1Part < v2Part) return -1;
        }
        return 0;
    }

    async showUpdateDialog(release) {
        const response = await dialog.showMessageBox({
            type: 'info',
            title: 'Update Available',
            message: `A new version (${release.tag_name}) is available!`,
            detail: `Release notes:\n${release.body || 'No release notes available.'}\n\nWould you like to download it?`,
            buttons: ['Download', 'Later'],
            defaultId: 0
        });

        if (response.response === 0) {
            await shell.openExternal(release.html_url);
        }
    }

    async downloadAndInstall(release) {
        const asset = release.assets.find(a => a.name.endsWith('.exe') || a.name.endsWith('.dmg') || a.name.endsWith('.AppImage'));

        if (!asset) {
            console.error('No compatible update file found.');
            return;
        }

        const downloadPath = path.join(app.getPath('temp'), asset.name);

        const file = fs.createWriteStream(downloadPath);
        https.get(asset.browser_download_url, (response) => {
            response.pipe(file);

            file.on('finish', () => {
                file.close(() => {
                    console.log('Update downloaded. Installing now...');
                    if (process.platform === 'darwin' || process.platform === 'linux') {
                        exec(`open "${downloadPath}"`);
                    } else if (process.platform === 'win32') {
                        exec(`start "" "${downloadPath}"`);
                    }
                    app.quit();
                });
            });
        }).on('error', (err) => {
            fs.unlink(downloadPath, () => console.error('Download failed:', err));
        });
    }
}

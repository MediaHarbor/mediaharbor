const fs = require('fs');
const path = require('path');
const os = require('os');
const { exec, spawn } = require('child_process');

function getVenvDir() {
    return path.join(os.homedir(), '.mediaharbor', 'venv');
}

function getVenvPython() {
    const venvDir = getVenvDir();
    if (os.platform() === 'win32') {
        return path.join(venvDir, 'Scripts', 'python.exe');
    }
    return path.join(venvDir, 'bin', 'python');
}

function getVenvPip() {
    return [getVenvPython(), '-m', 'pip'];
}

function isVenvReady() {
    try {
        return fs.existsSync(getVenvPython());
    } catch {
        return false;
    }
}

function findSystemPython() {
    const candidates = os.platform() === 'win32'
        ? ['py', 'python3', 'python']
        : ['python3', 'python'];

    return new Promise((resolve, reject) => {
        let i = 0;
        function tryNext() {
            if (i >= candidates.length) {
                reject(new Error('No suitable Python 3 installation found. Please install Python 3.10 or newer.'));
                return;
            }
            const cmd = candidates[i++];
            exec(`${cmd} --version`, (err, stdout, stderr) => {
                if (err) { tryNext(); return; }
                const output = (stdout || stderr).trim();
                const match = output.match(/Python (\d+)\.(\d+)/);
                if (match && parseInt(match[1]) === 3 && parseInt(match[2]) >= 10) {
                    resolve(cmd);
                } else {
                    tryNext();
                }
            });
        }
        tryNext();
    });
}

function ensureVenv(win) {
    if (isVenvReady()) return Promise.resolve();

    const sendProgress = (percent, status) => {
        if (win && !win.isDestroyed()) {
            win.webContents.send('installation-progress', JSON.stringify({
                dependency: 'python',
                percent,
                status
            }));
        }
    };

    return findSystemPython().then(systemPython => {
        const venvDir = getVenvDir();
        fs.mkdirSync(path.dirname(venvDir), { recursive: true });

        sendProgress(5, 'Creating MediaHarbor Python environment...');

        return new Promise((resolve, reject) => {
            const proc = spawn(systemPython, ['-m', 'venv', venvDir], { stdio: ['ignore', 'pipe', 'pipe'] });

            proc.on('close', code => {
                if (code === 0 && isVenvReady()) {
                    sendProgress(15, 'Python environment created');
                    resolve();
                } else {
                    reject(new Error(`Failed to create Python virtual environment (exit code ${code})`));
                }
            });

            proc.on('error', err => reject(new Error(`Failed to create virtual environment: ${err.message}`)));
        });
    });
}

function getVenvBin(name) {
    const venvDir = getVenvDir();
    const binName = os.platform() === 'win32' ? name + '.exe' : name;
    const dir = os.platform() === 'win32' ? 'Scripts' : 'bin';
    return path.join(venvDir, dir, binName);
}

function resolveCommand(name, fallbackPaths = []) {
    const venvBin = getVenvBin(name);
    if (fs.existsSync(venvBin)) return venvBin;
    for (const p of fallbackPaths) {
        if (fs.existsSync(p)) return p;
    }
    return name;
}

module.exports = { getVenvDir, getVenvPython, getVenvPip, isVenvReady, ensureVenv, getVenvBin, resolveCommand };

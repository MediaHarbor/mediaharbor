const { shell, app} = require('electron');
const { exec } = require('child_process');
const { spawn } = require('child_process');
const path = require("path");
function getResourcePath(filename) {
    if (app.isPackaged) {
        return path.join(process.resourcesPath, 'app.asar.unpacked', 'src', filename)
            .replace(/\\/g, '/');
    }
    return path.join(__dirname, filename).replace(/\\/g, '/');
}


function getPythonCommand() {
    switch (process.platform) {
        case 'win32':
            return 'py';
        case 'darwin':
        case 'linux':
            // fall back to python if python3 not found
            return new Promise((resolve) => {
                exec('python3 --version', (error) => {
                    if (error) {
                        resolve('python');
                    } else {
                        resolve('python3');
                    }
                });
            });
        default:
            return 'python';
    }
}

module.exports = {getPythonCommand};
const fs = require('fs');
const path = require('path');

const MAX_LOG_SIZE = 5 * 1024 * 1024; // 5MB

let logFilePath = null;
let mainWindow = null;

function init(userDataPath, win) {
  logFilePath = path.join(userDataPath, 'mediaharbor.log');
  mainWindow = win;
  rotateIfNeeded();
  info('system', 'Logger initialized');
}

function setWindow(win) {
  mainWindow = win;
}

function rotateIfNeeded() {
  if (!logFilePath) return;
  try {
    const stats = fs.statSync(logFilePath);
    if (stats.size > MAX_LOG_SIZE) {
      const content = fs.readFileSync(logFilePath, 'utf8');
      const lines = content.split('\n');
      const half = lines.slice(Math.floor(lines.length / 2));
      fs.writeFileSync(logFilePath, half.join('\n'), 'utf8');
    }
  } catch {}
}

function write(level, source, message) {
  const timestamp = new Date().toISOString();
  const line = `[${timestamp}] [${level.toUpperCase()}] [${source}] ${message}\n`;

  if (logFilePath) {
    try {
      fs.appendFileSync(logFilePath, line, 'utf8');
    } catch {}
  }

  try {
    if (mainWindow && !mainWindow.isDestroyed() && mainWindow.webContents && !mainWindow.webContents.isDestroyed()) {
      mainWindow.webContents.send('backend-log', {
        level,
        source,
        title: `[${source}] ${message.slice(0, 120)}`,
        message,
        timestamp,
      });
    }
  } catch {}
}

function info(source, message) {
  write('info', source, message);
}

function warn(source, message) {
  write('warn', source, message);
}

function error(source, message) {
  write('error', source, message);
}

module.exports = { init, setWindow, info, warn, error };

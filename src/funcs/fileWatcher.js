const chokidar = require('chokidar');
const path = require('path');
const { VIDEO_FORMATS, MUSIC_FORMATS } = require('./mediaScanner');
const logger = require('./logger');

const ALL_EXTENSIONS = new Set([...VIDEO_FORMATS, ...MUSIC_FORMATS]);

class FileWatcherService {
  constructor(mediaScanner) {
    this.mediaScanner = mediaScanner;
    this.watcher = null;
    this.directory = null;
    this.mainWindow = null;
    this.debounceTimer = null;
    this.pendingAdded = new Set();
    this.pendingRemoved = new Set();
    this.isProcessing = false;
    this.isReady = false;
  }

  setMainWindow(win) {
    this.mainWindow = win;
  }

  isMediaFile(filePath) {
    const ext = path.extname(filePath).toLowerCase();
    return ALL_EXTENSIONS.has(ext);
  }

  start(directory) {
    if (this.watcher) this.stop();
    this.directory = directory;
    this.isReady = false;

    logger.info('filewatcher', `Starting watcher on: ${directory}`);

    this.watcher = chokidar.watch(directory, {
      persistent: true,
      ignoreInitial: true,
      depth: 10,
      awaitWriteFinish: {
        stabilityThreshold: 2000,
        pollInterval: 200,
      },
      ignored: [
        /(^|[\/\\])\./,
        /node_modules/,
        /\.tmp$/,
        /\.part$/,
        /\.crdownload$/,
      ],
    });

    this.watcher
      .on('ready', () => {
        this.isReady = true;
        logger.info('filewatcher', 'Ready and watching for changes');
      })
      .on('add', (filePath) => {
        if (this.isMediaFile(filePath)) {
          logger.info('filewatcher', `File added: ${path.basename(filePath)}`);
          this.pendingAdded.add(filePath);
          this.pendingRemoved.delete(filePath);
          this.scheduleFlush();
        }
      })
      .on('change', (filePath) => {
        if (this.isMediaFile(filePath)) {
          logger.info('filewatcher', `File changed: ${path.basename(filePath)}`);
          this.pendingAdded.add(filePath);
          this.scheduleFlush();
        }
      })
      .on('unlink', (filePath) => {
        if (this.isMediaFile(filePath)) {
          logger.info('filewatcher', `File removed: ${path.basename(filePath)}`);
          this.pendingRemoved.add(filePath);
          this.pendingAdded.delete(filePath);
          this.scheduleFlush();
        }
      })
      .on('error', (err) => {
        logger.error('filewatcher', `Watcher error: ${err?.message || err}`);
      });
  }

  stop() {
    if (this.watcher) {
      this.watcher.close();
      this.watcher = null;
    }
    clearTimeout(this.debounceTimer);
    this.pendingAdded.clear();
    this.pendingRemoved.clear();
    this.directory = null;
    this.isReady = false;
  }

  changeDirectory(newDirectory) {
    logger.info('filewatcher', `Changing directory to: ${newDirectory}`);
    this.stop();
    this.start(newDirectory);
  }

  scheduleFlush() {
    clearTimeout(this.debounceTimer);
    this.debounceTimer = setTimeout(() => this.flush(), 800);
  }

  async flush() {
    if (this.isProcessing) {
      this.scheduleFlush();
      return;
    }

    const added = [...this.pendingAdded];
    const removed = [...this.pendingRemoved];
    this.pendingAdded.clear();
    this.pendingRemoved.clear();

    if (added.length === 0 && removed.length === 0) return;

    this.isProcessing = true;
    logger.info('filewatcher', `Flushing: ${added.length} added, ${removed.length} removed`);

    try {
      let newItems = [];

      if (added.length > 0) {
        const files = [];
        const fs = require('fs/promises');
        for (const filePath of added) {
          try {
            const stat = await fs.stat(filePath);
            files.push({ path: filePath, size: stat.size, mtime: stat.mtime });
          } catch (err) {
            logger.warn('filewatcher', `Cannot stat file (may have been removed): ${path.basename(filePath)}`);
          }
        }

        if (files.length > 0) {
          logger.info('filewatcher', `Processing ${files.length} files...`);
          const processed = await this.mediaScanner.processFilesParallel(files, () => {});
          newItems = this.mediaScanner.organizeMediaItems(processed);
          logger.info('filewatcher', `Processed into ${newItems.length} items`);

          if (this.directory) {
            this.mediaScanner.invalidateCache(this.directory);
          }
        }
      }

      if (removed.length > 0 && this.directory) {
        this.mediaScanner.invalidateCache(this.directory);
      }

      this.notify({ added: newItems, removedPaths: removed });
    } catch (err) {
      logger.error('filewatcher', `Flush error: ${err?.message || err}`);
    } finally {
      this.isProcessing = false;
    }
  }

  notify(data) {
    if (!this.mainWindow || this.mainWindow.isDestroyed()) {
      logger.warn('filewatcher', 'Cannot notify: mainWindow unavailable');
      return;
    }
    try {
      this.mainWindow.webContents.send('library:filesChanged', data);
      logger.info('filewatcher', `Notified renderer: ${data.added?.length || 0} added, ${data.removedPaths?.length || 0} removed`);
    } catch (err) {
      logger.error('filewatcher', `Notify error: ${err?.message || err}`);
    }
  }
}

module.exports = { FileWatcherService };

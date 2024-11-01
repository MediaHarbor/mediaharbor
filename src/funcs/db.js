const fs = require("fs");
const path = require("path");
const { getDefaultSettings } = require('./defaults');
const {app} = require("electron");
const sqlite3 = require('sqlite3').verbose();
const settingsFilePath = path.join(app.getPath('userData'), 'mh-settings.json');
function loadSettings() {
    try {
        const settingsData = fs.readFileSync(settingsFilePath, 'utf8');
        return JSON.parse(settingsData);
    } catch (err) {
        console.log('No user settings found, using default settings.');
        return getDefaultSettings(); // Fall back to default settings
    }
}
const userSettings = loadSettings();
const dbPath = userSettings.downloadsDatabasePath || path.join(app.getPath('userData'), 'downloads_database.db');

let db = new sqlite3.Database(dbPath, (err) => {
    if (err) {
        console.error('Error opening the database:', err.message);
    } else {
        console.log('Connected to the SQLite database.');
        // Create the table if it doesn't exist
        db.run(`CREATE TABLE IF NOT EXISTS downloads (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            downloadName TEXT,
            downloadArtistOrUploader TEXT,
            downloadLocation TEXT,
            downloadThumbnail TEXT
        )`);
    }
});


function saveDownloadToDatabase(downloadInfo) {
    const sql = `INSERT INTO downloads (downloadName, downloadArtistOrUploader, downloadLocation, downloadThumbnail) 
                 VALUES (?, ?, ?, ?)`;

    db.run(sql, [downloadInfo.downloadName, downloadInfo.downloadArtistOrUploader, downloadInfo.downloadLocation, downloadInfo.downloadThumbnail], function (err) {
        if (err) {
            return console.error('Error saving download to database:', err.message);
        }
        console.log(`A row has been inserted with rowid ${this.lastID}`);
    });
}

function loadDownloadsFromDatabase(callback) {
    const sql = `SELECT * FROM downloads`;

    db.all(sql, [], (err, rows) => {
        if (err) {
            console.error('Error loading downloads from database:', err.message);
            callback([]);  // Return empty array on error
            return;
        }
        callback(rows);
    });
}
function deleteFromDatabase(event, id) {
    return new Promise((resolve, reject) => {
        const sql = 'DELETE FROM downloads WHERE id = ?';
        db.run(sql, [id], (err) => {
            if (err) {
                console.error('Error deleting download:', err);
                reject(err);
                return;
            }
            resolve();
        });
    });
}
async function closeDatabase() {
    return new Promise((resolve, reject) => {
        db.close((err) => {
            if (err) {
                console.error('Error closing the database:', err.message);
                reject(err);
            } else {
                console.log('Database connection closed.');
                resolve();
            }
        });
    });
}

async function reconnectDatabase() {
    return new Promise((resolve, reject) => {
        db = new sqlite3.Database(userSettings.downloadsDatabasePath, (err) => {
            if (err) {
                console.error('Error reopening the database:', err.message);
                reject(err);
            } else {
                console.log('Reconnected to the SQLite database.');

                // Ensure the table is recreated
                db.run(`CREATE TABLE IF NOT EXISTS downloads (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    downloadName TEXT,
                    downloadArtistOrUploader TEXT,
                    downloadLocation TEXT,
                    downloadThumbnail TEXT
                )`, (err) => {
                    if (err) {
                        console.error('Error creating downloads table:', err.message);
                        reject(err);
                    } else {
                        console.log('Downloads table created (if it did not exist).');
                        resolve();
                    }
                });
            }
        });
    });
}


async function deleteDataBase() {
    console.log(userSettings.downloadsDatabasePath);
    try {
        // Close the database connection before deleting the file
        await closeDatabase();

        if (fs.existsSync(userSettings.downloadsDatabasePath)) {
            // Delete the database file
            fs.unlinkSync(userSettings.downloadsDatabasePath);
            console.log('Database file deleted successfully.');

            // Recreate an empty database and table
            await reconnectDatabase();
            console.log('Database reconnected and downloads table recreated.');

            return { success: true };
        } else {
            return { success: false, message: 'File not found' };
        }
    } catch (error) {
        console.error('Error during database deletion or reconnection:', error.message);
        return { success: false, message: error.message };
    }
}



module.exports = { saveDownloadToDatabase, loadDownloadsFromDatabase, deleteFromDatabase, deleteDataBase };
const fs = require("fs");
const TOML = require("@iarna/toml");
const path = require("path");
const { app } = require("electron");
const { spawn } = require("child_process");
const { getDefaultSettings } = require("./defaults");

const settingsFilePath = path.join(app.getPath('userData'), 'mh-settings.json');
const spotifyConfigPath = path.join(app.getPath('userData'), 'spotify_config.json');
const appleConfigPath = path.join(app.getPath('userData'), 'apple_config.json');

const mergeServiceSettings = (defaultSettings, serviceConfig, servicePrefix) => {
    return Object.entries(defaultSettings)
        .filter(([key]) => key.startsWith(servicePrefix))
        .reduce((obj, [key, defaultValue]) => {
            const configKey = key.replace(servicePrefix + '_', '');
            obj[key] = serviceConfig[configKey] || defaultValue;
            return obj;
        }, {});
};

// Load initial settings
const userSettings = loadTheSettings();

function loadTheSettings() {
    try {
        const settingsData = fs.readFileSync(settingsFilePath, 'utf8');
        return JSON.parse(settingsData);
    } catch (err) {
        console.log('No user settings found, using default settings.');
        return getDefaultSettings();
    }
}
async function saveServiceConfig(configPath, settings, servicePrefix) {
    // Extract service-specific settings from the main settings object
    const serviceSettings = Object.entries(settings)
        .filter(([key]) => key.startsWith(servicePrefix))
        .reduce((obj, [key, value]) => {
            const configKey = key.replace(servicePrefix + '_', '');
            obj[configKey] = value;
            return obj;
        }, {});

    try {
        await fs.promises.writeFile(
            configPath,
            JSON.stringify(serviceSettings, null, 4),
            'utf8'
        );
    } catch (err) {
        console.error(`Error saving service config to ${configPath}:`, err);
        throw err;
    }
}

async function loadServiceConfig(configPath) {
    try {
        const data = await fs.promises.readFile(configPath, 'utf8');
        return JSON.parse(data);
    } catch (err) {
        console.log(`No config found at ${configPath}, will be created on next save`);
        return {};
    }
}

function getStreamripPaths(callback) {
    const streamripProcess = spawn('custom_rip', ['config', 'path']);
    let stdout = '';
    let stderr = '';

    streamripProcess.stdout.on('data', (data) => {
        stdout += data.toString();
    });

    streamripProcess.stderr.on('data', (data) => {
        stderr += data.toString();
    });

    streamripProcess.on('close', (code) => {
        if (code !== 0) {
            console.error(`Error getting custom_rip config path: ${stderr}`);
            callback(null);
            return;
        }

        const configPathMatch = stdout.match(/Config path: '(.+)'/);
        if (configPathMatch && configPathMatch[1]) {
            const configPath = configPathMatch[1];
            const configDir = path.dirname(configPath);

            const paths = {
                configPath: configPath,
                downloadsDbPath: path.join(configDir, 'downloads.db'),
                failedDownloadsDbPath: path.join(configDir, 'failed_downloads.db')
            };

            callback(paths);
        } else {
            console.error('Could not find config path in output:', stdout);
            callback(null);
        }
    });
}

async function saveSettings(event, settings) {
    try {
        await fs.promises.writeFile(settingsFilePath, JSON.stringify(settings));
    } catch (err) {
        console.error('Error saving settings:', err);
        event.reply('settings-error', 'Failed to save application settings');
        return;
    }
    await Promise.all([
        saveServiceConfig(spotifyConfigPath, settings, 'spotify'),
        saveServiceConfig(appleConfigPath, settings, 'apple')
    ]);
    // Get streamrip config path
    getStreamripPaths(async (paths) => {
        if (!paths?.configPath) {
            console.error('Could not get streamrip config path');
            event.reply('settings-error', 'Could not locate streamrip configuration');
            return;
        }

        try {
            const data = await fs.promises.readFile(paths.configPath, 'utf8');
            let config;

            try {
                config = TOML.parse(data);
            } catch (parseErr) {
                console.error('Error parsing existing TOML:', parseErr);
                config = {};
            }

            // Update config object with new settings
            config.downloads = {
                ...config.downloads,
                folder: settings.downloadLocation,
                source_subdirectories: settings.createPlatformSubfolders,
                disc_subdirectories: settings.disc_subdirectories,
                concurrency: settings.concurrency,
                max_connections: parseInt(settings.max_connections),
                requests_per_minute: parseInt(settings.requests_per_minute)
            };

            // Update qobuz section
            config.qobuz = {
                ...config.qobuz,
                download_booklets: settings.qobuz_download_booklets,
                use_auth_token: settings.qobuz_token_or_email,
                email_or_userid: settings.qobuz_email_or_userid,
                password_or_token: settings.qobuz_password_or_token,
                app_id: settings.qobuz_app_id,
                secrets: Array.isArray(settings.qobuz_secrets)
                    ? settings.qobuz_secrets
                    : settings.qobuz_secrets.trim() === ""
                        ? [] // Ensures an empty array when input is empty
                        : settings.qobuz_secrets
                            .split(/[\s,]+/) // Split by commas and/or whitespace
                            .map(secret => secret.trim()), // Trim each secret to remove extra spaces

            };
            // Update tidal section
            config.tidal = {
                ...config.tidal,
                download_videos: settings.tidal_download_videos
            }
            // Update deezer section
            config.deezer = {
                ...config.deezer,
                use_deezloader: settings.deezer_use_deezloader,
                arl: settings.deezer_arl
            }
            // Update database section
            config.database = {
                ...config.database,
                downloads_enabled: settings.downloads_database_check,
                downloads_path: settings.downloads_database_path,
                failed_downloads_enabled: settings.failed_downloads_database_check,
                failed_downloads_path: settings.failed_downloads_database
            };

            // Update conversion section
            config.conversion = {
                ...config.conversion,
                enabled: settings.conversion_check,
                codec: settings.conversion_codec,
                sampling_rate: parseInt(settings.conversion_sampling_rate),
                bit_depth: parseInt(settings.conversion_bit_depth || 16)
            };
            config.metadata = {
                ...config.metadata,
                set_playlist_to_album: settings.meta_album_name_playlist_check,
                renumber_playlist_tracks: settings.meta_album_order_playlist_check,
                exclude: Array.isArray(settings.excluded_tags)
                    ? settings.excluded_tags
                    : settings.excluded_tags.trim() === ""
                        ? [] // Ensures an empty array when input is empty
                        : settings.excluded_tags.split(/\s+/)
            }

            const tomlString = TOML.stringify(config);

            try {
                await fs.promises.writeFile(paths.configPath, tomlString, 'utf8');
                event.reply('settings-saved', 'Settings saved successfully');
            } catch (writeErr) {
                console.error('Error writing streamrip config:', writeErr);
                event.reply('settings-error', 'Failed to save streamrip configuration');
            }
        } catch (readErr) {
            console.error('Error reading streamrip config:', readErr);
            event.reply('settings-error', 'Failed to read existing streamrip configuration');
        }
    });
}

function loadSettings(event) {
    fs.readFile(settingsFilePath, 'utf8', async (err, data) => {
        let settings;

        if (err) {
            console.log('Error loading settings, using defaults:', err);
            settings = getDefaultSettings();
        } else {
            try {
                settings = JSON.parse(data);
            } catch (parseErr) {
                console.log('Error parsing settings JSON, using defaults:', parseErr);
                settings = getDefaultSettings();
            }
        }

        try {
            const [spotifyConfig, appleConfig] = await Promise.all([
                loadServiceConfig(spotifyConfigPath),
                loadServiceConfig(appleConfigPath)
            ]);

            // Merge service defaults with configs
            const defaultSettings = getDefaultSettings();
            settings = {
                ...settings,
                ...mergeServiceSettings(defaultSettings, spotifyConfig, 'spotify'),
                ...mergeServiceSettings(defaultSettings, appleConfig, 'apple')
            };

        } catch (err) {
            console.error('Error loading service configs:', err);
        }

        getStreamripPaths((paths) => {
            if (paths) {
                fs.readFile(paths.configPath, 'utf8', (tomlErr, tomlData) => {
                    if (!tomlErr) {
                        try {
                            const streamripConfig = TOML.parse(tomlData);

                            // Update settings with streamrip config values
                            settings = {
                                ...settings,
                                downloads_database_path: paths.downloadsDbPath,
                                failed_downloads_database_path: paths.failedDownloadsDbPath,
                                downloadLocation: streamripConfig.downloads?.folder || settings.downloadLocation,
                                source_subdirectories: streamripConfig.downloads?.source_subdirectories || false,
                                disc_subdirectories: streamripConfig.downloads?.disc_subdirectories || true,
                                max_connections: streamripConfig.downloads?.max_connections || 6,
                                concurrency: streamripConfig.downloads?.concurrency || true,
                                qobuz_download_booklets: streamripConfig.qobuz?.download_booklets || true,
                                qobuz_token_or_email: streamripConfig.qobuz?.use_auth_token || true,
                                qobuz_email_or_userid: streamripConfig.qobuz?.email_or_userid || "",
                                qobuz_password_or_token: streamripConfig.qobuz?.password_or_token || "",
                                qobuz_app_id: streamripConfig.qobuz?.app_id || "",
                                qobuz_secrets: streamripConfig.qobuz?.secrets,
                                tidal_download_videos: streamripConfig.tidal?.download_videos,
                                deezer_use_deezloader: streamripConfig.deezer?.use_deezloader,
                                deezer_arl: streamripConfig.deezer?.arl || '',
                                downloads_database_check: streamripConfig.database?.downloads_enabled,
                                failed_downloads_database_check: streamripConfig.database?.failed_downloads_enabled,
                                downloads_database: streamripConfig.database?.downloads_path,
                                failed_downloads_database: streamripConfig.database?.failed_downloads_path,
                                conversion_check: streamripConfig.conversion?.enabled,
                                conversion_codec: streamripConfig.conversion?.codec || "ALAC",
                                conversion_sampling_rate: streamripConfig.conversion?.sampling_rate || 48000,
                                meta_album_name_playlist_check: streamripConfig.metadata?.set_playlist_to_album,
                                meta_album_order_playlist_check: streamripConfig.metadata?.renumber_playlist_tracks,
                                excluded_tags: streamripConfig.metadata?.exclude || [],
                            };

                            fs.writeFile(settingsFilePath, JSON.stringify(settings), (writeErr) => {
                                if (writeErr) console.error('Error saving merged settings:', writeErr);
                            });
                        } catch (tomlParseErr) {
                            console.error('Error parsing streamrip config:', tomlParseErr);
                        }
                    }
                    event.reply('settings-data', settings);
                });
            } else {
                event.reply('settings-data', settings);
            }
        });
    });
}

// Setup IPC handlers
function setupSettingsHandlers(ipcMain) {
    ipcMain.on('load-settings', (event) => {
        loadSettings(event);
    });

    ipcMain.on('save-settings', async (event, settings) => {
        await saveSettings(event, settings);
    });
}

module.exports = {
    setupSettingsHandlers,
    loadSettings,
    saveSettings
};
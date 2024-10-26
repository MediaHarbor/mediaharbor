const fs = require("fs");
const TOML = require("@iarna/toml");
const path = require("path");
const { app } = require("electron");
const { spawn } = require("child_process");
const { getDefaultSettings } = require("./defaults");

const settingsFilePath = path.join(app.getPath('userData'), 'mh-settings.json');
const spotifyConfigPath = path.join(app.getPath('userData'), 'spotify_config.json');
const appleConfigPath = path.join(app.getPath('userData'), 'apple_config.json');

// Updated merge function to preserve all settings
const mergeServiceSettings = (settings, serviceConfig, servicePrefix) => {
    const merged = { ...settings };

    // First, apply any service-specific settings from the config
    Object.entries(serviceConfig).forEach(([key, value]) => {
        const settingKey = `${servicePrefix}_${key}`;
        merged[settingKey] = value;
    });

    return merged;
};

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
    const serviceSettings = Object.entries(settings)
        .filter(([key]) => key.startsWith(servicePrefix))
        .reduce((obj, [key, value]) => {
            const configKey = key.replace(servicePrefix + '_', '');
            if (value !== null && value !== undefined && value !== '') {
                obj[configKey] = value;
            }
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

async function saveSettings(event, settings) {
    // First, save the main settings file
    try {
        await fs.promises.writeFile(
            settingsFilePath,
            JSON.stringify(settings, null, 4)
        );
    } catch (err) {
        console.error('Error saving main settings:', err);
        event.reply('settings-error', 'Failed to save application settings');
        return;
    }

    // Update output paths
    let spotifyOutputPath, appleOutputPath;
    if (settings.createPlatformSubfolders) {
        spotifyOutputPath = path.join(settings.downloadLocation, "Spotify");
        appleOutputPath = path.join(settings.downloadLocation, "Apple Music");
    } else {
        spotifyOutputPath = settings.downloadLocation;
        appleOutputPath = settings.downloadLocation;
    }

    settings.spotify_output_path = spotifyOutputPath;
    settings.apple_output_path = appleOutputPath;

    // Save service configs
    try {
        await Promise.all([
            saveServiceConfig(spotifyConfigPath, settings, 'spotify'),
            saveServiceConfig(appleConfigPath, settings, 'apple')
        ]);
    } catch (err) {
        console.error('Error saving service configs:', err);
        event.reply('settings-error', 'Failed to save service configurations');
        return;
    }

    // Handle streamrip config
    getStreamripPaths(async (paths) => {
        if (!paths?.configPath) {
            console.error('Could not get streamrip config path');
            event.reply('settings-error', 'Could not locate streamrip configuration');
            return;
        }

        try {
            const data = await fs.promises.readFile(paths.configPath, 'utf8');
            let config = {};

            try {
                config = TOML.parse(data);
            } catch (parseErr) {
                console.error('Error parsing existing TOML:', parseErr);
            }

            // Update streamrip config
            config.downloads = {
                folder: settings.downloadLocation,
                source_subdirectories: settings.createPlatformSubfolders,
                disc_subdirectories: settings.disc_subdirectories,
                concurrency: settings.concurrency,
                max_connections: parseInt(settings.max_connections),
                requests_per_minute: parseInt(settings.requests_per_minute)
            };

            // Other config sections remain the same...
            config.qobuz = {
                download_booklets: settings.qobuz_download_booklets,
                use_auth_token: settings.qobuz_token_or_email,
                email_or_userid: settings.qobuz_email_or_userid,
                password_or_token: settings.qobuz_password_or_token,
                app_id: settings.qobuz_app_id,
                secrets: Array.isArray(settings.qobuz_secrets)
                    ? settings.qobuz_secrets
                    : settings.qobuz_secrets.trim() === ""
                        ? []
                        : settings.qobuz_secrets.split(/[\s,]+/).map(secret => secret.trim())
            };

            config.tidal = {
                download_videos: settings.tidal_download_videos
            };

            config.deezer = {
                use_deezloader: settings.deezer_use_deezloader,
                arl: settings.deezer_arl
            };

            config.database = {
                downloads_enabled: settings.downloads_database_check,
                downloads_path: settings.downloads_database_path,
                failed_downloads_enabled: settings.failed_downloads_database_check,
                failed_downloads_path: settings.failed_downloads_database
            };

            config.conversion = {
                enabled: settings.conversion_check,
                codec: settings.conversion_codec,
                sampling_rate: parseInt(settings.conversion_sampling_rate),
                bit_depth: parseInt(settings.conversion_bit_depth || 16)
            };

            config.metadata = {
                set_playlist_to_album: settings.meta_album_name_playlist_check,
                renumber_playlist_tracks: settings.meta_album_order_playlist_check,
                exclude: Array.isArray(settings.excluded_tags)
                    ? settings.excluded_tags
                    : settings.excluded_tags.trim() === ""
                        ? []
                        : settings.excluded_tags.split(/\s+/)
            };

            const tomlString = TOML.stringify(config);
            await fs.promises.writeFile(paths.configPath, tomlString, 'utf8');
            event.reply('settings-saved', 'Settings saved successfully');
        } catch (err) {
            console.error('Error handling streamrip config:', err);
            event.reply('settings-error', 'Failed to update streamrip configuration');
        }
    });
}

function loadSettings(event) {
    fs.readFile(settingsFilePath, 'utf8', async (err, data) => {
        let settings;

        try {
            if (err) {
                console.log('Using default settings');
                settings = getDefaultSettings();
            } else {
                settings = JSON.parse(data);
            }

            // Load service configs
            const [spotifyConfig, appleConfig] = await Promise.all([
                loadServiceConfig(spotifyConfigPath),
                loadServiceConfig(appleConfigPath)
            ]);

            // Merge all settings together
            settings = mergeServiceSettings(settings, spotifyConfig, 'spotify');
            settings = mergeServiceSettings(settings, appleConfig, 'apple');

            // Load streamrip settings
            getStreamripPaths((paths) => {
                if (paths) {
                    fs.readFile(paths.configPath, 'utf8', (tomlErr, tomlData) => {
                        if (!tomlErr) {
                            try {
                                const streamripConfig = TOML.parse(tomlData);

                                // Merge streamrip settings
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
                            } catch (tomlParseErr) {
                                console.error('Error parsing streamrip config:', tomlParseErr);
                            }
                        }

                        // Save the complete merged settings
                        fs.writeFile(settingsFilePath, JSON.stringify(settings, null, 4), (writeErr) => {
                            if (writeErr) console.error('Error saving merged settings:', writeErr);
                        });

                        event.reply('settings-data', settings);
                    });
                } else {
                    event.reply('settings-data', settings);
                }
            });
        } catch (err) {
            console.error('Error in loadSettings:', err);
            event.reply('settings-error', 'Failed to load settings');
            event.reply('settings-data', getDefaultSettings());
        }
    });
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
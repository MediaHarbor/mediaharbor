const fs = require("fs");
const TOML = require("@iarna/toml");
const path = require("path");
const { app } = require("electron");
const { spawn } = require("child_process");
const { getDefaultSettings } = require("./defaults");
const stream = require("node:stream");

const settingsFilePath = path.join(app.getPath('userData'), 'mh-settings.json');
const spotifyConfigPath = path.join(app.getPath('userData'), 'zotify_config.json'); // Renamed, otherwise could make some problems.
const appleConfigPath = path.join(app.getPath('userData'), 'apple_config.json');


const mergeServiceSettings = (settings, serviceConfig, servicePrefix) => {
    const merged = { ...settings };

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
    const defaults = getDefaultSettings();
    const defaultKeys = Object.keys(defaults).filter(k => k.startsWith(servicePrefix + "_"));

    const serviceSettings = Object.entries(settings)
        .filter(([key, value]) =>
            key.startsWith(servicePrefix) &&
            defaultKeys.includes(key) &&
            value !== null && value !== undefined && value !== ''
        )
        .reduce((obj, [key, value]) => {
            obj[key.replace(servicePrefix + '_', '')] = value;
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
    const defaultKeys = Object.keys(getDefaultSettings());
    const cleanedSettings = Object.fromEntries(
        Object.entries(settings).filter(([key]) => defaultKeys.includes(key))
    );
    try {
        await fs.promises.writeFile(
            settingsFilePath,
            JSON.stringify(cleanedSettings, null, 4)
        );
    } catch (err) {
        console.error('Error saving main settings:', err);
        event.reply('settings-error', 'Failed to save application settings');
        return;
    }

    // Update output paths
    let spotifyOutputPath, appleOutputPath;if (settings.createPlatformSubfolders) {
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
            console.warn('custom_rip is not installed or not available, skipping streamrip config update');
            event.reply('settings-saved', 'Settings saved successfully (without streamrip config)');
            return;
        }

        try {
            const normalizedConfigPath = path.normalize(paths.configPath);
            console.log('Attempting to read from normalized path:', normalizedConfigPath);

            const data = await fs.promises.readFile(normalizedConfigPath, 'utf8');
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
                requests_per_minute: parseInt(settings.requests_per_minute),
                verify_ssl: true
            };

            config.qobuz = {
                quality: settings.qobuz_quality,
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
                quality: settings.tidal_quality,
                user_id: settings.tidal_user_id,
                country_code: settings.tidal_country_code,
                access_token: settings.tidal_access_token,
                refresh_token: settings.tidal_refresh_token,
                token_expiry: settings.tidal_token_expiry,
                download_videos: settings.tidal_download_videos
            };

            config.deezer = {
                quality: settings.deezer_quality,
                deezloader_warnings: settings.deezloader_warnings,
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
                bit_depth: parseInt(settings.conversion_bit_depth || 16),
                lossy_bitrate: parseInt(settings.conversion_lossy_bitrate || 320)
            };
            config.qobuz_filters = {
                extras: false,
                repeats: false,
                non_albums: false,
                features: false,
                non_studio_albums: false,
                non_remaster: false
            }

            config.artwork = {
                embed: true,
                embed_size :"large",
                embed_max_width: -1,
                save_artwork: true,
                saved_max_width: -1,
            }

            config.metadata = {
                set_playlist_to_album: settings.meta_album_name_playlist_check,
                renumber_playlist_tracks: settings.meta_album_order_playlist_check,
                exclude: Array.isArray(settings.excluded_tags)
                    ? settings.excluded_tags
                    : settings.excluded_tags.trim() === ""
                        ? []
                        : settings.excluded_tags.split(/\s+/)
            };
            // Config Fixes
            config.filepaths = {
                add_singles_to_folder: settings.filepaths_add_singles_to_folder,
                folder_format: settings.filepaths_folder_format,
                track_format: settings.filepaths_track_format,
                restrict_characters: settings.filepaths_restrict_characters,
                truncate_to: settings.filepaths_truncate_to,
            };
            config.soundcloud = {
                quality: settings.soundcloud_quality,
                client_id: settings.soundcloud_client_id,
                app_version: settings.soundcloud_app_version,

            };
            config.youtube = {
                quality: settings.youtube_quality,
                download_videos: settings.youtube_download_videos,
                video_downloads_folder: settings.youtube_video_downloads_folder,
            };
            config.lastfm = {
                source: settings.lastfm_source,
                fallback_source: settings.lastfm_fallback_source,
            };

            config.cli = {
                text_output: settings.cli_text_output,
                progress_bars: settings.cli_progress_bars,
                max_search_results: settings.cli_max_search_results,
            };
            config.misc = {
                version: '2.0.6',
                check_for_updates: 'false'
            };
            const tomlString = TOML.stringify(config);
            await fs.promises.writeFile(normalizedConfigPath, tomlString, 'utf8');
            event.reply('settings-saved', 'Settings saved successfully');
        } catch (err) {
            console.error('Error handling streamrip config:', err);
            console.error('Attempted path:', paths.configPath);
            event.reply('settings-error', `Failed to update streamrip configuration: ${err.message}`);
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
                if (paths?.configPath) {
                    fs.readFile(paths.configPath, 'utf8', (tomlErr, tomlData) => {
                        if (!tomlErr) {
                            try {
                                const streamripConfig = TOML.parse(tomlData);

                                // Merge streamrip settings
                                settings = {
                                    ...settings,
                                    downloads_database_path: paths.downloadsDbPath,
                                    failed_downloads_database_path: paths.failedDownloadsDbPath,
                                    downloadLocation: settings.downloadLocation || streamripConfig.downloads?.folder,
                                    source_subdirectories: streamripConfig.downloads?.source_subdirectories || false,
                                    disc_subdirectories: streamripConfig.downloads?.disc_subdirectories || true,
                                    max_connections: streamripConfig.downloads?.max_connections || 6,
                                    concurrency: streamripConfig.downloads?.concurrency || true,
                                    qobuz_quality: streamripConfig.qobuz?.quality || 3,
                                    qobuz_download_booklets: streamripConfig.qobuz?.download_booklets || true,
                                    qobuz_token_or_email: streamripConfig.qobuz?.use_auth_token || true,
                                    qobuz_email_or_userid: streamripConfig.qobuz?.email_or_userid || "",
                                    qobuz_password_or_token: streamripConfig.qobuz?.password_or_token || "",
                                    qobuz_app_id: streamripConfig.qobuz?.app_id || "",
                                    qobuz_secrets: streamripConfig.qobuz?.secrets,
                                    tidal_quality: streamripConfig.tidal?.quality || 3,
                                    tidal_user_id: streamripConfig.tidal?.user_id || '',
                                    tidal_country_code: streamripConfig.tidal?.country_code || "US",
                                    tidal_access_token: streamripConfig.tidal?.access_token || "",
                                    tidal_refresh_token: streamripConfig.tidal?.refresh_token || "",
                                    tidal_token_expiry: streamripConfig.tidal?.token_expiry || "",
                                    deezer_quality: streamripConfig.deezer?.quality || "1",
                                    deezloader_warnings: streamripConfig.deezer?.deezloader_warnings || false,
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
                                    conversion_lossy_bitrate: streamripConfig.conversion?.lossy_bitrate || 320,
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
                    console.warn('custom_rip is not installed or not available, skipping streamrip settings load');
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

    streamripProcess.on('error', (err) => {
        console.warn('custom_rip is not installed:', err);
        callback(null);
    });

    streamripProcess.on('close', (code) => {
        if (code !== 0) {
            console.warn(`custom_rip exited with code ${code}: ${stderr}`);
            callback(null);
            return;
        }

        const configPathMatch = stdout.match(/Config path: '([^']+)'/);
        if (configPathMatch && configPathMatch[1]) {

            const rawPath = configPathMatch[1]
                .replace(/\n/g, '')
                .replace(/\r/g, '')
                .replace(/\\+/g, '/')
                .replace(/\s+/g, ' ')
                .trim();

            const configPath = path.normalize(rawPath);
            const configDir = path.dirname(configPath);
            console.log('Raw matched path:', configPathMatch[1]);
            console.log('Cleaned path:', rawPath);
            console.log('Normalized config path:', configPath);

            const paths = {
                configPath: configPath,
                downloadsDbPath: path.join(configDir, 'downloads.db'),
                failedDownloadsDbPath: path.join(configDir, 'failed_downloads.db')
            };

            callback(paths);
        } else {
            console.warn('Could not find config path in custom_rip output:', stdout);
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
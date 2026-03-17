const fs = require("fs");
const path = require("path");
const { app } = require("electron");
const { getDefaultSettings } = require("./defaults");

const settingsFilePath = path.join(app.getPath('userData'), 'mh-settings.json');
const spotifyConfigPath = path.join(app.getPath('userData'), 'votify_config.ini');
const appleConfigPath = path.join(app.getPath('userData'), 'gamdl_config.ini');

const APPLE_TO_GAMDL_KEY_MAP = {
    'cookies_path': 'cookies_path',
    'output_path': 'output_path',
    'temp_path': 'temp_path',
    'download_mode': 'download_mode',
    'remux_mode': 'music_video_remux_mode',
    'cover_format': 'cover_format',
    'cover_size': 'cover_size',
    'save_cover': 'save_cover',
    'synced_lyrics_format': 'synced_lyrics_format',
    'synced_lyrics_only': 'synced_lyrics_only',
    'no_synced_lyrics': 'no_synced_lyrics',
    'template_folder_album': 'album_folder_template',
    'template_folder_compilation': 'compilation_folder_template',
    'template_file_single_disc': 'single_disc_file_template',
    'template_file_multi_disc': 'multi_disc_file_template',
    'template_folder_no_album': 'no_album_folder_template',
    'template_file_no_album': 'no_album_file_template',
    'template_file_playlist': 'playlist_file_template',
    'date_tag_template': 'date_tag_template',
    'save_playlist': 'save_playlist',
    'overwrite': 'overwrite',
    'language': 'language',
    'truncate': 'truncate',
    'exclude_tags': 'exclude_tags',
    'log_level': 'log_level',
    'use_album_date': 'use_album_date',
    'fetch_extra_tags': 'fetch_extra_tags',
    'no_exceptions': 'no_exceptions',
    'mv_codec_priority': 'music_video_codec_priority',
    'mv_remux_format': 'music_video_remux_format',
    'mv_resolution': 'music_video_resolution',
    'uploaded_video_quality': 'uploaded_video_quality',
    'nm3u8dlre_path': 'nm3u8dlre_path',
    'mp4decrypt_path': 'mp4decrypt_path',
    'ffmpeg_path': 'ffmpeg_path',
    'mp4box_path': 'mp4box_path',
    'wvd_path': 'wvd_path',
    'use_wrapper': 'use_wrapper',
    'wrapper_account_url': 'wrapper_account_url',
    'wrapper_decrypt_ip': 'wrapper_decrypt_ip',
};

const SPOTIFY_TO_VOTIFY_KEY_MAP = {
    'cookies_path': 'cookies_path',
    'output_path': 'output',
    'audio_quality': 'audio_quality',
    'audio_download_mode': 'audio_download_mode',
    'audio_remux_mode': 'audio_remux_mode',
    'video_format': 'video_format',
    'video_resolution': 'video_resolution',
    'video_remux_mode': 'video_remux_mode',
    'cover_size': 'cover_size',
    'wvd_path': 'wvd_path',
    'no_drm': 'no_drm',
    'wait_interval': 'wait_interval',
    'overwrite': 'overwrite',
    'no_synced_lyrics_file': 'no_synced_lyrics_file',
    'save_playlist_file': 'save_playlist_file',
    'save_cover_file': 'save_cover_file',
    'synced_lyrics_only': 'synced_lyrics_only',
    'album_folder_template': 'album_folder_template',
    'compilation_folder_template': 'compilation_folder_template',
    'podcast_folder_template': 'podcast_folder_template',
    'no_album_folder_template': 'no_album_folder_template',
    'single_disc_file_template': 'single_disc_file_template',
    'multi_disc_file_template': 'multi_disc_file_template',
    'podcast_file_template': 'podcast_file_template',
    'no_album_file_template': 'no_album_file_template',
    'playlist_file_template': 'playlist_file_template',
    'date_tag_template': 'date_tag_template',
    'truncate': 'truncate',
    'exclude_tags': 'exclude_tags',
    'log_level': 'log_level',
    'no_exceptions': 'no_exceptions',
    'artist_media_option': 'artist_media_option',
    'prefer_video': 'prefer_video',
};


const mergeServiceSettings = (settings, serviceConfig, servicePrefix) => {
    const merged = { ...settings };

    Object.entries(serviceConfig).forEach(([key, value]) => {
        const settingKey = `${servicePrefix}_${key}`;
        if (!merged[settingKey] && merged[settingKey] !== false && merged[settingKey] !== 0) {
            merged[settingKey] = value;
        }
    });

    return merged;
};

function loadTheSettings() {
    try {
        const settingsData = fs.readFileSync(settingsFilePath, 'utf8');
        return JSON.parse(settingsData);
    } catch (err) {
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
            value !== null && value !== undefined && value !== '' && value !== 'null'
        )
        .reduce((obj, [key, value]) => {
            obj[key.replace(servicePrefix + '_', '')] = value;
            return obj;
        }, {});

    try {
        if (servicePrefix === 'apple') {
            let ini = '[gamdl]\n';
            for (const [appKey, value] of Object.entries(serviceSettings)) {
                const gamdlKey = APPLE_TO_GAMDL_KEY_MAP[appKey];
                if (!gamdlKey) continue;
                if (typeof value === 'boolean') {
                    ini += `${gamdlKey} = ${value ? 'true' : 'false'}\n`;
                } else {
                    ini += `${gamdlKey} = ${value}\n`;
                }
            }
            await fs.promises.writeFile(configPath, ini, 'utf8');
        } else if (servicePrefix === 'spotify') {
            const existingExtra = {};
            const mappedIniKeys = new Set(Object.values(SPOTIFY_TO_VOTIFY_KEY_MAP));
            try {
                const existing = await fs.promises.readFile(configPath, 'utf8');
                for (const line of existing.split('\n')) {
                    const trimmed = line.trim();
                    if (!trimmed || trimmed.startsWith('[') || trimmed.startsWith('#')) continue;
                    const eqIdx = trimmed.indexOf('=');
                    if (eqIdx === -1) continue;
                    const key = trimmed.substring(0, eqIdx).trim();
                    if (!mappedIniKeys.has(key)) {
                        existingExtra[key] = trimmed.substring(eqIdx + 1).trim();
                    }
                }
            } catch (e) {}

            let ini = '[votify]\n';
            for (const [appKey, value] of Object.entries(serviceSettings)) {
                const votifyKey = SPOTIFY_TO_VOTIFY_KEY_MAP[appKey];
                if (!votifyKey) continue;
                if (typeof value === 'boolean') {
                    ini += `${votifyKey} = ${value ? 'true' : 'false'}\n`;
                } else {
                    ini += `${votifyKey} = ${value}\n`;
                }
            }
            for (const [key, value] of Object.entries(existingExtra)) {
                ini += `${key} = ${value}\n`;
            }
            await fs.promises.writeFile(configPath, ini, 'utf8');
        } else {
            await fs.promises.writeFile(
                configPath,
                JSON.stringify(serviceSettings, null, 4),
                'utf8'
            );
        }
    } catch (err) {
        throw err;
    }
}

async function loadServiceConfig(configPath) {
    try {
        const data = await fs.promises.readFile(configPath, 'utf8');
        if (configPath.endsWith('.ini')) {
            const isVotify = data.includes('[votify]');
            const keyMap = isVotify ? SPOTIFY_TO_VOTIFY_KEY_MAP : APPLE_TO_GAMDL_KEY_MAP;
            const result = {};
            const reverseMap = {};
            for (const [appKey, configKey] of Object.entries(keyMap)) {
                reverseMap[configKey] = appKey;
            }
            const lines = data.split('\n');
            for (const line of lines) {
                const trimmed = line.trim();
                if (!trimmed || trimmed.startsWith('[') || trimmed.startsWith('#')) continue;
                const eqIdx = trimmed.indexOf('=');
                if (eqIdx === -1) continue;
                const key = trimmed.substring(0, eqIdx).trim();
                let val = trimmed.substring(eqIdx + 1).trim();
                const appKey = reverseMap[key];
                if (!appKey) continue;
                if (val === 'true') val = true;
                else if (val === 'false') val = false;
                result[appKey] = val;
            }
            return result;
        }
        return JSON.parse(data);
    } catch (err) {
        return {};
    }
}

async function saveSettings(settings) {
    const defaultKeys = Object.keys(getDefaultSettings());

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

    const cleanedSettings = Object.fromEntries(
        Object.entries(settings).filter(([key]) => defaultKeys.includes(key))
    );

    await fs.promises.writeFile(
        settingsFilePath,
        JSON.stringify(cleanedSettings, null, 4)
    );

    await Promise.all([
        saveServiceConfig(spotifyConfigPath, settings, 'spotify'),
        saveServiceConfig(appleConfigPath, settings, 'apple')
    ]);
}

async function loadSettings() {
    let settings;
    try {
        const data = await fs.promises.readFile(settingsFilePath, 'utf8');
        settings = JSON.parse(data);
    } catch {
        settings = getDefaultSettings();
    }

    const [spotifyConfig, appleConfig] = await Promise.all([
        loadServiceConfig(spotifyConfigPath),
        loadServiceConfig(appleConfigPath)
    ]);

    settings = mergeServiceSettings(settings, spotifyConfig, 'spotify');
    settings = mergeServiceSettings(settings, appleConfig, 'apple');

    fs.promises.writeFile(settingsFilePath, JSON.stringify(settings, null, 4)).catch(() => {});

    return settings;
}

function setupSettingsHandlers(ipcMain) {
    ipcMain.on('load-settings', async (event) => {
        try {
            const data = await loadSettings();
            event.reply('settings-data', data);
        } catch (err) {
            event.reply('settings-error', 'Failed to load settings');
            event.reply('settings-data', getDefaultSettings());
        }
    });

    ipcMain.on('save-settings', async (event, settings) => {
        try {
            await saveSettings(settings);
            event.reply('settings-saved', 'Settings saved successfully');
        } catch (err) {
            event.reply('settings-error', `Failed to save settings: ${err.message || err}`);
        }
    });
}

module.exports = {
    setupSettingsHandlers,
    loadSettings,
    saveSettings,
    saveServiceConfig,
    spotifyConfigPath
};
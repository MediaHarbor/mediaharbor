// Global state
const state = {
    downloads: [],
    currentPage: 'music'
};
let settings = {};

window.addEventListener('DOMContentLoaded', initializeTheme);
// theme checker
function applyTheme(theme) {
    // Remove existing theme classes
    document.body.classList.remove('dark', 'light');

    if (theme === 'auto') {
        const isDarkMode = window.matchMedia('(prefers-color-scheme: dark)').matches;
        document.body.classList.add(isDarkMode ? 'dark' : 'light');
        applyDarkModeScrollbars(isDarkMode);
    } else if (theme === 'dark') {
        document.body.classList.add('dark');
        applyDarkModeScrollbars(true);
    } else {
        document.body.classList.add('light');
        applyDarkModeScrollbars(false);
    }
}

function applyDarkModeScrollbars(isDarkMode) {
    let styleElement = document.getElementById('scrollbar-styles');

    if (!styleElement) {
        styleElement = document.createElement('style');
        styleElement.id = 'scrollbar-styles';
        document.head.appendChild(styleElement);
    }

    if (isDarkMode) {
        styleElement.innerHTML = `
            *::-webkit-scrollbar-track {
                background: #1d1d1d;
            }
            *::-webkit-scrollbar-thumb {
                background-color: #4CAF50;
                border-radius: 10px;
                border: 3px solid #2f2f2f;
            }
        `;
    } else {
        styleElement.innerHTML = ''; // Remove the dark mode styles
    }
}

// Initialize the theme based on user preference or system default
function initializeTheme() {
    const savedTheme = localStorage.getItem('theme') || 'auto'; // Default to 'auto'
    applyTheme(savedTheme);

    // Listen for system theme changes in auto mode
    if (savedTheme === 'auto') {
        window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
            applyTheme('auto');
        });
    }

    // Set the select option to reflect the current theme
    document.getElementById('theme').value = savedTheme;
}
document.addEventListener('DOMContentLoaded', () => {
    loadPage('music');
    document.querySelector('.navbar').classList.add('show');
});

const pages = {
    music: 'music.html',
    video: 'video.html',
    downloads: 'downloads.html',
    settings: 'settings.html',
    help: 'help.html',
    search: 'search.html'
};


async function locateDownload(location) {
    try {
        console.log('Attempting to show file:', location);
        const result = await window.electronAPI.showItemInFolder(location);
        if (!result) {
            console.error('Failed to show file location');
        }
    } catch (error) {
        console.error('Error locating download:', error);
        alert('Could not locate file: ' + error.message);
    }
}

async function deleteDownload(id) {
    try {
        await window.electronAPI.deleteDownload(id);
        const element = document.querySelector(`[data-id="${id}"]`);
        if (element) {
            element.remove();
        }
    } catch (error) {
        console.error('Error deleting download:', error);
    }
}
async function clearStreamripDatabase() {
    const { value: formValues } = await Swal.fire({
        title: "Select Databases to Clear",
        html:
            '<input type="checkbox" id="failedDownloads" /> Failed Downloads Database<br>' +
            '<input type="checkbox" id="downloads" /> Downloads Database',
        showCancelButton: true,
        confirmButtonText: "Delete",
        preConfirm: () => {
            return {
                failedDownloads: document.getElementById('failedDownloads').checked,
                downloads: document.getElementById('downloads').checked
            };
        }
    });

    // Proceed if user confirmed
    if (formValues) {
        window.electron.ipcRenderer.send("clear-database", formValues);
    }
}

async function clearDownloadsDatabase() {
    const result = await Swal.fire({
        title: 'Clear Downloads?',
        text: 'Are you sure you want to clear all downloads from the database?',
        icon: 'warning',
        showCancelButton: true,
        confirmButtonText: 'Yes, clear it!',
        cancelButtonText: 'No, keep them',
        reverseButtons: true,
        customClass: {
            confirmButton: 'btn btn-success',
            cancelButton: 'btn btn-danger'
        },
        buttonsStyling: false
    });

    if (result.isConfirmed) {
        try {
            await window.electronAPI.clearDownloadsDatabase();
            document.getElementById('download-history-container').innerHTML =
                '<p class="no-downloads">No downloads found.</p>';
            Swal.fire('Cleared!', 'Your download history has been cleared.', 'success');
        } catch (error) {
            console.error('Error clearing downloads database:', error);
            Swal.fire('Error!', 'There was an error clearing the downloads database.', 'error');
        }
    }
}

async function loadPage(pageName) {
    const contentDiv = document.getElementById('content');
    try {
        const response = await fetch(pages[pageName]);
        if (!response.ok) throw new Error('Page not found!');
        const pageContent = await response.text();
        contentDiv.innerHTML = pageContent;
        state.currentPage = pageName;
        if (pageName === 'music') {
            initializeMusicTab();
        }
        else if (pageName === 'video') {
            initializeVideoTab();
        }
        else if (pageName === 'settings'){
            initializeSettingsTab();
        }
        else if (pageName === 'help') {
            await initializeHelpTab();
        }
        else if (pageName === 'downloads') {
            await initializeDownloadStatusPage();
        }
        else if (pageName === 'search') {
            await initializeSearchPage()
        }
    } catch (error) {
        contentDiv.innerHTML = '<p>Error loading the page: ' + error.message + '</p>';
    }
    updateActiveNavButton(pageName);
}

function updateActiveNavButton(pageName) {
    const navButtons = document.querySelectorAll('.navbar a');
    navButtons.forEach(button => {
        button.classList.remove('active');
        if (button.id === `${pageName}Btn`) {
            button.classList.add('active');
        }
    });
}
async function initializeHelpTab(){
    // Toggle visibility of FAQ sections with animations
    document.querySelectorAll("section").forEach(section => {
        section.addEventListener("click", () => {
            const content = section.querySelector(".faq-content");
            const allSections = document.querySelectorAll("section");

            // Collapse other sections and open the clicked one
            allSections.forEach(sec => sec.classList.remove("opened"));
            section.classList.toggle("opened");

            // Adjust the FAQ content display
            document.querySelectorAll(".faq-content").forEach(c => {
                if (c !== content) c.style.display = "none";
            });

            content.style.display = content.style.display === "block" ? "none" : "block";
        });
    });
}

function inToggleActiveOnChange(checkboxId, fieldSelector) {
    const checkbox = document.getElementById(checkboxId);
    const field = document.querySelector(fieldSelector);

    checkbox.addEventListener('change', () => {
        if (checkbox.checked) {
            field.style.display = 'none'; // Hide the field when checked
        } else {
            field.style.display = 'block'; // Show the field when unchecked
        }
    });
}
async function initializeDownloadStatusPage() {
    const downloadContainer = document.getElementById('download-history-container');
    const warningContainer = document.getElementById('database-warning');

    try {
        let downloads = await window.electronAPI.getDownloads();
        console.log('Received downloads:', downloads);

        if (!downloads || downloads.length === 0) {
            downloadContainer.innerHTML = '<p class="no-downloads">No downloads found.</p>';
            return;
        }

        // Reverse the order of downloads
        downloads = downloads.reverse();

        const downloadsList = downloads.map(download => {
            const escapedLocation = download.downloadLocation
                .replace(/\\/g, '\\\\')  // Escape backslashes first
                .replace(/'/g, "\\'");    // Escape single quotes

            return `
                <div class="download-item" data-id="${download.id}">
                    <div class="download-thumbnail">
                        ${download.downloadThumbnail ?
                `<img src="${download.downloadThumbnail}" alt="${download.downloadName}">` :
                '<div class="no-thumbnail">No thumbnail</div>'
            }
                    </div>
                    <div class="download-info">
                        <h3 class="download-name">${download.downloadName}</h3>
                        <p class="download-artist">${download.downloadArtistOrUploader}</p>
                        <p class="download-location">${download.downloadLocation}</p>
                        <div class="download-actions">
                            <button class="fab-button delete" onclick="deleteDownload(${download.id})" title="Delete">
                                <i class="fas fa-trash"></i>
                            </button>
                            <button class="fab-button locate" onclick="locateDownload('${escapedLocation}')" title="Locate">
                                <i class="fas fa-folder-open"></i>
                            </button>
                        </div>
                    </div>
                </div>
            `;
        }).join('');

        downloadContainer.innerHTML = downloadsList;
    } catch (error) {
        console.error('Error loading downloads:', error);
        showNotification(`ERROR DATABASE FILE NOT FOUND PLEASE SELECT NEW LOCATION, ${error}`)
        warningContainer.style.display = 'block';
    }
}



// Handle dependent fields
function handleDependentFields() {
    // Function to toggle field visibility based on checkbox state
    function toggleFieldVisibility(checkbox, fieldSelector) {
        const field = document.querySelector(fieldSelector);
        if (checkbox && field) {
            field.classList.toggle('active', checkbox.checked);
        }
    }

    // Function to set up checkbox listener and initial state
    function setupDependentField(checkboxId, fieldSelector) {
        const checkbox = document.getElementById(checkboxId);
        if (checkbox) {
            // Set up change listener
            checkbox.addEventListener('change', function() {
                toggleFieldVisibility(this, fieldSelector);
            });
            // Set initial state
            toggleFieldVisibility(checkbox, fieldSelector);
        }
    }

    // Special handling for speed limit fields
    const speedLimitCheck = document.getElementById('download_speed_limit');
    const speedLimitFields = document.querySelectorAll('#speed_limit_value, .dropdown');
    if (speedLimitCheck) {
        // Set up listener
        speedLimitCheck.addEventListener('change', function() {
            speedLimitFields.forEach(el => el.closest('.dependent-field')?.classList.toggle('active', this.checked));
        });
        // Set initial state
        speedLimitFields.forEach(el => el.closest('.dependent-field')?.classList.toggle('active', speedLimitCheck.checked));
    }

    // List of all checkbox-field mappings
    const fieldMappings = [
        { checkboxId: 'use_proxy', fieldSelector: '#proxy_url' },
        { checkboxId: 'use_authentication', fieldSelector: '#auth-fields' },
        { checkboxId: 'downloads_database_check', fieldSelector: '#str_download_database_form' },
        { checkboxId: 'failed_downloads_database_check', fieldSelector: '#str_failed_downloads_database_form' },
        { checkboxId: 'conversion_check', fieldSelector: '#conversion_container' },
        { checkboxId: 'use_cookies', fieldSelector: '#cookies_form' },
        { checkboxId: 'use_cookies', fieldSelector: '#browser_cookies_form' },
        { checkboxId: 'override_download_extension', fieldSelector: '#override_download_extension_form' },
        { checkboxId: 'yt_override_download_extension', fieldSelector: '#youtubeExtensions' },
        { checkboxId: 'ytm_override_download_extension', fieldSelector: '#youtubeMusicExtensions' },
        {checkboxId: 'filepaths_restrict_characters', fieldSelector: '#filepaths_restrict_characters_form'}
    ];

    // Set up all mappings
    fieldMappings.forEach(({ checkboxId, fieldSelector }) => {
        setupDependentField(checkboxId, fieldSelector);
    });

    // Handle inverse toggles (hide when checked)
    const inverseFieldMappings = [
        { checkboxId: 'deezer_use_deezloader', fieldSelector: '#deezer_arl_input' }
    ];

    inverseFieldMappings.forEach(({ checkboxId, fieldSelector }) => {
        const checkbox = document.getElementById(checkboxId);
        const field = document.querySelector(fieldSelector);
        if (checkbox && field) {
            // Set up change listener
            checkbox.addEventListener('change', () => {
                field.style.display = checkbox.checked ? 'none' : 'block';
            });
            // Set initial state
            field.style.display = checkbox.checked ? 'none' : 'block';
        }
    });
}
function openPopup(url) {
    window.open(url, '_blank', 'width=600,height=8000');
}

// Settings Tab
function initializeSettingsTab() {
    initializeDropdowns();
    handleTabSwitch();
    handleDependentFields();
    handleDropdownSelection();
    handleSectionExpansion();
    populateSettings();
    window.electronAPI.send('load-settings');
    document.getElementById('theme').addEventListener('change', function() {
        const selectedTheme = this.value;
        localStorage.setItem('theme', selectedTheme);
        applyTheme(selectedTheme);
    });


    window.electronAPI.receive('settings-data', (loadedSettings) => {
        settings = loadedSettings;
        populateSettings();
        addSettingsListeners();
    });


}
async function selectFileLocation(inputId) {
    try {
        // Get the save file path using system dialog
        const result = await window.electronAPI.fileLocation();

        if (result) {
            // Update the corresponding input field with the selected path
            const inputElement = document.getElementById(inputId);
            if (inputElement) {
                inputElement.value = result;
                settings[inputId] = result;
                window.electronAPI.send('save-settings', settings);
            }
        }
    } catch (error) {
        console.error('Error selecting file location:', error);
    }
}

// Function to handle folder selection
async function selectFolderLocation(inputId) {
    try {
        // Get the folder path using system dialog
        const result = await window.electronAPI.folderLocation();

        if (result) {
            // Update the corresponding input field with the selected path
            const inputElement = document.getElementById(inputId);
            if (inputElement) {
                inputElement.value = result;
                // Save settings right after updating the input
                settings[inputId] = result;
                window.electronAPI.send('save-settings', settings);
            }
        }
    } catch (error) {
        console.error('Error selecting folder location:', error);
    }
}

async function openFileLocation(inputId) {
    try {
        // Get the folder path using system dialog
        const result = await window.electronAPI.fileSelectLocation();

        // Log the result for debugging
        console.log('Selected file path:', result);

        if (result && typeof result === 'string') {
            // Update the corresponding input field with the selected path
            const inputElement = document.getElementById(inputId);
            if (inputElement) {
                inputElement.value = result;
                // Save settings right after updating the input
                settings[inputId] = result;
                window.electronAPI.send('save-settings', settings);
            }
            return result;
        } else {
            console.error('Invalid file path selected:', result);
            return null;  // Ensure we handle invalid file paths
        }
    } catch (error) {
        console.error('Error selecting folder location:', error);
        return null;  // Handle errors by returning null
    }
}
async function openWvdLocation(inputId) {
    try {
        // Get the folder path using system dialog
        const result = await window.electronAPI.openWvdLocation();

        if (result && typeof result === 'string') {
            // Update the corresponding input field with the selected path
            const inputElement = document.getElementById(inputId);
            if (inputElement) {
                inputElement.value = result;
                // Save settings right after updating the input
                settings[inputId] = result;
                window.electronAPI.send('save-settings', settings);
            }
            return result;
        } else {
            console.error('Invalid file path selected:', result);
            return null;  // Ensure we handle invalid file paths
        }
    } catch (error) {
        console.error('Error selecting folder location:', error);
        return null;  // Handle errors by returning null
    }
}

function handleTabSwitch() {
    const tabButtons = document.querySelectorAll('.tab-button');
    const tabContents = document.querySelectorAll('.tab-content');

    tabButtons.forEach(button => {
        button.addEventListener('click', () => {
            document.querySelector('.tab-button.active').classList.remove('active');
            document.querySelector('.tab-content.active').classList.remove('active');

            button.classList.add('active');
            document.getElementById(button.dataset.tab).classList.add('active');
        });
    });
}

function handleDropdownSelection() {
    document.querySelectorAll('.dropdown-content a').forEach(item => {
        item.addEventListener('click', (e) => {
            e.preventDefault();
            const dropdown = item.closest('.dropdown');
            dropdown.querySelector('.dropdown-btn').textContent = item.textContent;
            toggleDropdown(dropdown.querySelector('.dropdown-content').id);
        });
    });
}

function handleSectionExpansion() {
    document.querySelectorAll('.settings-section').forEach(section => {
        const header = section.querySelector('.settings-header');
        const content = section.querySelector('.settings-content');
        if (header && content) {
            header.removeAttribute('onclick');
            header.addEventListener('click', () => {
                const isVisible = content.style.display === 'block';
                content.style.display = isVisible ? 'none' : 'block';
                header.classList.toggle('expanded', !isVisible);
                content.classList.toggle('expanded', !isVisible);
            });
        }
    });

    document.querySelectorAll('.settings-content').forEach(content => {
        content.style.display = 'none';
    });
}

function populateSettings() {
    const settingFields = {
        'autoUpdate': 'checked',
        'theme': 'value', 'downloadsDatabasePath': 'value', 'downloadLocation': 'value',
        'createPlatformSubfolders': 'checked', 'orpheusDL': 'checked', 'streamrip': 'checked',
        'use_cookies': 'checked', 'cookies': 'value', 'cookies_from_browser': 'value',
        'override_download_extension': 'checked', 'yt_override_download_extension': 'checked',
        'ytm_override_download_extension': 'checked', 'youtubeVideoExtensions': 'value',
        'youtubeAudioExtensions': 'value', 'use_aria2': 'checked', 'auto_update': 'checked',
        'max_downloads': 'value', 'download_speed_limit': 'checked', 'speed_limit_value': 'value',
        'max_retries': 'value', 'download_output_template': 'value', 'continue': 'checked',
        'add_metadata': 'checked', 'embed_chapters': 'checked', 'add_subtitle_to_file': 'checked',
        'use_proxy': 'checked', 'proxy_url': 'value', 'use_authentication': 'checked',
        'username': 'value', 'password': 'value', 'sponsorblock_mark': 'value',
        'sponsorblock_remove': 'value', 'sponsorblock_chapter_title': 'value',
        'no_sponsorblock': 'checked', 'sponsorblock_api_url': 'value', 'disc_subdirectories': 'checked',
        'concurrency': 'checked', 'max_connections': 'value', 'requests_per_minute': 'value',
        'qobuz_download_booklets': 'checked', 'qobuz_token_or_email': 'checked',
        'qobuz_email_or_userid': 'value', 'qobuz_password_or_token': 'value',
        'tidal_download_videos': 'checked', 'deezer_use_deezloader': 'checked',
        'deezer_arl': 'value', 'downloads_database_check': 'checked', 'downloads_database': 'value',
        'failed_downloads_database_check': 'checked', 'failed_downloads_database': 'value',
        'filepaths_add_singles_to_folder': 'checked', 'filepaths_folder_format': 'value', 'filepaths_track_format': 'value', 'filepaths_restrict_characters': 'checked', 'filepaths_truncate_to': 'value',
        'conversion_check': 'checked', 'conversion_codec': 'value', 'conversion_sampling_rate': 'value',
        'conversion_bit_depth': 'value', 'meta_album_name_playlist_check': 'checked',
        'meta_album_order_playlist_check': 'checked','excluded_tags': 'value', 'speed_limit_type': 'value',
        'qobuz_app_id': 'value', 'qobuz_secrets': 'value',
        'apple_output_path': 'value', 'apple_temp_path': 'value', 'apple_download_mode': 'value', 'apple_remux_mode': 'value', 'apple_cover_format': 'value', 'apple_synced_lyrics_format': 'value',
        'apple_template_folder_album': 'value', 'apple_template_folder_compilation': 'value', 'apple_template_file_single_disc': 'value', 'apple_template_file_multi_disc': 'value',
        "apple_disable_music_video_skip": "checked", "apple_save_cover": "checked", "apple_overwrite": "checked", "apple_save_playlist": "checked", "apple_synced_lyrics_only": "checked", "apple_no_synced_lyrics": "checked", "apple_cover_size": "value",
        "apple_cookies_path": "value",
        "zotify_userName": "value",
        "spotify_audio_format": "value",
        "spotify_transcode_bitrate": "value",
        "spotify_ffmpeg_args": "value",
        "spotify_download_quality": "value",
        "spotify_artwork_size": "value",
        "spotify_save_subtitles": "checked",
        "spotify_lyrics_file": "checked",
        "spotify_lyrics_only": "checked",
        "spotify_create_playlist_file": "checked",
        "spotify_save_metadata": "checked",
        "spotify_replace_existing": "checked",
        "spotify_skip_previous": "checked",
        "spotify_skip_duplicates": "checked",
        "spotify_output_album": "value",
        "spotify_output_playlist_track": "value",
        "spotify_output_playlist_episode": "value",
        "spotify_output_podcast": "value",
    };

    Object.keys(settingFields).forEach(id => {
        const element = document.getElementById(id);
        if (element) {
            element[settingFields[id]] = settings[id];
        }
    });
    handleDependentFields();
}

async function deleteZotifyCredentials() {
    try {
        const zotify_userName = document.getElementById('zotify_userName');
        const result = await window.electron.invoke('clear-zotify-credentials');
        zotify_userName.value = '';
        if (result.success) {
            showNotification('Credentials cleared',);
        } else {
            throw new Error(result.message);
        }
    } catch (error) {
        console.error('Failed to clear credentials:', error);
        showNotification('Error', 'Failed to clear credentials: ' + error.message);
    }
}

function addSettingsListeners() {
    const settingsMapping = [
        {id: 'autoUpdate', key: 'autoUpdate', type: 'checked'},
        { id: 'theme', key: 'theme', type: 'value' },
        { id: 'downloadsDatabasePath', key: 'downloadsDatabasePath', type: 'value' },
        { id: 'downloadLocation', key: 'downloadLocation', type: 'value' },
        { id: 'createPlatformSubfolders', key: 'createPlatformSubfolders', type: 'checked' },
        { id: 'orpheusDL', key: 'orpheusDL', type: 'checked' },
        { id: 'streamrip', key: 'streamrip', type: 'checked' },
        { id: 'use_cookies', key: 'use_cookies', type: 'checked' },
        { id: 'cookies', key: 'cookies', type: 'value' },
        { id: 'cookies_from_browser', key: 'cookies_from_browser', type: 'value' },
        { id: 'override_download_extension', key: 'override_download_extension', type: 'checked' },
        { id: 'yt_override_download_extension', key: 'yt_override_download_extension', type: 'checked' },
        { id: 'ytm_override_download_extension', key: 'ytm_override_download_extension', type: 'checked' },
        { id: 'youtubeVideoExtensions', key: 'youtubeVideoExtensions', type: 'value' },
        { id: 'youtubeAudioExtensions', key: 'youtubeAudioExtensions', type: 'value' },
        { id: 'use_aria2', key: 'use_aria2', type: 'checked' },
        { id: 'auto_update', key: 'auto_update', type: 'checked' },
        { id: 'max_downloads', key: 'max_downloads', type: 'value' },
        { id: 'download_speed_limit', key: 'download_speed_limit', type: 'checked' },
        { id: 'speed_limit_value', key: 'speed_limit_value', type: 'value' },
        { id: 'max_retries', key: 'max_retries', type: 'value' },
        { id: 'download_output_template', key: 'download_output_template', type: 'value' },
        { id: 'continue', key: 'continue', type: 'checked' },
        { id: 'add_metadata', key: 'add_metadata', type: 'checked' },
        { id: 'embed_chapters', key: 'embed_chapters', type: 'checked' },
        { id: 'add_subtitle_to_file', key: 'add_subtitle_to_file', type: 'checked' },
        { id: 'use_proxy', key: 'use_proxy', type: 'checked' },
        { id: 'proxy_url', key: 'proxy_url', type: 'value' },
        { id: 'use_authentication', key: 'use_authentication', type: 'checked' },
        { id: 'username', key: 'username', type: 'value' },
        { id: 'password', key: 'password', type: 'value' },
        { id: 'sponsorblock_mark', key: 'sponsorblock_mark', type: 'value' },
        { id: 'sponsorblock_remove', key: 'sponsorblock_remove', type: 'value' },
        { id: 'sponsorblock_chapter_title', key: 'sponsorblock_chapter_title', type: 'value' },
        { id: 'no_sponsorblock', key: 'no_sponsorblock', type: 'checked' },
        { id: 'sponsorblock_api_url', key: 'sponsorblock_api_url', type: 'value' },
        { id: 'disc_subdirectories', key: 'disc_subdirectories', type: 'checked' },
        { id: 'concurrency', key: 'concurrency', type: 'checked' },
        { id: 'max_connections', key: 'max_connections', type: 'value' },
        { id: 'requests_per_minute', key: 'requests_per_minute', type: 'value' },
        { id: 'qobuz_download_booklets', key: 'qobuz_download_booklets', type: 'checked' },
        { id: 'qobuz_token_or_email', key: 'qobuz_token_or_email', type: 'checked' },
        { id: 'qobuz_email_or_userid', key: 'qobuz_email_or_userid', type: 'value' },
        { id: 'qobuz_password_or_token', key: 'qobuz_password_or_token', type: 'value' },
        { id: 'tidal_download_videos', key: 'tidal_download_videos', type: 'checked' },
        { id: 'deezer_use_deezloader', key: 'deezer_use_deezloader', type: 'checked' },
        { id: 'deezer_arl', key: 'deezer_arl', type: 'value' },
        { id: 'downloads_database_check', key: 'downloads_database_check', type: 'checked' },
        { id: 'downloads_database', key: 'downloads_database', type: 'value' },
        { id: 'failed_downloads_database_check', key: 'failed_downloads_database_check', type: 'checked' },
        { id: 'failed_downloads_database', key: 'failed_downloads_database', type: 'value' },
        { id: 'conversion_check', key: 'conversion_check', type: 'checked' },
        { id: 'conversion_codec', key: 'conversion_codec', type: 'value' },
        { id: 'conversion_sampling_rate', key: 'conversion_sampling_rate', type: 'value' },
        { id: 'conversion_bit_depth', key: 'conversion_bit_depth', type: 'value' },
        { id: 'meta_album_name_playlist_check', key: 'meta_album_name_playlist_check', type: 'checked' },
        { id: 'meta_album_order_playlist_check', key: 'meta_album_order_playlist_check', type: 'checked' },
        { id: 'excluded_tags', key: 'excluded_tags', type: 'value' },
        {id:'filepaths_add_singles_to_folder', key:'filepaths_add_singles_to_folder' ,type:'checked'},
        {id:'filepaths_folder_format', key:'filepaths_folder_format' ,type:'value'},
        {id:'filepaths_track_format', key:'filepaths_track_format' ,type:'value'},
        {id:'filepaths_restrict_characters', key:'filepaths_restrict_characters' ,type:'checked'},
        {id:'filepaths_truncate_to', key:'filepaths_truncate_to' ,type:'value'},
        {id: 'speed_limit_type', key:'speed_limit_type', type: 'value'},
        {id: 'qobuz_app_id', key: 'qobuz_app_id', type: 'value' },
        {id: 'qobuz_secrets', key: 'qobuz_secrets', type: 'value' },
        {id:'apple_output_path', key:'apple_output_path', type: 'value'},
        {id:'apple_temp_path', key:'apple_temp_path', type: 'value'},
        {id:'apple_download_mode', key:'apple_download_mode', type: 'value'},
        {id:'apple_remux_mode', key:'apple_remux_mode', type: 'value'},
        {id:'apple_cover_format', key:'apple_cover_format', type: 'value'},
        {id:'apple_synced_lyrics_format', key:'apple_synced_lyrics_format', type: 'value'},
        {id:'apple_template_folder_album', key:'apple_template_folder_album', type: 'value'},
        {id:'apple_template_folder_compilation', key:'apple_template_folder_compilation', type: 'value'},
        {id:'apple_template_file_single_disc', key:'apple_template_file_single_disc', type: 'value'},
        {id:'apple_template_file_multi_disc', key:'apple_template_file_multi_disc', type: 'value'},
        {id: "apple_disable_music_video_skip", key: "apple_disable_music_video_skip", type: "checked"},
        {id: "apple_save_cover", key: "apple_save_cover", type: "checked"},
        {id: "apple_overwrite", key: "apple_overwrite", type: "checked"},
        {id: "apple_save_playlist", key: "apple_save_playlist", type: "checked"},
        {id: "apple_synced_lyrics_only", key: "apple_synced_lyrics_only", type: "checked"},
        {id: "apple_no_synced_lyrics", key: "apple_no_synced_lyrics", type: "checked"},
        {id: "apple_cover_size", key: "apple_cover_size", type: "value"},
        {id: "apple_cookies_path", key: "apple_cookies_path", type: "value"},
        {id: "zotify_userName", key: "zotify_userName", type: "value"},
        {id: "spotify_audio_format", key: "spotify_audio_format", type: "value"},
        {id: "spotify_transcode_bitrate", key: "spotify_transcode_bitrate", type: "integer"},
        {id: "spotify_ffmpeg_args", key: "spotify_ffmpeg_args", type: "value"},
        {id: "spotify_download_quality", key: "spotify_download_quality", type: "value"},
        {id: "spotify_artwork_size", key: "spotify_artwork_size", type: "value"},
        {id: "spotify_save_subtitles", key: "spotify_save_subtitles", type: "checked"},
        {id: "spotify_lyrics_file", key: "spotify_lyrics_file", type: "checked"},
        {id: "spotify_lyrics_only", key: "spotify_lyrics_only", type: "checked"},
        {id: "spotify_create_playlist_file", key: "spotify_create_playlist_file", type: "checked"},
        {id: "spotify_save_metadata", key: "spotify_save_metadata", type: "checked"},
        {id: "spotify_replace_existing", key: "spotify_replace_existing", type: "checked"},
        {id: "spotify_skip_previous", key: "spotify_skip_previous", type: "checked"},
        {id: "spotify_skip_duplicates", key: "spotify_skip_duplicates", type: "checked"},
        {id: "spotify_output_album", key: "spotify_output_album", type: "value"},
        {id: "spotify_output_playlist_track", key: "spotify_output_playlist_track", type: "value"},
        {id: "spotify_output_playlist_episode", key: "spotify_output_playlist_episode", type: "value"},
        {id: "spotify_output_podcast", key: "spotify_output_podcast", type: "value"}
    ];

    settingsMapping.forEach(({ id, key, type }) => {
        const element = document.getElementById(id);
        if (element) {
            element.addEventListener('change', (e) => {
                if (type === 'checked') {
                    settings[key] = e.target.checked;
                } else if (type === 'integer') {
                    const intValue = parseInt(e.target.value, 10);
                    settings[key] = isNaN(intValue) ? 0 : intValue;
                } else {
                    settings[key] = e.target.value;
                }
                window.electronAPI.send('save-settings', settings);
            });

            if (type === 'value' && element.tagName === 'INPUT') {
                element.addEventListener('input', (e) => {
                    if (type === 'integer') {
                        const intValue = parseInt(e.target.value, 10);
                        settings[key] = isNaN(intValue) ? 0 : intValue;
                    } else {
                        settings[key] = e.target.value;
                    }
                    window.electronAPI.send('save-settings', settings);
                });
            }
        }
    });


}


// Search Tab
function initializeSearchPage() {
    handleTabSwitch();
    // Initialize all search inputs and buttons
    const platforms = ['youtube', 'youtubeMusic', 'spotify', 'deezer', 'qobuz', 'tidal', 'applemusic'];

    platforms.forEach(platform => {
        const searchInput = document.getElementById(`${platform}-search`);
        const searchButton = document.getElementById(`${platform}-search-button`);
        const searchType = document.getElementById(`${platform}-search-type`);

        // Add event listeners for search
        searchInput.addEventListener('keypress', (e) => {
            if (e.key === 'Enter') {
                performSearch(platform);
            }
        });

        searchButton.addEventListener('click', () => {
            performSearch(platform);
        });

        // Initialize dropdown functionality
        if (searchType) {
            initializeDropdown(searchType);
        }
    });

    // Set up event listeners for search results and stream ready events
    window.api.onSearchResults(handleSearchResults);
    window.api.onError(handleError);
}

function initializeDropdown(dropdown) {
    const button = dropdown.querySelector('.dropdown-btn');
    const content = dropdown.querySelector('.dropdown-content');

    content.querySelectorAll('a').forEach(item => {
        item.addEventListener('click', (e) => {
            e.preventDefault();
            button.textContent = item.textContent;
            button.dataset.value = item.dataset.value;
            content.style.display = 'none';
        });
    });

    button.addEventListener('click', (e) => {
        e.stopPropagation();
        content.style.display = content.style.display === 'block' ? 'none' : 'block';
    });

    document.addEventListener('click', (e) => {
        if (!dropdown.contains(e.target)) {
            content.style.display = 'none';
        }
    });
}

async function performSearch(platform) {
    const searchInput = document.getElementById(`${platform}-search`);
    const searchType = document.getElementById(`${platform}-search-type`);
    const query = searchInput.value.trim();
    const type = searchType ? searchType.querySelector('.dropdown-btn').dataset.value : 'track';

    if (!query) {
        showNotification('Please enter a search query');
        return;
    }

    showNotification('Searching...');

    try {
        const result = await window.api.performSearch({ platform, query, type });
        handleSearchResults(result, type);
    } catch (error) {
        handleError(error);
    }
}
window.errorNotifier.onError((message) => {
    showNotification(`Error: ${message}`);
});
function showLoadingOverlay() {
    const overlay = document.createElement('div');
    overlay.id = 'loading-overlay';
    overlay.style.cssText = `
        position: fixed;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        background: rgba(0, 0, 0, 0.7);
        display: flex;
        justify-content: center;
        align-items: center;
        z-index: 2000;
    `;

    const spinner = document.createElement('div');
    spinner.style.cssText = `
        width: 50px;
        height: 50px;
        border: 5px solid #f3f3f3;
        border-top: 5px solid #45a049; 
        border-radius: 50%;
        animation: spin 1s linear infinite;
    `;

    const style = document.createElement('style');
    style.textContent = `
        @keyframes spin {
            0% { transform: rotate(0deg); }
            100% { transform: rotate(360deg); }
        }
    `;

    document.head.appendChild(style);
    overlay.appendChild(spinner);
    document.body.appendChild(overlay);
}

function hideLoadingOverlay() {
    const overlay = document.getElementById('loading-overlay');
    if (overlay) {
        overlay.remove();
    }
}
let currentCardData = null;
let playlist = [];
let currentIndex = 0;
function handleSearchResults({ results, platform }, type) {
    const container = document.getElementById('search-container');
    container.innerHTML = '';

    let normalizedResults = normalizeResults(results, platform, type);

    // Store the playlist as an array of { result, platform, type }
    playlist = normalizedResults.map(result => ({ result, platform, type }));

    normalizedResults.forEach((result, index) => {
        const resultCard = createResultCard(result, platform, type, index);
        container.appendChild(resultCard);
    });

    window.api.onStreamReady(({ streamUrl, platform }) => {
        const mediaData = {
            streamUrl,
            platform,
            thumbnail: currentCardData.thumbnail,
            title: currentCardData.title,
            artist: currentCardData.artist || currentCardData.details,
            album: currentCardData.album
        };
        handleStreamReady(mediaData);
    });
}
window.electronAPI.receive('toggleLoading', (show) => {
    if (show) {
        showLoadingOverlay();
    } else {
        hideLoadingOverlay();
    }
});

// Show notifications from the main process
window.electronAPI.receive('showNotification', (message) => {
    showNotification(message);
});
async function dependencypopup() {
    let loadingPopup = null;
    const notificationHandler = (event, data) => {
        console.log("Notification received:", data); // Debug logging
        hideLoadingOverlay();
        let title = 'Notification';
        let message = 'Operation completed';
        let icon = 'info';

        try {
            // Only try to extract data if it's properly formed
            if (data && typeof data === 'object' && data.message) {
                icon = data.type || 'info';
                message = data.message;

                title = {
                    'success': 'Success',
                    'error': 'Error',
                    'warning': 'Warning',
                    'info': 'Information'
                }[icon] || 'Notification';
            } else if (typeof data === 'string') {
                message = data;
            }
        } catch (e) {
            console.error("Error processing notification:", e);
        }

        Swal.fire({
            title: title,
            text: message,
            icon: icon,
            timer: 3000,
            timerProgressBar: true
        });
    };

    const loadingHandler = (event, isLoading) => {
        console.log("Loading state changed:", isLoading); // Debug logging

        if (isLoading === true) {
            loadingPopup = Swal.fire({
                title: 'Updating Dependencies',
                text: 'Please wait while dependencies are updated...',
                allowOutsideClick: false,
                allowEscapeKey: false,
                didOpen: () => {
                    Swal.showLoading();
                }
            });
        } else {
            hideLoadingOverlay();
        }
    };

    // Register event listeners
    window.electronAPI.receive('showNotification', notificationHandler);
    window.electronAPI.receive('toggleLoading', loadingHandler);

    const dependencies = [
        { id: 'yt-dlp', label: 'yt-dlp', link: 'https://github.com/ytdlp/ytdlp', description: '' },
        { id: 'streamrip', label: 'streamrip (Qobuz, Tidal, Deezer Downloader)', link: 'https://github.com/mediaharbor/custom_streamrip', description: '(Qobuz, Deezer, Tidal)' },
        { id: 'ytmusicapi', label: 'YTMusic API (YT Music Search)', link: 'https://github.com/sigma67/ytmusicapi', description: '' },
        { id: 'zotify', label: 'Zotify (Spotify Downloader)', link: 'https://github.com/mediaharbor/custom_zotify', description: '(Spotify)' },
        { id: 'gamdl', label: 'Gamdl (Apple Music Downloader)', link: 'https://github.com/mediaharbor/custom_gamdl', description: '(Apple Music)' },
        { id: 'googleapi', label: 'googleapi', link: 'https://github.com/googleapis/google-api-python-client', description: '(YouTube Search API)' },
        { id: 'pyapplemusicapi', label: 'pyapplemusicapi (Apple Music API for Search)', link: 'https://github.com/queengooborg/pyapplemusicapi', description: '(Apple Music Search API)' },
    ];


    const dependencyHTML = dependencies.map(dep => `
    <div style="margin-bottom: 8px;">
        <label style="display: flex; align-items: center;">
            <input type="checkbox" id="${dep.id}" style="margin-right: 8px;" ${['yt-dlp', 'streamrip', 'ytmusicapi', 'zotify', 'gamdl'].includes(dep.id) ? 'checked' : ''}>
            <span>
                ${dep.label} (<a href="${dep.link}" target="_blank">GitHub</a>) ${dep.description}
            </span>
        </label>
    </div>
`).join('');

    const { value: selectedPackages } = await Swal.fire({
        title: 'Select Dependencies to Update',
        html: dependencyHTML,
        icon: 'info',
        showCancelButton: true,
        confirmButtonColor: "#4CAF50",
        confirmButtonText: 'Update Selected',
        cancelButtonText: 'Cancel',
        focusConfirm: false,
        preConfirm: () => {
            const selected = dependencies
                .filter(dep => Swal.getPopup().querySelector(`#${dep.id}`).checked)
                .map(dep => dep.id);
            return selected;
        }
    });

    if (selectedPackages && selectedPackages.length > 0) {
        window.electronAPI.send('updateDep', selectedPackages);
    } else if (selectedPackages !== undefined) {
        Swal.fire('No dependencies selected', 'Please select at least one package to install.', 'warning');
    }

    return function cleanup() {
        window.electronAPI.removeListener('showNotification', notificationHandler);
        window.electronAPI.removeListener('toggleLoading', loadingHandler);
    };
}
document.addEventListener("DOMContentLoaded", function () {
    const volumeSlider = document.getElementById('volume-slider');
    const muteButton = document.getElementById('mute-btn');
    const volumeIcon = document.getElementById('volume-icon');
    const volumeXmark = document.createElement('i');
    volumeXmark.classList.add('fa-solid', 'fa-volume-xmark');
    volumeXmark.id = 'volume-xmark';
    muteButton.appendChild(volumeXmark);

    function updateIcon() {
        if (volumeSlider.value === '0') {
            volumeIcon.style.display = 'none';
            volumeXmark.style.display = 'inline-block';
        } else {
            volumeIcon.style.display = 'inline-block';
            volumeXmark.style.display = 'none';
        }
    }

    volumeSlider.addEventListener('input', updateIcon);
    muteButton.addEventListener('click', () => {
        volumeSlider.value = volumeSlider.value === '0' ? '1' : '0';
        updateIcon();
    });

    updateIcon();

    const player = document.getElementById("custom-player");
    const navbar = document.querySelector(".navbar");
    const mainContent = document.querySelector(".main-content");
    function adjustNavbarPadding() {
        if (player && player.style.display !== "none") {
            navbar.style.paddingBottom = "80px";
            mainContent.style.paddingBottom = "80px";
        } else {
            navbar.style.paddingBottom = "";
            mainContent.style.paddingBottom = "";
        }
    }

    // Check padding on load
    adjustNavbarPadding();

    const observer = new MutationObserver(adjustNavbarPadding);
    observer.observe(player, { attributes: true, attributeFilter: ["style"] });
    window.addEventListener("beforeunload", () => observer.disconnect());

});


function normalizeResults(results, platform, type = 'track') {
    switch (platform) {
        case 'spotify':
            switch(type) {
                case 'track':
                    return results.tracks?.items || [];
                case 'album':
                    return results.albums?.items || [];
                case 'artist':
                    return results.artists?.items || [];
                case 'playlist':
                    return results.playlists?.items || [];
                case 'podcast':
                    return results.shows?.items;
                case 'episode':
                    return results.episodes?.items;
                default:
                    return [];
            }

        case 'qobuz':
            switch(type) {
                case 'track':
                    return results.tracks?.items || [];
                case 'album':
                    return results.albums?.items || [];
                case 'artist':
                    return results.artists?.items || [];
                case 'playlist':
                    return results.playlists?.items || [];
                default:
                    return [];
            }

        case 'tidal':
            switch(type) {
                case 'track':
                    return results.tracks.map(track => track.resource) || [];
                case 'album':
                    return results.albums.map(album => album.resource) || [];
                case 'artist':
                    return results.artists.map(artist => artist.resource) || [];
                case 'video':
                    return results.videos.map(video => video.resource) || [];
                default:
                    return [];
            }

        case 'applemusic':
            switch(type) {
                case 'track':
                    return Array.isArray(results) ? results.filter(item =>
                        item.wrapperType === 'track' && item.kind === 'song'
                    ) : [];
                case 'album':
                    return Array.isArray(results) ? results.filter(item =>
                        item.wrapperType === 'collection'
                    ) : [];
                case 'artist':
                    return Array.isArray(results) ? results.filter(item =>
                        item.wrapperType === 'artist'
                    ) : [];
                default:
                    return [];
            }

        default:
            return Array.isArray(results) ? results : [];
    }
}


function createResultCard(result, platform, type = 'track') {
    const card = document.createElement('div');
    card.className = 'result-card';

    const cardData = getCardData(result, platform, type);

    const title = cardData.title;

    // Explicit Icon
    const explicitIcon = cardData.explicit
        ? `<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" fill="currentColor" class="bi bi-explicit" viewBox="0 0 16 16">
                <path d="M6.826 10.88H10.5V12h-5V4.002h5v1.12H6.826V7.4h3.457v1.073H6.826z"/>
                <path d="M2.5 0A2.5 2.5 0 0 0 0 2.5v11A2.5 2.5 0 0 0 2.5 16h11a2.5 2.5 0 0 0 2.5-2.5v-11A2.5 2.5 0 0 0 13.5 0zM1 2.5A1.5 1.5 0 0 1 2.5 1h11A1.5 1.5 0 0 1 15 2.5v11a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 1 13.5z"/>
           </svg>`
        : '';
    // Hi res icon
    const hiResIcon = cardData.hires
        ? `<img src="../assets/HiRes.jpg" alt="Hi-Res Icon" class="hi-res-icon" width="22" height="22">`
        : '';

    const totalTracks = cardData.totalTracks
        ? `<div style="display: ruby">
                <h4>${cardData.totalTracks}</h4>
                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" class="bi bi-music-note" viewBox="0 0 16 16">
                    <path d="M9 13c0 1.105-1.12 2-2.5 2S4 14.105 4 13s1.12-2 2.5-2 2.5.895 2.5 2"/>
                    <path fill-rule="evenodd" d="M9 3v10H8V3z"/>
                    <path d="M8 2.82a1 1 0 0 1 .804-.98l3-.6A1 1 0 0 1 13 2.22V4L8 5z"/>
                </svg>
           </div>`
        : '';
    const maxQuality = cardData.maxQuality ? `${cardData.maxQuality}` : '';

    card.innerHTML = `
        <div class="card-content">
            <img src="${cardData.thumbnail}" alt="Thumbnail" class="result-thumbnail">
            <div class="result-info">
                <h3>${cardData.title}</h3>
                ${cardData.details}
            </div>
            <div class="extra-info">
                ${totalTracks}
                ${explicitIcon}
                ${hiResIcon}
                ${maxQuality}
            </div>
            <button class="download-button" data-url="${cardData.copyUrl}" data-type="${type}">
               <span class="fa-solid fa-download"></span>
            </button>
            <button class="play-button" data-url="${cardData.playUrl}" data-type="${type}">
               <span class="fa-solid fa-play"></span>
            </button>
            <button class="copy-button" data-url="${cardData.copyUrl}" data-type="${type}">
                <span class="fa-regular fa-copy"></span>
            </button>
        </div>
    `;

    const downloadBtn = card.querySelector('.download-button');
    if (downloadBtn) {
        downloadBtn.addEventListener('click', (event) => {
            const urlToDownload = downloadBtn.dataset.url;
            showQualitySelectionPopup(urlToDownload, platform, title);
        });
    }

    const copyBtn = card.querySelector('.copy-button');
    if (copyBtn) {
        copyBtn.addEventListener('click', () => {
            const urlToCopy = copyBtn.dataset.url;
            copyClipboard(urlToCopy);
        });
    }
    // Testing a clickable album browser from tracks menu (only for qobuz rn)
    const albumHref = card.querySelector('.album-href');
    if (albumHref) {
        albumHref.addEventListener('click', async () => {
            try {
                showLoadingOverlay();
                const albumId = albumHref.dataset.value;

                await showAlbumDetails(platform, albumId, result, type);
            } catch (error) {
                console.error('Error:', error);
            } finally {
                hideLoadingOverlay();
            }
        });
    }

    const playBtn = card.querySelector('.play-button');
    if (playBtn) {
        playBtn.addEventListener('click', async () => {
            try {
                showLoadingOverlay();

                if (type === "album") {
                    await showAlbumDetails(platform, playBtn.dataset.url, result, type);
                }
                else if (type === "playlist") {
                    await showPlaylistDetails(platform, playBtn.dataset.url, result, type);
                } else {
                    currentCardData = getCardData(result, platform, type);
                    await window.api.playMedia({
                        url: playBtn.dataset.url,
                        platform,
                        type,
                        id: result.id
                    });
                }
            } catch (error) {
                handleError(error);
                console.error("Error during media playback:", error);
            } finally {
                hideLoadingOverlay();
            }
        });
    }
    return card;
}

async function showAlbumDetails(platform, albumId, result, type) {
    try {
        showLoadingOverlay();

        const response = await window.electronAPI.getAlbumDetails(platform, albumId);

        if (!response.success) {
            throw new Error(response.error);
        }

        const data = response.data;
        let sortConfig = {
            column: 'number',
            direction: 'asc'
        };
        let searchTerm = '';

        const currentCardData = getCardData(result, platform, type);

        const overlay = document.createElement('div');
        overlay.className = 'popup-overlay';

        const popup = document.createElement('div');
        popup.className = 'album-popup';

        function getSortIcon(columnName) {
            if (sortConfig.column !== columnName) return '↕️';
            return sortConfig.direction === 'asc' ? '↑' : '↓';
        }

        function filterAndSortTracks() {
            return [...data.tracks]
                .filter(track => {
                    if (!searchTerm) return true;
                    const searchLower = searchTerm.toLowerCase();
                    return (
                        track.title.toLowerCase().includes(searchLower) ||
                        String(track.number).includes(searchLower) ||
                        (track.quality || '').toLowerCase().includes(searchLower)
                    );
                })
                .sort((a, b) => {
                    let comparison = 0;
                    switch (sortConfig.column) {
                        case 'number':
                            comparison = a.number - b.number;
                            break;
                        case 'title':
                            comparison = a.title.localeCompare(b.title);
                            break;
                        case 'duration':
                            comparison = a.duration - b.duration;
                            break;
                        case 'quality':
                            comparison = (a.quality || '').localeCompare(b.quality || '');
                            break;
                    }
                    return sortConfig.direction === 'asc' ? comparison : -comparison;
                });
        }

        function updatePopupContent() {
            popup.innerHTML = `
                <div class="album-info">
                    <img src="${currentCardData.thumbnail}" alt="${currentCardData.title}" class="album-cover">
                    <div class="album-details">
                        <h2 class="album-title">${currentCardData.title}</h2>
                        <p class="album-artist">${data.album.artist}</p>
                        <p class="album-date">Released ${data.album.releaseDate}</p>
                        ${data.album.description ? `<p class="album-description">${data.album.description}</p>` : ''}
                    </div>
                </div>
                <div class="tracks-section">
                    <div class="search-container">
                        <input type="text" 
                               class="search-input" 
                               placeholder="Search tracks..." 
                               value="${searchTerm}">
                        <span class="search-icon">🔍</span>
                    </div>
                    <div class="tracks-container">
                        ${renderTracks()}
                    </div>
                </div>
            `;
        }

        function renderTracks() {
            const filteredAndSortedTracks = filterAndSortTracks();

            if (filteredAndSortedTracks.length === 0) {
                return `
                    <div class="track-row track-header">
                        <div data-sort="number"># ${getSortIcon('number')}</div>
                        <div data-sort="title">Title ${getSortIcon('title')}</div>
                        <div data-sort="duration">Duration ${getSortIcon('duration')}</div>
                        <div data-sort="quality">Quality ${getSortIcon('quality')}</div>
                        <div>Actions</div>
                    </div>
                    <div class="no-results">No tracks found matching your search</div>
                `;
            }

            return `
                <div class="track-row track-header">
                    <div data-sort="number"># ${getSortIcon('number')}</div>
                    <div data-sort="title">Title ${getSortIcon('title')}</div>
                    <div data-sort="duration">Duration ${getSortIcon('duration')}</div>
                    <div data-sort="quality">Quality ${getSortIcon('quality')}</div>
                    <div>Actions</div>
                </div>
                ${filteredAndSortedTracks.map(track => `
                    <div class="track-row">
                        <div>${track.number}</div>
                        <div class="track-title">${track.title}</div>
                        <div>${formatDuration(track.duration)}</div>
                        <div>${track.quality || 'N/A'}</div>
                        <div class="button-container">
                            <button class="play-button" data-url="${track.playUrl}">
                                <span class="fa-solid fa-play"></span>
                            </button>
                            <button class="download-btn" data-track-id="${track.playUrl}">
                                <span class="fa-solid fa-download"></span>
                            </button>
                            <button class="copy-btn" data-track-id="${track.playUrl}">
                                <span class="fa-solid fa-copy"></span>
                            </button>
                        </div>
                    </div>
                `).join('')}
            `;
        }

        // Initialize popup content
        updatePopupContent();

        // Event handlers
        popup.addEventListener('click', async (e) => {
            const playButton = e.target.closest('.play-button');
            const downloadButton = e.target.closest('.download-btn');
            const copyButton = e.target.closest('.copy-btn');
            const sortHeader = e.target.closest('[data-sort]');
            const title = e.target.closest('.track-title');

            if (playButton) {
                try {
                    const url = playButton.dataset.url;
                    await window.api.playMedia({
                        url,
                        platform,
                        type: 'track',
                        id: playButton.closest('.track-row').querySelector('.download-btn').dataset.trackId
                    });
                    window.api.onStreamReady(({ streamUrl, platform }) => {
                        console.log('Stream Ready Data:', { streamUrl, platform });
                        console.log('Current Card Data:', currentCardData);
                        console.log('Album Data:', data.album);

                        const filteredAndSortedTracks = filterAndSortTracks();
                        const currentTrack = filteredAndSortedTracks.find(t => t.playUrl === streamUrl);
                        console.log('Found Track:', currentTrack);

                        if (currentTrack) {
                            const mediaData = {
                                streamUrl,
                                platform,
                                thumbnail: currentCardData.thumbnail,
                                title: currentTrack.title,
                                artist: data.album.artist,
                                album: currentCardData.title
                            };
                            console.log('Media Data Being Sent:', mediaData);
                            handleStreamReady(mediaData);
                        }
                    });
                } catch (error) {
                    handleError(error);
                    console.error("Error during media playback:", error);
                }
            }
            else if (downloadButton) {
                const trackId = downloadButton.dataset.trackId;
                console.log('Download clicked for track:', trackId);
                showQualitySelectionPopup(trackId, platform, title)
            }
            else if (copyButton) {
                const downUrl = copyButton.dataset.trackId;
                console.log('Sending', downUrl, 'to clipboard');
                await window.electronAPI.copyText(downUrl);

            }
            else if (sortHeader) {
                const column = sortHeader.dataset.sort;
                if (sortConfig.column === column) {
                    sortConfig.direction = sortConfig.direction === 'asc' ? 'desc' : 'asc';
                } else {
                    sortConfig.column = column;
                    sortConfig.direction = 'asc';
                }
                updatePopupContent();
            }
        });

        popup.addEventListener('input', (e) => {
            if (e.target.classList.contains('search-input')) {
                searchTerm = e.target.value;
                updatePopupContent();
            }
        });

        overlay.addEventListener('click', (e) => {
            if (e.target === overlay) {
                overlay.remove();
            }
        });

        overlay.appendChild(popup);
        document.body.appendChild(overlay);

    } catch (error) {
        handleError(error);
        console.error('Error showing album details:', error);
    } finally {
        hideLoadingOverlay();
    }
}

async function showPlaylistDetails(platform, playlistId, result, type) {
    try {
        const response = await window.electronAPI.getPlaylistDetails(platform, playlistId);

        if (!response.success) {
            throw new Error(response.error);
        }

        const data = response.data;
        let sortConfig = {
            column: 'number',
            direction: 'asc'
        };
        let searchTerm = '';

        // Create popup container
        const overlay = document.createElement('div');
        overlay.className = 'popup-overlay';

        const popup = document.createElement('div');
        popup.className = 'playlist-popup';
        currentCardData = getCardData(result, platform, type);

        function getSortIcon(columnName) {
            if (sortConfig.column !== columnName) return '↕️';
            return sortConfig.direction === 'asc' ? '↑' : '↓';
        }

        function filterAndSortTracks() {
            return [...data.tracks]
                .filter(track => {
                    if (!searchTerm) return true;
                    const searchLower = searchTerm.toLowerCase();
                    return (
                        track.title.toLowerCase().includes(searchLower) ||
                        String(track.number).includes(searchLower) ||
                        (track.quality || '').toLowerCase().includes(searchLower)
                    );
                })
                .sort((a, b) => {
                    let comparison = 0;
                    switch (sortConfig.column) {
                        case 'number':
                            comparison = a.number - b.number;
                            break;
                        case 'title':
                            comparison = a.title.localeCompare(b.title);
                            break;
                        case 'duration':
                            comparison = a.duration - b.duration;
                            break;
                        case 'quality':
                            comparison = (a.quality || '').localeCompare(b.quality || '');
                            break;
                    }
                    return sortConfig.direction === 'asc' ? comparison : -comparison;
                });
        }

        function renderTracks() {
            const filteredAndSortedTracks = filterAndSortTracks();

            if (filteredAndSortedTracks.length === 0) {
                return `
            <div class="track-row track-header">
                <div data-sort="number"># ${getSortIcon('number')}</div>
                <div></div>
                <div></div>
                <div data-sort="title">Title ${getSortIcon('title')}</div>
                <div data-sort="duration">Duration ${getSortIcon('duration')}</div>
                <div data-sort="quality">Quality ${getSortIcon('quality')}</div>
                <div>Actions</div>
            </div>
            <div class="no-results">No tracks found matching your search</div>
        `;
            }

            return `
        <div class="track-row track-header">
            <div data-sort="number"># ${getSortIcon('number')}</div>
            <div></div>
            <div></div>
            <div data-sort="title">Title ${getSortIcon('title')}</div>
            <div data-sort="duration">Duration ${getSortIcon('duration')}</div>
            <div data-sort="quality">Quality ${getSortIcon('quality')}</div>
            <div>Actions</div>
        </div>
        ${filteredAndSortedTracks.map(track => `
            <div class="track-row">
                <div>${track.number}</div>
                <div><img src="${track.cover}" alt="Track cover"/></div>
                <div>${track.explicit ? '<span class="explicit-badge">E</span>' : ''}</div>
                <div class="track-info">
                    <h2>${track.title}</h2>
                    <div class="album-info">
                        ${track.albumTitle ? `<div>${track.albumTitle}</div>` : ''}
                        ${track.albumArtist ? `<div>${track.albumArtist}</div>` : ''}
                    </div>
                </div>
                <div>${formatDuration(track.duration)}</div>
                <div>${track.quality || 'N/A'}</div>
                <div class="button-container">
                    <button class="btn play-btn" data-track-id="${track.id}">
                        <span class="fa-solid fa-play"></span>
                    </button>
                    <button class="btn download-btn" data-track-id="${track.id}">
                        <span class="fa-solid fa-download"></span>
                    </button>
                    <button class="btn copy-btn" data-track-id="${track.id}">
                        <span class="fa-solid fa-copy"></span>
                    </button>
                </div>
            </div>
        `).join('')}
    `;
        }
        function updatePopupContent() {
            popup.innerHTML = `
                <div class="playlist-info">
                    <img src="${currentCardData.thumbnail}" alt="${currentCardData.title}" class="playlist-cover">
                    <div class="playlist-details">
                        <h2 class="playlist-title">${currentCardData.title}</h2>
                        <p class="playlist-artist">${data.playlist.artist}</p>
                        <p class="playlist-date">Released ${data.playlist.releaseDate}</p>
                        ${data.playlist.description ? `<p class="playlist-description">${data.playlist.description}</p>` : ''}
                    </div>
                </div>
                <div class="tracks-section">
                    <div class="search-container">
                        <input type="text" 
                               class="search-input" 
                               placeholder="Search tracks..." 
                               value="${searchTerm}"
                        >
                        <span class="search-icon">🔍</span>
                    </div>
                    <div class="tracks-container">
                        ${renderTracks()}
                    </div>
                </div>
            `;
        }

        updatePopupContent();

        popup.addEventListener('input', (e) => {
            if (e.target.classList.contains('search-input')) {
                searchTerm = e.target.value;
                const tracksContainer = popup.querySelector('.tracks-container');
                tracksContainer.innerHTML = renderTracks();
            }
        });

        popup.addEventListener('click', async (e) => {
            if (e.target.closest('[data-sort]')) {
                const column = e.target.closest('[data-sort]').dataset.sort;
                if (sortConfig.column === column) {
                    sortConfig.direction = sortConfig.direction === 'asc' ? 'desc' : 'asc';
                } else {
                    sortConfig.column = column;
                    sortConfig.direction = 'asc';
                }
                const tracksContainer = popup.querySelector('.tracks-container');
                tracksContainer.innerHTML = renderTracks();
            }

            if (e.target.classList.contains('play-btn')) {
                const trackId = e.target.dataset.trackId;
                try {
                    await window.api.playMedia({
                        url: data.tracks.find(t => t.id === trackId)?.playUrl,
                        platform,
                        type: 'track',
                        id: trackId
                    });
                } catch (error) {
                    console.error('Error playing track:', error);
                }
            }

            if (e.target.classList.contains('download-btn')) {
                const trackId = e.target.dataset.trackId;
                console.log('Download clicked for track:', trackId);
            }
        });

        // Close popup when clicking overlay
        overlay.addEventListener('click', (e) => {
            if (e.target === overlay) {
                overlay.remove();
            }
        });

        // Add to document
        overlay.appendChild(popup);
        document.body.appendChild(overlay);

    } catch (error) {
        console.error('Error showing playlist details:', error);
    }
}

function formatDuration(seconds) {
    if (!seconds) return '--:--';
    const minutes = Math.floor(seconds / 60);
    const remainingSeconds = Math.floor(seconds % 60);
    return `${minutes}:${remainingSeconds.toString().padStart(2, '0')}`;
}

// Export for use in your renderer process
window.showAlbumDetails = showAlbumDetails;
window.showPlaylistDetails = showPlaylistDetails;
async function playTrackAtIndex(index) {
    if (index < 0 || index >= playlist.length) {
        console.error("Index out of bounds");
        return;
    }

    const { result, platform, type } = playlist[index];

    currentCardData = getCardData(result, platform, type);

    showLoadingOverlay();

    try {
        await window.api.playMedia({
            url: currentCardData.playUrl,
            platform,
            type,
            id: result.id
        });
    } catch (error) {
        handleError(error);
        hideLoadingOverlay();
    }
}

// Quality options for downloading on search.
const qualityOptions = {
    youtube: [
        { value: "bestvideo+bestaudio", label: "Best Quality" },
        { value: "bestvideo[height<=2160][fps>30]+bestaudio", label: "4K60" },
        { value: "bestvideo[height<=2160]+bestaudio", label: "4K" },
        { value: "bestvideo[height<=1440][fps>30]+bestaudio", label: "2K60" },
        { value: "bestvideo[height<=1440]+bestaudio", label: "2K" },
        { value: "bestvideo[height<=1080][fps>30]+bestaudio", label: "1080p60" },
        { value: "bestvideo[height<=1080]+bestaudio", label: "1080p" },
        { value: "bestvideo[height<=720][fps>30]+bestaudio", label: "720p60" },
        { value: "bestvideo[height<=720]+bestaudio", label: "720p" },
        { value: "bestvideo[height<=480]+bestaudio", label: "480p" },
        { value: "bestvideo[height<=360]+bestaudio", label: "360p" },
        { value: "bestvideo[height<=240]+bestaudio", label: "240p" },
        { value: "bestvideo[height<=144]+bestaudio", label: "144p" },
    ],
    youtubeMusic: [
        { value: "0", label: "Best" },
        { value: "5", label: "Meh" },
        { value: "9", label: "Worst" }
    ],
    qobuz: [
        { value: "4", label: "24 bit, ≤ 192 kHz" },
        { value: "3", label: "24 bit, ≤ 96 kHz" },
        { value: "2", label: "16 bit, 44.1 kHz (CD)" },
        { value: "1", label: "320 kbps MP3" }
    ],
    tidal: [
        { value: "3", label: "24 bit, ≤ 96 kHz (MQA)" },
        { value: "2", label: "16 bit, 44.1 kHz (CD)" },
        { value: "1", label: "320 kbps AAC" },
        { value: "0", label: "128 kbps AAC" }
    ],
    deezer: [
        { value: "2", label: "16 bit, 44.1 kHz (CD)" },
        { value: "1", label: "320 kbps MP3" },
        { value: "0", label: "128 kbps MP3" }
    ],
    spotify: [
        { value: "auto", label: "Best"},
        { value: "very_high", label: "Very High" },
        { value: "high", label: "High" },
        { value: "normal", label: "Normal" }
    ],
    appleMusic: [
        { value: "aac-legacy", label: "256 kbps" },
        { value: "aac-he-legacy", label: "64 kbps" }
    ]
};

function showQualitySelectionPopup(url, platform, title) {
    const overlay = document.createElement('div');
    overlay.className = 'popup-overlay';

    const popup = document.createElement('div');
    popup.className = 'quality-popup';

    const options = qualityOptions[platform] || [];

    let optionsHTML = options.map(option => `
        <option value="${option.value}">${option.label}</option>
    `).join("");

    popup.innerHTML = `
        <div class="quality-popup-content">
            <h3>Select Quality for "${title}"</h3>
            <select class="quality-dropdown">
                ${optionsHTML}
            </select>
            <button class="confirm-download">Download</button>
            <button class="close-popup">Cancel</button>
        </div>
    `;

    overlay.appendChild(popup);
    document.body.appendChild(overlay);

    popup.querySelector('.confirm-download').addEventListener('click', () => {
        const selectedQuality = popup.querySelector('.quality-dropdown').value;
        handlePopupDownload(url, platform, selectedQuality);
        document.body.removeChild(overlay);
    });

    popup.querySelector('.close-popup').addEventListener('click', () => {
        document.body.removeChild(overlay);
    });
}

// Protocol Listener
window.protocolHandler.onProtocolAction((data) => {
    const { url, platform, title } = data;
    showQualitySelectionPopup(url, platform,title);
});

function handlePopupDownload(url, platform, quality) {
    try {
        if (!url) throw new Error('Download URL is required.');

        // Send the download
        switch (platform) {
            case 'qobuz':
                window.electronAPI.send('start-qobuz-download', { url, quality });
                break;
            case 'tidal':
                window.electronAPI.send('start-tidal-download', { url, quality });
                break;
            case 'deezer':
                window.electronAPI.send('start-deezer-download', { url, quality });
                break;
            case 'spotify':
                window.electronAPI.send('start-spotify-download', { url, quality });
                break;
            case 'apple':
                window.electronAPI.send('start-apple-download', { url, quality });
                break;
            case 'youtubeMusic':
                window.electronAPI.send('start-yt-music-download', { url, quality, isPlaylist: false });
                break;
            case 'youtube':
                window.electronAPI.send('start-yt-video-download', { url, quality, isPlaylist: false });
                break;
            default:
                throw new Error(`Unsupported platform: ${platform}`);
        }
    } catch (error) {
        console.error(`Error starting download on ${platform}:`, error);
        displayErrorNotification(`Error starting download on ${platform}: ${error.message}`);
    }
}

function copyClipboard(url) {
    if (!url) {
        showNotification('No URL')
        return;
    }

    navigator.clipboard.writeText(url)
        .then(() => {
            showNotification('URL copied to clipboard')
        })
        .catch(err => {
            showNotification('Error copying clipboard:', err);
        });
}

function handleStreamReady({ streamUrl, platform, thumbnail, title, artist, album }) {
    hideLoadingOverlay();

    // Remove any existing media element before adding a new one
    const existingMedia = document.getElementById('media-element');
    if (existingMedia) {
        existingMedia.pause();
        existingMedia.src = '';
        existingMedia.remove();
    }

    const isVideo = platform === 'youtube';
    const mediaElement = document.createElement(isVideo ? 'video' : 'audio');
    mediaElement.id = 'media-element';
    mediaElement.src = streamUrl;
    mediaElement.style.display = 'none';
    document.body.appendChild(mediaElement);

    // Update player info
    document.getElementById('player-thumbnail').src = thumbnail || '';
    document.getElementById('player-title').textContent = title || 'Unknown Title';
    document.getElementById('player-artist').textContent = artist || 'Unknown Artist';

    // Autoplay
    mediaElement.addEventListener('loadeddata', () => {
        mediaElement.play().catch(error => {
            console.error('Autoplay failed:', error);
        });
    });

    // send media to OS
    if ('mediaSession' in navigator) {
        navigator.mediaSession.metadata = new MediaMetadata({
            title: title || 'Unknown Title',
            artist: artist || 'Unknown Artist',
            album: album,
            artwork: [
                { src: thumbnail || 'default-thumbnail.jpg', sizes: '512x512', type: 'image/jpg' }
            ]
        });

        navigator.mediaSession.setActionHandler('play', () => mediaElement.play());
        navigator.mediaSession.setActionHandler('pause', () => mediaElement.pause());
        navigator.mediaSession.setActionHandler('stop', () => mediaElement.pause());
        navigator.mediaSession.setActionHandler('seekbackward', (details) => {
            mediaElement.currentTime = Math.max(mediaElement.currentTime - (details.seekOffset || 10), 0);
        });
        navigator.mediaSession.setActionHandler('seekforward', (details) => {
            mediaElement.currentTime = Math.min(mediaElement.currentTime + (details.seekOffset || 10), mediaElement.duration);
        });

        navigator.mediaSession.setActionHandler('previoustrack', () => {
            currentIndex--;
            playTrackAtIndex(currentIndex);

        });

        navigator.mediaSession.setActionHandler('nexttrack', () => {
            if (currentIndex < playlist.length - 1) {
                currentIndex++;
                playTrackAtIndex(currentIndex);
            } else {
                currentIndex = 0;
                playTrackAtIndex(currentIndex);
            }
        });

    }
    initializePlayerControls(mediaElement, isVideo);
}


function initializePlayerControls(mediaElement, isVideo) {
    const playPauseBtn = document.getElementById('play-pause-btn');
    const muteBtn = document.getElementById('mute-btn');
    const volumeSlider = document.getElementById('volume-slider');
    const fullscreenBtn = document.getElementById('fullscreen-btn');
    const progress = document.getElementById('progress');
    const progressBar = document.querySelector('.progress-bar');
    const currentTimeSpan = document.getElementById('current-time');
    const durationSpan = document.getElementById('duration');
    const prevBtn = document.getElementById('prev-btn');
    const nextBtn = document.getElementById('next-btn');

    nextBtn.onclick = () => {
        if (currentIndex < playlist.length - 1) {
            currentIndex++;
            playTrackAtIndex(currentIndex);
        } else {
            currentIndex = 0;
            playTrackAtIndex(currentIndex);
        }
    };
    prevBtn.onclick = () => {
        if (currentIndex > 0) {
            currentIndex--;
            playTrackAtIndex(currentIndex);
        } else {
            currentIndex = playlist.length - 1;
            playTrackAtIndex(currentIndex);
        }
    };

    // Auto-play next track when current track ends
    mediaElement.addEventListener('ended', () => {
        if (currentIndex < playlist.length - 1) {
            currentIndex++;
            playTrackAtIndex(currentIndex);
        }
    });

    if (playPauseBtn) {
        playPauseBtn.innerHTML = '<i class="fas fa-pause"></i>';
    }
    mediaElement.addEventListener('playing', () => {
        if (playPauseBtn) {
            playPauseBtn.innerHTML = '<i class="fas fa-pause"></i>';
        }
    });

    mediaElement.addEventListener('ended', () => {
        if (playPauseBtn) {
            playPauseBtn.innerHTML = '<i class="fas fa-play"></i>';
        }
    });
    playPauseBtn.onclick = () => {
        if (mediaElement.paused) {
            mediaElement.play();
            playPauseBtn.innerHTML = '<i class="fas fa-pause"></i>';
        } else {
            mediaElement.pause();
            playPauseBtn.innerHTML = '<i class="fas fa-play"></i>';
        }
    };

    muteBtn.onclick = () => {
        mediaElement.muted = !mediaElement.muted;
        muteBtn.innerHTML = mediaElement.muted ?
            '<i class="fas fa-volume-mute"></i>' :
            '<i class="fas fa-volume-up"></i>';
    };

    volumeSlider.oninput = (e) => {
        mediaElement.volume = e.target.value;
        if (e.target.value === 0) {
            muteBtn.innerHTML = '<i class="fas fa-volume-mute"></i>';
        } else {
            muteBtn.innerHTML = '<i class="fas fa-volume-up"></i>';
        }
    };

    mediaElement.ontimeupdate = () => {
        const percentage = (mediaElement.currentTime / mediaElement.duration) * 100;
        progress.style.width = percentage + '%';
        currentTimeSpan.textContent = formatTime(mediaElement.currentTime);
    };

    mediaElement.onloadedmetadata = () => {
        durationSpan.textContent = formatTime(mediaElement.duration);
    };

    progressBar.onclick = (e) => {
        const rect = progressBar.getBoundingClientRect();
        const pos = (e.clientX - rect.left) / progressBar.offsetWidth;
        mediaElement.currentTime = pos * mediaElement.duration;
    };

    // Fullscreen (only for youtube videos)
    if (isVideo) {
        fullscreenBtn.style.display = 'block';
        fullscreenBtn.onclick = () => {
            if (!document.fullscreenElement) {
                mediaElement.style.display = 'block';
                mediaElement.requestFullscreen();
            } else {
                document.exitFullscreen();
            }
        };

        document.onfullscreenchange = () => {
            if (!document.fullscreenElement) {
                mediaElement.style.display = 'none';
            }
        };
    } else {
        fullscreenBtn.style.display = 'none';
    }
}

function formatTime(seconds) {
    const minutes = Math.floor(seconds / 60);
    seconds = Math.floor(seconds % 60);
    return `${minutes}:${seconds.toString().padStart(2, '0')}`;
}

function getCardData(result, platform, type = 'track') {
    switch (platform) {
        case 'youtube':
            switch (type){
                case 'video':
                    return {
                        thumbnail: result.Thumbnail,
                        title: result['Video Title'],
                        details: `<p>Channel: ${result['Channel Title']}</p>`,
                        playUrl: result['Video URL'],
                        copyUrl: result['Video URL']
                    };
                case 'playlist':
                    return {
                        thumbnail: result.Thumbnail,
                        title: result['Playlist Title'],
                        details: `<p>Channel: ${result['Channel Title']}</p>`,
                        playUrl: result.BrowseId,
                        copyUrl: result['Playlist URL']
                    };
                case 'channel':
                    return {
                        thumbnail: result.Thumbnail,
                        title: result['Channel Title'],
                        details: `<p>Channel ID: ${result['Channel ID']}</p>`,
                        playUrl: 'WIP',
                        copyUrl: result['Channel URL'],
                    }
            }
            break;
        case 'youtubeMusic':
            switch (type){
                case "album":
                    return {
                        thumbnail: result.AlbumCover,
                        title: result.AlbumTitle,
                        details: `
                        <p>Artist: ${result.ArtistName}</p>`,
                        artist: result.ArtistName,
                        album: result.AlbumTitle,
                        explicit: result.Explicit,
                        playUrl: result.BrowseId,
                        copyUrl: result.AlbumURL,
                    };
                case "playlist":
                    return {
                        thumbnail: result.PlaylistCover,
                        title: result.PlaylistTitle,
                        details: `
                        <p>${result.Author}`,
                        explicit: result.Explicit,
                        playUrl: result.BrowseId,
                        copyUrl: result.PlaylistURL
                    };
                case "song":
                    return {
                        thumbnail: result.AlbumCover,
                        title: result.TrackTitle,
                        details: `
                    <p>Album: ${result.AlbumTitle}</p>
                    <p>Artist: ${result.ArtistName}</p>
                `,
                        artist: result.ArtistName,
                        album: result.AlbumTitle,
                        explicit: result.Explicit,
                        playUrl: result.TrackURL,
                        copyUrl: result.TrackURL
                    };
                case "podcast":
                    return {
                        thumbnail: result.PodcastCover,
                        title: result.PodcastTitle,
                        details: '<p></p>'
                    }
                case "episode":
                    return {
                        thumbnail: result.EpisodeCover,
                        title: result.EpisodeTitle,
                        details: `<p>${result.Podcast}</p>`,
                        artist: result.Podcast,
                        playUrl: result.EpisodeURL,
                        copyUrl: result.EpisodeURL
                    }
                case "artist":
                    return {
                        thumbnail: result.ArtistCover,
                        title: result.ArtistName,
                        details: `<p></p>`,
                        playUrl: result.ArtistURL,
                        copyUrl: result.ArtistURL
                    }
            }
            break;
        case 'spotify':
            switch(type) {
                case 'track':
                    return {
                        thumbnail: result.album?.images[0]?.url || result.Thumbnail,
                        title: result.name,
                        details: `
                            <p>Album: ${result.album?.name || 'Unknown Album'}</p>
                            <p>Artist: ${result.artists[0]?.name || 'Unknown Artist'}</p>
                        `,
                        artist: result.artists[0]?.name,
                        album: result.album?.name,
                        explicit: result.explicit,
                        playUrl: result.preview_url,
                        copyUrl: result.external_urls.spotify
                    };
                case 'album':
                    return {
                        thumbnail: result.images[0]?.url || '',
                        title: result.name,
                        details: `<h4>${result.release_date}</h4><p>Artist: ${result.artists[0]?.name || 'Unknown Artist'}</p>`,
                        totalTracks: result.total_tracks,
                        playUrl: result.id,
                        copyUrl: result.external_urls.spotify
                    };
                case 'artist':
                    return {
                        thumbnail: result.images[0]?.url || '',
                        title: result.name,
                        details: `<p>Followers: ${result.followers?.total || 0}</p>`,
                        playUrl: `WIP`,
                        copyUrl: result.external_urls.spotify
                    };
                case 'playlist':
                    return {
                        thumbnail: result?.images?.[0]?.url || '',
                        title: result?.name,
                        details: `<p>By: ${result?.owner?.display_name || 'Unknown'}</p>`,
                        playUrl: result?.id,
                        copyUrl: result?.external_urls.spotify
                    };
                case 'podcast':
                    return {
                        thumbnail:  result.images[0]?.url || '',
                        title: result.name,
                        details: `<p>By: ${result.publisher}</p>`,
                        copyUrl: result.external_urls.spotify
                    };
                case 'episode':
                    return {
                        thumbnail: result.images[0].url,
                        title: result.name,
                        details: `<p>${result.release_date}</p>`,
                        copyUrl: result.external_urls.spotify
                    }
            }
            break;
        case 'deezer':
            switch(type) {
                case 'track':
                    return {
                        thumbnail: result.album?.cover_medium,
                        title: result.title_short,
                        details: `
                            <p>Album: ${result.album?.title || 'Unknown Album'}</p>
                            <p>Artist: ${result.artist?.name || 'Unknown Artist'}</p>
                        `,
                        artist: result.artist?.name,
                        album: result.album?.name,
                        explicit: result.explicit_lyrics,
                        playUrl: result.preview,
                        copyUrl: result.link
                    };
                case 'album':
                    return {
                        thumbnail: result.cover_medium,
                        title: result.title,
                        details: `<p>Artist: ${result.artist?.name || 'Unknown Artist'}</p>`,
                        explicit: result.explicit_lyrics,
                        totalTracks: result.nb_tracks,
                        playUrl: result.id,
                        copyUrl: result.link
                    };
                case 'artist':
                    return {
                        thumbnail: result.picture_medium,
                        title: result.name,
                        details: `<p>Fans: ${result.nb_fan || 0}</p>`,
                        playUrl: result.id,
                        copyUrl: result.link
                    };
                case 'playlist':
                    return {
                        thumbnail: result.picture_medium,
                        title: result.title,
                        details: `<p>Tracks: ${result.nb_tracks || 0}</p>`,
                        playUrl: result.id,
                        copyUrl: result.link
                    };
            }
            break;
        case 'qobuz':
            switch (type) {
                case 'track':
                    return {
                        thumbnail: result.album?.image?.small || result.Thumbnail,
                        title: result.title || 'Unknown Title',
                        details: `
                    <p style="font-size: 0.9em;">${result.maximum_bit_depth}-Bit/${result.maximum_sampling_rate} kHz</p>
                    <p data-value="${result.album?.id || ''}" id="album-href" class="album-href">Album: ${result.album?.title || 'Unknown Album'}</p>
                    <p>Artist: ${result.album?.artist?.name || 'Unknown Artist'}</p>
                `,
                        artist: result.album?.artist?.name,
                        album: result.album?.album?.name,
                        explicit: result.parental_warning,
                        hires: result.hires,
                        playUrl: `https://play.qobuz.com/track/${result.id}`,
                        copyUrl: `https://play.qobuz.com/track/${result.id}`
                    };
                case 'album':
                    return {
                        thumbnail: result.image?.small || result.Thumbnail,
                        title: result.title || 'Unknown Title',
                        details: `
                    <p style="font-size: 0.9em;">${result.maximum_bit_depth}-Bit/${result.maximum_sampling_rate} kHz</p>
                    <p>${result.release_date_original}</p>
                    <p>Artist: ${result.artist?.name || 'Unknown Artist'}</p>
                `,
                        artist: result.album?.artist?.name,
                        album: result.album?.album?.name,
                        explicit: result.parental_warning,
                        hires: result.hires,
                        totalTracks: `${result.tracks_count}/${result.media_count} `,
                        playUrl: result.id,
                        copyUrl: `https://play.qobuz.com/album/${result.id}`,
                    };
                case 'playlist':
                    return {
                        thumbnail: result.image_rectangle[0],
                        title: result.name,
                        details: `<p>${result.genres[0].percent}% ${result.genres[0].name}</p>`,
                        playUrl: result.id,
                        copyUrl: `https://play.qobuz.com/playlist/${result.id}`,
                    }
                case 'artist':
                    return {
                        thumbnail: result.image?.medium || 'https://www.qobuz.com/assets-static/img/common/default_artist.svg'|| result.image.small,
                        title: result.name,
                        details: `<p/>`,
                        playUrl: `WIP`,
                        copyUrl: `https://play.qobuz.com/artist/${result.id}`
                    }
            }
            break;
        case 'tidal':
            switch (type) {
                case 'track':
                    return {
                        thumbnail: result.album.imageCover[2].url, // Using the 160x160 version
                        title: result.title,
                        details: `
                    <p>${result.mediaMetadata?.tags?.[0] || ''}</p>
                    <p>Album: ${result.album?.title || 'Unknown Album'}</p>
                    <p>Artist: ${result.artists?.[0]?.name || 'Unknown Artist'}</p>
                `,
                        album: result.album?.title || 'Unknown Album',
                        artist: result.artists?.[0]?.name || 'Unknown Artist',
                        explicit: result.properties?.content ? result.properties.content[0] : null,
                        hires: result.mediaMetadata?.tags?.includes("HIRES_LOSSLESS") ? true : null,
                        playUrl: result.tidalUrl,
                        copyUrl: result.tidalUrl
                    };
                case 'album':
                    return {
                        thumbnail: result.imageCover[2].url, // Using the 160x160 version
                        title: result.title,
                        details: `
                    <p>${result.mediaMetadata?.tags[0]}</p>
                    <p>Artist: ${result.artists[0]?.name || 'Unknown Artist'}</p>
                    <p>Release: ${result.releaseDate}</p>
                `,
                        explicit: result.properties?.content ? result.properties.content[0] : null,
                        totalTracks: `${result.numberOfTracks}/${result.numberOfVolumes}`,
                        playUrl: result.id,
                        copyUrl: result.tidalUrl
                    };
                case 'artist':
                    return {
                        thumbnail: findFirstSquareImage(result.picture),
                        title: result.name,
                        details: ` <p/>
                `,
                        playUrl: "WIP",
                        copyUrl: result.tidalUrl
                    };
                case 'video':
                    return {
                        thumbnail: findFirstSquareImage(result.image),
                        title: result.title,
                        details: `<p>Artist: ${result.artists[0].name}</p>`,
                        playUrl: result.tidalUrl,
                        copyUrl: result.tidalUrl
                    }
            }
            break;
        case 'applemusic':
            switch (type) {
                case 'track':
                    return {
                        thumbnail: result.artworkUrl100 || result.artworkUrl60,
                        title: result.trackName,
                        details: `
                            <p>Album: ${result.collectionName || 'Unknown Album'}</p>
                            <p>Artist: ${result.artistName || 'Unknown Artist'}</p>
                            <p>Genre: ${result.primaryGenreName || 'Unknown Genre'}</p>
                        `,
                        artist: result.artistName,
                        album: result.collectionName,
                        explicit: result.trackExplicitness === "explicit" ? true : (result.trackExplicitness === "notExplicit" ? false : null),
                        playUrl: result.trackViewUrl,
                        copyUrl: result.trackViewUrl
                    };
                case 'album':
                    return {
                        thumbnail: result.artworkUrl100 || result.artworkUrl60,
                        title: result.collectionName,
                        details: `
                            <p>Artist: ${result.artistName || 'Unknown Artist'}</p>
                            <p>Genre: ${result.primaryGenreName || 'Unknown Genre'}</p>
                            <p>Release Date: ${new Date(result.releaseDate).toLocaleDateString()}</p>
                        `,
                        explicit: result.trackExplicitness === "explicit" ? true : (result.trackExplicitness === "notExplicit" ? false : null),
                        playUrl: result.collectionViewUrl,
                        copyUrl: result.collectionViewUrl
                    };
                case 'artist':
                    return {
                        thumbnail: 'https://tidal.com/browse/assets/images/defaultImages/defaultArtistImage.png', // Apple Music API doesn't provide an artist image in the search API
                        title: result.artistName,
                        details: `<p>Genre: ${result.primaryGenreName || 'Unknown Genre'}</p>`,
                        playUrl: result.artistLinkUrl,
                        copyUrl: result.artistLinkUrl
                    };
                default:
                    return {
                        thumbnail: '',
                        title: 'Unknown Title',
                        details: '',
                        playUrl: '#'
                    };
            }
            break;
        default:
            return {
                thumbnail: '',
                title: 'Unknown Title',
                details: '',
                playUrl: '#'
            };
    }
}
const findFirstSquareImage = (pictures) => {
    return pictures.find(pic => pic.width === pic.height)?.url || 'https://tidal.com/browse/assets/images/defaultImages/defaultArtistImage.png';
}

function handleError(error) {
    showNotification(`Error: ${error.message}`);
}

function showNotification(message) {
    let container = document.getElementById('floating-download-notifications') ||
        document.getElementById('floating-search-notifications') || document.getElementById('floating-notifications');

    if (!container) return;

    const notification = document.createElement('div');
    notification.className = 'notification';
    notification.textContent = message;

    container.appendChild(notification);

    setTimeout(() => {
        notification.remove();
    }, 3000);
}

function displayCompletionNotification(message) {
    const container = document.getElementById('floating-download-notifications');

    if (typeof message === 'object') {
        message = `Download complete: ${message.order || 'Unknown file'}`;
    }

    // Create a new notification element
    const notification = document.createElement('div');
    notification.classList.add('download-notification', 'completion');

    notification.innerHTML = `
        <div class="notification-header">
            <h3>Download Complete</h3>
            <button class="close-btn" onclick="removeCompletionNotification(this)">x</button>
        </div>
        <div class="notification-body">${message}</div>
    `;

    container.appendChild(notification);

    setTimeout(() => {
        removeCompletionNotification(notification);
    }, 5000);
}

function removeCompletionNotification(element) {
    if (element) {
        element.remove();
    }
}

// Listener for completion events
window.electronAPI.receive('download-complete', (message) => {
    displayCompletionNotification(message);
});


function initializeVideoTab() {
    const tabButtons = document.querySelectorAll('.tab-button');
    const tabContents = document.querySelectorAll('.tab-content');

    tabButtons.forEach(button => {
        button.addEventListener('click', () => {
            document.querySelector('.tab-button.active').classList.remove('active');
            document.querySelector('.tab-content.active').classList.remove('active');

            button.classList.add('active');
            document.getElementById(button.dataset.tab).classList.add('active');
        });
    });

    // Initialize dropdowns
    initializeDropdowns();

    const platformBoxes = ['youtube', 'generic'];

    platformBoxes.forEach(platform => {
        const input = document.getElementById(`${platform}-url`);
        const downloadButton = document.getElementById(`${platform}-download-button`);

        if (input && downloadButton) {
            input.addEventListener('keypress', (event) => {
                if (event.key === 'Enter') {
                    downloadButton.click();
                }
            });
        }
    });

    // Add event listeners for download buttons
    document.getElementById('youtube-download-button').addEventListener('click', handleYoutubeVideoDownload);
    document.getElementById('generic-download-button').addEventListener('click', handleGenericDownload);

    // Initialize the site search dropdown
    initializeSiteSearch();

    // Initialize download container
    renderDownloads();
}
// Video Tab
function handleYoutubeVideoDownload() {
    const url = document.getElementById('youtube-url').value;
    const isPlaylist = document.getElementById('youtube-video-playlist').checked;
    const qualityDropdown = document.getElementById('youtube-quality');
    const quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || 'bestvideo+bestaudio/best';
    window.electronAPI.send('start-yt-video-download', { url, quality, isPlaylist });
}

function handleGenericDownload() {
    const url = document.getElementById('generic-url').value;
    const qualityDropdown = document.getElementById('generic-quality');
    const quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || 'bestvideo+bestaudio/best';
    window.electronAPI.send('start-generic-video-download', { url, quality });
}

function initializeSiteSearch() {
    const siteDropdown = document.getElementById('generic-site');
    const dropdownContent = siteDropdown.querySelector('.dropdown-content');
    const siteList = document.getElementById('site-list');
    const searchInput = document.getElementById('site-search');
    const dropdownBtn = siteDropdown.querySelector('.dropdown-btn');

    let allSites = [];

    fetch('https://raw.githubusercontent.com/yt-dlp/yt-dlp/master/supportedsites.md')
        .then(response => response.text())
        .then(data => {
            allSites = data.split('\n')
                .filter(line => line.startsWith(' - **'))
                .map(line => {
                    const match = line.match(/\*\*(.*?)\*\*/);
                    return match ? match[1] : null;
                })
                .filter(site => site !== null);

            populateSiteList(allSites);
        });

    dropdownBtn.addEventListener('click', () => {
        dropdownContent.classList.toggle('show');
        if (dropdownContent.classList.contains('show')) {
            searchInput.focus();
        }
    });

    searchInput.addEventListener('input', () => {
        const searchTerm = searchInput.value.toLowerCase();
        const filteredSites = allSites.filter(site => site.toLowerCase().includes(searchTerm));
        populateSiteList(filteredSites);
    });

    function populateSiteList(sites) {
        siteList.innerHTML = '';
        sites.forEach(site => {
            const a = document.createElement('a');
            a.href = '#';
            a.textContent = site;
            a.addEventListener('click', (e) => {
                e.preventDefault();
                dropdownBtn.textContent = site;
                dropdownContent.classList.remove('show');
            });
            siteList.appendChild(a);
        });
    }

    // Close the dropdown when clicking outside
    window.addEventListener('click', (e) => {
        if (!siteDropdown.contains(e.target)) {
            dropdownContent.classList.remove('show');
        }
    });
}


window.electronAPI.receive('youtube-video-info', (data) => {
    updateDownload({
        title: data.title,
        uploader: data.uploader,
        thumbnail: data.thumbnail,
        order: data.order,
        progress: 0
    });
});

window.electronAPI.receive('generic-video-info', (data) => {
    updateDownload({
        title: data.title,
        uploader: data.uploader,
        thumbnail: data.thumbnail,
        order: data.order,
        progress: 0
    });
});


function initializeMusicTab() {
    // Tab switch functionality
    const tabButtons = document.querySelectorAll('.tab-button');
    const tabContents = document.querySelectorAll('.tab-content');

    tabButtons.forEach(button => {
        button.addEventListener('click', () => {
            document.querySelector('.tab-button.active').classList.remove('active');
            document.querySelector('.tab-content.active').classList.remove('active');

            button.classList.add('active');
            document.getElementById(button.dataset.tab).classList.add('active');
        });
    });
    const platformBoxes = ['youtube', 'qobuz', 'tidal', 'spotify', 'deezer', 'appleMusic'];

    platformBoxes.forEach(platform => {
        const input = document.getElementById(`${platform}-url`);
        const downloadButton = document.getElementById(`${platform}-download-button`);

        if (input && downloadButton) {
            input.addEventListener('keypress', (event) => {
                if (event.key === 'Enter') {
                    downloadButton.click();
                }
            });
        }
    });

    document.querySelectorAll('.dropdown-btn').forEach(button => {
        button.addEventListener('click', () => {
            const dropdownContent = button.nextElementSibling;
            const items = dropdownContent.querySelectorAll('a');
            items.forEach((item, index) => {
                item.style.setProperty('--order', index);
            });
            dropdownContent.classList.toggle('show');
        });
    });
    document.querySelectorAll('.batch-download-btn').forEach(button => {
        button.addEventListener('click', (event) => {
            const order = event.target.dataset.order;
            showBatchDownloadNotification(order);
        });
    });

    // Initialize dropdowns
    initializeDropdowns();

    // Add event listeners for download buttons
    document.getElementById('youtube-download-button').addEventListener('click', handleYoutubeDownload);
    document.getElementById('qobuz-download-button').addEventListener('click', handleQobuzDownload);
    document.getElementById('tidal-download-button').addEventListener('click', handleTidalDownload);
    document.getElementById('deezer-download-button').addEventListener('click', handleDeezerDownload);
    document.getElementById('streamrip-btn').addEventListener('click', handleStreamripDownload);
    document.getElementById('qobuzDownloadBatch_btn').addEventListener('click', startQobuzDownloadBatch);
    document.getElementById('appleMusic-download-button').addEventListener("click", handleAppleDownload);
    document.getElementById('spotify-download-button').addEventListener('click', handleSpotifyDownload);

    renderDownloads();

}



window.electronAPI.receive('download-error', (errorMessage) => {
    displayErrorNotification(errorMessage);
});

function displayErrorNotification(errorMessage) {
    const container = document.getElementById('floating-download-notifications');

    // Create a new notification element
    const notification = document.createElement('div');
    notification.classList.add('download-notification', 'error'); // Add error-specific styling if needed

    notification.innerHTML = `
        <div class="notification-header">
            <h3>Error</h3>
            <button class="close-btn" onclick="removeErrorNotification(this)">x</button>
        </div>
        <div class="notification-body">${errorMessage}</div>
    `;

    container.appendChild(notification);

    setTimeout(() => {
        removeErrorNotification(notification);
    }, 5000);
}

// Function to remove the notification
function removeErrorNotification(element) {
    if (element) {
        element.remove();
    }
}


function handleYoutubeDownload() {
    const url = document.getElementById('youtube-url').value;
    const isPlaylist = document.getElementById('youtube-playlist').checked;
    const qualityDropdown = document.getElementById('youtube-quality');
    const qualityLink = qualityDropdown.querySelector('.dropdown-content a[data-value]');
    const quality = qualityLink ? qualityLink.getAttribute('data-value') : null;

    try {
        if (!url) {
            throw new Error('YouTube URL is required.');
        }
        window.electronAPI.send('start-yt-music-download', { url, quality, isPlaylist });

        // Initial placeholder for YouTube download
        const downloadCount = state.downloads.length + 1;
        updateDownload({
            title: 'Fetching Title...',
            uploader: 'Fetching Uploader...',
            thumbnail: 'placeholder.png',
            order: downloadCount,
            progress: 0
        });
    } catch (error) {
        console.error('Error starting YouTube download:', error);
        displayErrorNotification('Error starting YouTube download: ' + error.message);
    }
}

async function startQobuzDownloadBatch() {
    try {
        const filePath = await openFileLocation();

        if (!filePath || typeof filePath !== 'string') {
            throw new Error('Invalid file path selected.');
        }

        if (!filePath.endsWith('.txt')) {
            throw new Error('Invalid file type. Expected a .txt file.');
        }

        const qualityDropdown = document.getElementById('qobuz-quality');
        let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || "1";

        window.electronAPI.send('start-qobuz-batch-download', { filePath, quality });
    } catch (error) {
        console.error('Error starting Qobuz batch download:', error);
        displayErrorNotification('Error starting Qobuz batch download: ' + error.message);
    }
}
async function startAppleDownloadBatch() {
    try {
        const filePath = await openFileLocation();

        if (!filePath || typeof filePath !== 'string') {
            throw new Error('Invalid file path selected.');
        }

        if (!filePath.endsWith('.txt')) {
            throw new Error('Invalid file type. Expected a .txt file.');
        }

        const qualityDropdown = document.getElementById('appleMusic-quality');
        let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || "1";

        window.electronAPI.send('start-apple-batch-download', { filePath, quality });
    } catch (error) {
        console.error('Error starting Apple Music batch download:', error);
        displayErrorNotification('Error starting Apple Music batch download: ' + error.message);
    }
}
async function startSpotifyDownloadBatch() {
    try {
        const filePath = await openFileLocation();

        if (!filePath || typeof filePath !== 'string') {
            throw new Error('Invalid file path selected.');
        }

        if (!filePath.endsWith('.txt')) {
            throw new Error('Invalid file type. Expected a .txt file.');
        }

        const qualityDropdown = document.getElementById('spotify-quality');
        let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || "1";

        window.electronAPI.send('start-spotify-batch-download', { filePath, quality });
    } catch (error) {
        console.error('Error starting Spotify batch download:', error);
        displayErrorNotification('Error starting Spotify batch download: ' + error.message);
    }
}
function startTidalDownloadBatch() {
    try {
        const filePath = openFileLocation();
        if (!filePath) throw new Error('File path selection canceled.');

        const qualityDropdown = document.getElementById('tidal-quality');
        let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || "1";

        window.electronAPI.send('start-tidal-batch-download', { filePath, quality });
    } catch (error) {
        console.error('Error starting Tidal batch download:', error);
        displayErrorNotification('Error starting Tidal batch download: ' + error.message);
    }
}

function startDeezerDownloadBatch() {
    try {
        const filePath = openFileLocation();
        if (!filePath) throw new Error('File path selection canceled.');

        const qualityDropdown = document.getElementById('deezer-quality');
        let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || "1";

        window.electronAPI.send('start-deezer-batch-download', { filePath, quality });
    } catch (error) {
        console.error('Error starting Deezer batch download:', error);
        displayErrorNotification('Error starting Deezer batch download: ' + error.message);
    }
}


function handleQobuzDownload() {
    const url = document.getElementById('qobuz-url').value;
    const qualityDropdown = document.getElementById('qobuz-quality');
    let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || "1";

    try {
        if (!url) {
            throw new Error('Qobuz URL is required.');
        }

        window.electronAPI.send('start-qobuz-download', { url, quality });
    } catch (error) {
        console.error('Error starting Qobuz download:', error);
        displayErrorNotification('Error starting Qobuz download: ' + error.message);
    }
}


function handleTidalDownload() {
    const url = document.getElementById('tidal-url').value;
    const qualityDropdown = document.getElementById('tidal-quality');
    let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || "1";

    try {
        if (!url) {
            throw new Error('Tidal URL is required.');
        }

        window.electronAPI.send('start-tidal-download', { url, quality });
    } catch (error) {
        console.error('Error starting Tidal download:', error);
        displayErrorNotification('Error starting Tidal download: ' + error.message);
    }
}

function handleDeezerDownload() {
    const url = document.getElementById('deezer-url').value;
    const qualityDropdown = document.getElementById('deezer-quality');
    let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || "1";

    try {
        if (!url) {
            throw new Error('Deezer URL is required.');
        }

        window.electronAPI.send('start-deezer-download', { url, quality });
    } catch (error) {
        console.error('Error starting Deezer download:', error);
        displayErrorNotification('Error starting Deezer download: ' + error.message);
    }
}
function handleSpotifyDownload() {
    const url = document.getElementById('spotify-url').value;
    const qualityDropdown = document.getElementById('spotify-quality');
    let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || "1";

    try {
        if (!url) {
            throw new Error('Spotify URL is required.');
        }
        window.electronAPI.send('start-spotify-download', { url, quality });
    } catch (error) {
        console.error('Error starting Spotıfy download:', error);
        displayErrorNotification('Error starting Spotıfy download: ' + error.message);
    }
}

function handleAppleDownload() {
    const url = document.getElementById('appleMusic-url').value;
    const qualityDropdown = document.getElementById('appleMusic-quality');
    let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || "aac";

    try {
        if (!url) {
            throw new Error('AppleMusic URL is required.');
        }

        window.electronAPI.send('start-apple-download', { url, quality });
    } catch (error) {
        console.error('Error starting Apple download:', error);
        displayErrorNotification('Error starting Apple Music download: ' + error.message);
    }
}

function handleStreamripDownload() {
    const command = document.getElementById('streamrip-input').value;

    try {
        if (!command) {
            throw new Error('Streamrip command is required.');
        }

        window.electronAPI.send('start-streamrip', command);
    } catch (error) {
        console.error('Error starting streamrip:', error);
        displayErrorNotification('Error starting streamrip: ' + error.message);
    }
}


function initializeDropdowns() {
    const dropdowns = document.querySelectorAll('.dropdown');

    dropdowns.forEach(dropdown => {
        const btn = dropdown.querySelector('.dropdown-btn');
        const content = dropdown.querySelector('.dropdown-content');

        btn.addEventListener('click', () => {
            dropdown.classList.toggle('active');
        });

        document.addEventListener('click', (e) => {
            if (!dropdown.contains(e.target)) {
                dropdown.classList.remove('active');
            }
        });

        const options = content.querySelectorAll('a');
        options.forEach(option => {
            option.addEventListener('click', (e) => {
                e.preventDefault();
                btn.textContent = option.textContent;
                btn.dataset.value = option.dataset.value;
                dropdown.classList.remove('active');

                // Trigger change event for compatibility with existing code
                const changeEvent = new Event('change');
                dropdown.dispatchEvent(changeEvent);
            });
        });
    });
}let timeoutIds = {};
let fadeOutTimeouts = {};

function renderDownloads() {
    const downloadContainer = document.getElementById('download-container');
    if (!downloadContainer) return;

    state.downloads.forEach(download => {
        let downloadDiv = document.getElementById(`download-${download.order}`);
        if (!downloadDiv) {
            downloadDiv = document.createElement('div');
            downloadDiv.id = `download-${download.order}`;
            downloadDiv.classList.add('download-entry');
            downloadContainer.prepend(downloadDiv);

            // Initial rendering for static and dynamic content
            const batchButton = download.isBatch ?
                `<button class="batch-download-btn" data-order="${download.order}">View Progress</button>` : '';

            downloadDiv.innerHTML = `
                <img src="${download.thumbnail || './assets/placeholder.png'}" class="thumbnail" alt="${download.title || 'Unknown Title'}" />
                <div class="download-info">
                    <h3 class="title">${download.title || 'Unknown Title'}</h3>
                    <p class="uploader">${download.uploader || download.artist || 'Unknown Artist'}</p>
                    ${download.album ? `<p class="album">${download.album}</p>` : ''}
                    <p>Download #${download.order}</p>
                    ${batchButton}
                    <div class="progress-bar"><div class="progress" style="width: ${download.progress}%;"></div></div>
                    <p class="progress-text">${download.progress.toFixed(1)}%</p>
                </div>
            `;
        } else {
            // Selectively update only changed dynamic content
            const title = downloadDiv.querySelector('.title');
            const uploader = downloadDiv.querySelector('.uploader');
            const album = downloadDiv.querySelector('.album');
            const thumbnail = downloadDiv.querySelector('.thumbnail');
            const progressBar = downloadDiv.querySelector('.progress');
            const progressText = downloadDiv.querySelector('.progress-text');

            if (title && title.textContent !== download.title) title.textContent = download.title;
            if (uploader && uploader.textContent !== (download.uploader || download.artist)) {
                uploader.textContent = download.uploader || download.artist;
            }
            if (album && album.textContent !== download.album) {
                album.textContent = download.album;
            }
            if (thumbnail && thumbnail.src !== download.thumbnail) {
                thumbnail.src = download.thumbnail || './assets/placeholder.png';
            }
            if (progressBar) progressBar.style.width = `${download.progress}%`;
            if (progressText) progressText.textContent = `${download.progress.toFixed(1)}%`;
        }
    });

    document.querySelectorAll('.batch-download-btn').forEach(button => {
        button.addEventListener('click', (event) => {
            const order = parseInt(event.target.dataset.order);
            const download = state.downloads.find(d => d.order === order);
            if (download?.isBatch) {
                showBatchDownloadNotification(order);
            }
        });
    });
}

window.electronAPI.receive('download-update', (data) => {
    const { tracksProgress, order } = data;

    const downloadIndex = state.downloads.findIndex(d => d.order === order);
    if (downloadIndex !== -1) {
        state.downloads[downloadIndex].tracksProgress = tracksProgress;

        if (tracksProgress && tracksProgress.length > 0) {
            const totalProgress = tracksProgress.reduce((sum, track) => sum + track.progress, 0);
            state.downloads[downloadIndex].progress = totalProgress / tracksProgress.length;
        }
    }

    renderDownloads();
    updateBatchNotification(order, tracksProgress);
});

function showBatchDownloadNotification(order) {
    // Remove existing notification if any
    removeNotification(order);

    const download = state.downloads.find(d => d.order === order);
    if (!download?.isBatch) return;

    const container = document.getElementById('floating-download-notifications');
    const notification = document.createElement('div');
    notification.classList.add('download-notification');
    notification.id = `download-notification-${order}`;

    notification.innerHTML = `
        <div class="notification-header">
            <h3>Batch Download #${order}</h3>
            <button class="close-btn" onclick="removeNotification(${order})">×</button>
        </div>
        <div id="download-progress-${order}" class="notification-tracks">
            ${download.tracksProgress ? download.tracksProgress.map(track => `
                <div class="track-item">
                    <span class="track-name">${track.trackTitle || 'Loading...'}</span>
                    <div class="track-progress-bar">
                        <div class="track-progress-fill" style="width: ${track.progress}%;"></div>
                    </div>
                    <span class="track-percentage">${track.progress.toFixed(1)}%</span>
                </div>
            `).join('') : 'Loading tracks...'}
        </div>
    `;

    container.appendChild(notification);

    // Start automatic fade-out timer
    startAutoFadeOut(order);
}

function updateBatchNotification(order, tracksProgress) {
    const progressElement = document.getElementById(`download-progress-${order}`);
    if (progressElement && tracksProgress) {
        progressElement.innerHTML = tracksProgress.map(track => `
            <div class="track-item">
                <span class="track-name">${track.trackTitle || 'Loading...'}</span>
                <div class="track-progress-bar">
                    <div class="track-progress-fill" style="width: ${track.progress}%;"></div>
                </div>
                <span class="track-percentage">${track.progress.toFixed(1)}%</span>
            </div>
        `).join('');
    }
}

function startAutoFadeOut(order) {
    const notification = document.getElementById(`download-notification-${order}`);
    if (!notification) return;

    // Reset any existing timeouts
    if (timeoutIds[order]) {
        clearTimeout(timeoutIds[order]);
        delete timeoutIds[order];
    }
    if (fadeOutTimeouts[order]) {
        clearTimeout(fadeOutTimeouts[order]);
        delete fadeOutTimeouts[order];
    }

    // Add mouseenter/mouseleave events to pause/resume the auto-hide
    notification.addEventListener('mouseenter', () => {
        if (timeoutIds[order]) {
            clearTimeout(timeoutIds[order]);
            delete timeoutIds[order];
        }
        if (fadeOutTimeouts[order]) {
            clearTimeout(fadeOutTimeouts[order]);
            delete fadeOutTimeouts[order];
        }
        notification.classList.remove('fade-out');
    });

    notification.addEventListener('mouseleave', () => {
        startAutoFadeOut(order);
    });

    // Start the auto-hide timer
    timeoutIds[order] = setTimeout(() => {
        notification.classList.add('fade-out');
        fadeOutTimeouts[order] = setTimeout(() => {
            removeNotification(order);
        }, 500); // 500ms for fade-out animation
    }, 5000); // 5 seconds before starting fade-out
}

function removeNotification(order) {
    const notification = document.getElementById(`download-notification-${order}`);
    if (notification) {
        notification.remove();
    }

    // Clear any existing timeouts
    if (timeoutIds[order]) {
        clearTimeout(timeoutIds[order]);
        delete timeoutIds[order];
    }
    if (fadeOutTimeouts[order]) {
        clearTimeout(fadeOutTimeouts[order]);
        delete fadeOutTimeouts[order];
    }
}

window.electronAPI.receive('download-complete', (data) => {
    const { order, completedTracks, totalTracks } = data;
    if (completedTracks === totalTracks) {
        removeNotification(order);
    }
});
function updateDownload(data) {
    const existingDownload = state.downloads.find(d => d.order === data.order);
    if (existingDownload) {
        Object.assign(existingDownload, data);
    } else {
        state.downloads.push(data);
    }
    renderDownloads();
}

window.electronAPI.receive('download-info', (data) => {
    updateDownload({ ...data, progress: 0 });
});

window.electronAPI.receive('download-update', (data) => {
    updateDownload(data);
});

window.electronAPI.receive('qobuz-details', (data) => {
    updateDownload({
        order: data.order,
        thumbnail: data.album.image.small,
        title: data.title,
        artist: data.album.artist.name,
        bitDepth: data.album.maximum_bit_depth,
        samplingRate: data.album.maximum_sampling_rate
    });
});

window.electronAPI.receive('deezer-details', (data) => {
    updateDownload({
        order: data.order,
        thumbnail: data.album.cover_medium,
        title: data.title_short,
        artist: data.artist.name,
        album: data.album.title
    });
});

window.electronAPI.receive('tidal-details', (data) => {
    updateDownload({
        order: data.order,
        thumbnail: data.thumbnail,
        title: data.title,
        artist: data.artist,
        quality: data.quality
    });
});

window.electronAPI.receive('download-complete', (data) => {
    updateDownload({ ...data, progress: 100 });
});

document.getElementById('musicBtn').addEventListener('click', () => loadPage('music'));
document.getElementById('videoBtn').addEventListener('click', () => loadPage('video'));
document.getElementById('downloadsBtn').addEventListener('click', () => loadPage('downloads'));
document.getElementById('searchBtn').addEventListener('click', () => loadPage('search'));
document.getElementById('settingsBtn').addEventListener('click', () => loadPage('settings'));
document.getElementById('helpBtn').addEventListener('click', () => loadPage('help'));
document.getElementById('hamburger-menu').addEventListener('click', () => {
    const navbar = document.querySelector('.navbar');
    navbar.classList.toggle('collapsed');
    localStorage.setItem('navbarCollapsed', navbar.classList.contains('collapsed'));
});

// Restore navbar state on page load
document.addEventListener('DOMContentLoaded', () => {
    const navbar = document.querySelector('.navbar');
    const isCollapsed = localStorage.getItem('navbarCollapsed') === 'true';
    if (isCollapsed) {
        navbar.classList.add('collapsed');
    }
});
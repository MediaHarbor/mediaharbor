// Global state
const state = {
    downloads: [],
    currentPage: 'music'
};
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
    console.log('Download status page initializing...');
    const downloadContainer = document.getElementById('download-history-container');

    try {
        const downloads = await window.electronAPI.getDownloads();
        console.log('Received downloads:', downloads);

        if (!downloads || downloads.length === 0) {
            downloadContainer.innerHTML = '<p class="no-downloads">No downloads found.</p>';
            return;
        }

        const downloadsList = downloads.map(download => {
            // Replace backslashes with forward slashes and escape special characters
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
        downloadContainer.innerHTML = '<p class="error">Error loading downloads: ' + error.message + '</p>';
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
        { checkboxId: 'ytm_override_download_extension', fieldSelector: '#youtubeMusicExtensions' }
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

// Initialize settings
let settings = {};

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
        'conversion_check': 'checked', 'conversion_codec': 'value', 'conversion_sampling_rate': 'value',
        'conversion_bit_depth': 'value', 'meta_album_name_playlist_check': 'checked',
        'meta_album_order_playlist_check': 'checked','excluded_tags': 'value', 'speed_limit_type': 'value',
        'qobuz_app_id': 'value', 'qobuz_secrets': 'value',
        'spotify_output_path': 'value', 'spotify_temp_path': 'value','spotify_enable_videos': 'checked', 'spotify_download_music_videos': 'checked', 'spotify_download_podcast_videos': 'checked', 'spotify_force_premium': 'checked', 'spotify_download_premium_videos': 'checked', 'spotify_download_mode': 'value', 'spotify_video_format': 'value', 'spotify_remux_mode_video': 'value',
        'spotify_template_folder_album': 'value', 'spotify_template_folder_compilation': 'value', 'spotify_template_file_single_disc': 'value', 'spotify_template_file_multi_disc': 'value',
        'apple_output_path': 'value', 'apple_temp_path': 'value', 'apple_download_mode': 'value', 'apple_remux_mode': 'value', 'apple_cover_format': 'value', 'apple_synced_lyrics_format': 'value',
        'apple_template_folder_album': 'value', 'apple_template_folder_compilation': 'value', 'apple_template_file_single_disc': 'value', 'apple_template_file_multi_disc': 'value',
        "spotify_wait_interval" : "value", "spotify_no_exceptions": "checked", "spotify_save_cover": "checked", "spotify_save_playlist": "checked", "spotify_overwrite": "checked", "spotify_lrc_only": "checked", "spotify_no_lrc": "checked",
        "apple_disable_music_video_skip": "checked", "apple_save_cover": "checked", "apple_overwrite": "checked", "apple_save_playlist": "checked", "apple_synced_lyrics_only": "checked", "apple_no_synced_lyrics": "checked", "apple_cover_size": "value",
        "apple_cookies_path": "value", "spotify_cookies_path": "value", "spotify_wvd_path": "value",
    };

    Object.keys(settingFields).forEach(id => {
        const element = document.getElementById(id);
        if (element) {
            element[settingFields[id]] = settings[id];
        }
    });
    handleDependentFields();
}


function addSettingsListeners() {
    const settingsMapping = [
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
        {id: 'speed_limit_type', key:'speed_limit_type', type: 'value'},
        {id: 'qobuz_app_id', key: 'qobuz_app_id', type: 'value' },
        {id: 'qobuz_secrets', key: 'qobuz_secrets', type: 'value' },
        {id: 'spotify_output_path', key: 'spotify_output_path', type: 'value' },
        {id: 'spotify_temp_path', key: 'spotify_temp_path', type: 'value' },
        {id: 'spotify_enable_videos', key:'spotify_enable_videos', type: 'checked'},
        {id: 'spotify_download_music_videos', key:'spotify_download_music_videos', type: 'checked'},
        {id: 'spotify_download_podcast_videos', key:'spotify_download_podcast_videos', type: 'checked'},
        {id: 'spotify_force_premium', key:'spotify_force_premium', type: 'checked'},
        {id: 'spotify_download_premium_videos', key:'spotify_download_premium_videos', type: 'checked'},
        {id: 'spotify_download_mode', key: 'spotify_download_mode', type:'value'},
        {id: 'spotify_video_format', key: 'spotify_video_format', type:'value'},
        {id: 'spotify_remux_mode_video', key: 'spotify_remux_mode_video', type:'value'},
        {id: 'spotify_template_folder_album', key: 'spotify_template_folder_album', type:'value'},
        {id: 'spotify_template_folder_compilation', key: 'spotify_template_folder_compilation', type:'value'},
        {id: 'spotify_template_file_single_disc', key: 'spotify_template_file_single_disc', type:'value'},
        {id: 'spotify_template_file_multi_disc', key: 'spotify_template_file_multi_disc', type:'value'},
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
        {id: "spotify_wait_interval" ,key: "spotify_wait_interval", type: "value"},
        {id: "spotify_no_exceptions" ,key: "spotify_no_exceptions", type: "checked"},
        {id: "spotify_save_cover" ,key: "spotify_save_cover", type: "checked"},
        {id: "spotify_save_playlist" ,key: "spotify_save_playlist", type: "checked"},
        {id: "spotify_overwrite" ,key: "spotify_overwrite", type: "checked"},
        {id: "spotify_lrc_only" ,key: "spotify_lrc_only", type: "checked"},
        {id: "spotify_no_lrc" ,key: "spotify_no_lrc", type: "checked"},
        {id: "apple_disable_music_video_skip", key: "apple_disable_music_video_skip", type: "checked"},
        {id: "apple_save_cover", key: "apple_save_cover", type: "checked"},
        {id: "apple_overwrite", key: "apple_overwrite", type: "checked"},
        {id: "apple_save_playlist", key: "apple_save_playlist", type: "checked"},
        {id: "apple_synced_lyrics_only", key: "apple_synced_lyrics_only", type: "checked"},
        {id: "apple_no_synced_lyrics", key: "apple_no_synced_lyrics", type: "checked"},
        {id: "apple_cover_size", key: "apple_cover_size", type: "value"},
        {id: "spotify_cookies_path", key: "spotify_cookies_path", type: "value"},
        {id: "apple_cookies_path", key: "apple_cookies_path", type: "value"},
        {id: "spotify_wvd_path", key:"spotify_wvd_path", type: "value"},
    ];

    settingsMapping.forEach(({ id, key, type }) => {
        const element = document.getElementById(id);
        if (element) {
            // Add change listener for all elements
            element.addEventListener('change', (e) => {
                settings[key] = type === 'checked' ? e.target.checked : e.target.value;
                window.electronAPI.send('save-settings', settings);
            });

            // Add input listener for text fields to save as you type
            if (type === 'value' && element.tagName === 'INPUT') {
                element.addEventListener('input', (e) => {
                    settings[key] = e.target.value;
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
    const platforms = ['youtube', 'youtubeMusic', 'spotify', 'deezer', 'qobuz', 'tidal'];

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

    button.addEventListener('click', () => {
        content.style.display = content.style.display === 'block' ? 'none' : 'block';
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

function handleSearchResults({ results, platform }, type) {
    const container = document.getElementById('search-container');
    container.innerHTML = '';

    let normalizedResults = normalizeResults(results, platform, type);
    normalizedResults.forEach(result => {
        const resultCard = createResultCard(result, platform, type);
        container.appendChild(resultCard);
    });
    window.api.onStreamReady(({ streamUrl, platform }) => {
        const isVideo = platform === 'youtube';
        const player = document.createElement(isVideo ? 'video' : 'audio');
        player.controls = true;
        player.autoplay = true;
        player.src = streamUrl;

        const popup = document.createElement('div');
        popup.style.cssText = `
        position: fixed;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        background: rgba(0, 0, 0, 0.8);
        display: flex;
        justify-content: center;
        align-items: center;
        z-index: 1000;
    `;

        const closeBtn = document.createElement('button');
        closeBtn.textContent = '×';
        closeBtn.style.cssText = `
        position: absolute;
        top: 20px;
        right: 20px;
        background: none;
        border: none;
        color: white;
        font-size: 24px;
        cursor: pointer;
    `;
        closeBtn.onclick = () => {
            popup.remove();
            player.pause();
        };

        const playerContainer = document.createElement('div');
        playerContainer.style.cssText = `
        max-width: 90%;
        max-height: 90%;
    `;

        playerContainer.appendChild(player);
        popup.appendChild(closeBtn);
        popup.appendChild(playerContainer);
        document.body.appendChild(popup);
    });
    window.api.onStreamReady(handleStreamReady);
}

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
                default:
                case 'video':
                    return results.videos.map(video => video.resource) || [];
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

    card.innerHTML = `
        <div class="card-content">
            <img src="${cardData.thumbnail}" alt="Thumbnail" class="result-thumbnail">
            <div class="result-info">
                <h3>${cardData.title}</h3>
                ${cardData.details}
            </div>
            <button class="play-button" data-url="${cardData.playUrl}" data-type="${type}">
               <span class="fa-solid fa-play"></span>
            </button>
            <button class="copy-button" data-url="${cardData.copyUrl}" data-type="${type}">
                <span class="fa-regular fa-copy"></span>
            </button>
        </div>
    `;

    const copyBtn = card.querySelector('.copy-button');
    if (copyBtn) {
        copyBtn.addEventListener('click', () => {
            const urlToCopy = copyBtn.dataset.url; // Fixed variable name
            copyClipboard(urlToCopy);
        });
    }

    const playBtn = card.querySelector('.play-button');
    if (playBtn) {
        // Add debouncing to prevent multiple clicks
        let isPlaying = false;
        playBtn.addEventListener('click', async () => {
            if (isPlaying) return;
            isPlaying = true;
            playBtn.disabled = true;

            showLoadingOverlay();

            try {
                await window.api.playMedia({
                    url: playBtn.dataset.url,
                    platform,
                    type,
                    id: result.id
                });
            } catch (error) {
                handleError(error);
                hideLoadingOverlay();
            } finally {
                isPlaying = false;
                playBtn.disabled = false;
            }
        });
    }

    return card;
}

function copyClipboard(url) {
    if (!url) {
        console.error('No URL provided to copy.');
        return;
    }

    navigator.clipboard.writeText(url)
        .then(() => {
            console.log('URL copied to clipboard:', url);
            // Optionally, show a success message to the user
        })
        .catch(err => {
            console.error('Failed to copy: ', err);
            // Optionally, show an error message to the user
        });
}

function handleStreamReady({ streamUrl, platform }) {
    hideLoadingOverlay();

    const isVideo = platform === 'youtube';
    const player = document.createElement(isVideo ? 'video' : 'audio');
    player.controls = true;
    player.autoplay = true;
    player.src = streamUrl;

    const popup = document.createElement('div');
    popup.style.cssText = `
        position: fixed;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        background: rgba(0, 0, 0, 0.8);
        display: flex;
        justify-content: center;
        align-items: center;
        z-index: 1000;
    `;

    // Cleanup function
    const cleanup = () => {
        popup.remove();
        player.pause();
        player.src = '';
        player.load();
    };

    const closeBtn = document.createElement('button');
    closeBtn.textContent = '×';
    closeBtn.style.cssText = `
        position: absolute;
        top: 20px;
        right: 20px;
        background: none;
        border: none;
        color: white;
        font-size: 24px;
        cursor: pointer;
    `;
    closeBtn.onclick = cleanup;

    const playerContainer = document.createElement('div');
    playerContainer.style.cssText = `
        max-width: 90%;
        max-height: 90%;
    `;

    playerContainer.appendChild(player);
    popup.appendChild(closeBtn);
    popup.appendChild(playerContainer);
    document.body.appendChild(popup);
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
                        playUrl: result['Playlist URL'],
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
        case 'youtubeMusic':
            switch (type){
                case "album":
                    return {
                        thumbnail: result.AlbumCover,
                        title: result.AlbumTitle,
                        details: `
                        <p>Artist: ${result.ArtistName}</p>`,
                        playUrl: result.AlbumURL,
                        copyUrl: result.AlbumURL
                    };
                case "playlist":
                    return {
                        thumbnail: result.PlaylistCover,
                        title: result.PlaylistTitle,
                        details: `
                        <p>${result.Author}`,
                        playUrl: result.PlaylistURL,
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
                        playUrl: result.preview_url,
                        copyUrl: result.external_urls.spotify
                    };
                case 'album':
                    return {
                        thumbnail: result.images[0]?.url || '',
                        title: result.name,
                        details: `<p>Artist: ${result.artists[0]?.name || 'Unknown Artist'}</p>`,
                        playUrl: `WIP`,
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
                        thumbnail: result.images[0]?.url || '',
                        title: result.name,
                        details: `<p>By: ${result.owner?.display_name || 'Unknown'}</p>`,
                        playUrl: `WIP`,
                        copyUrl: result.external_urls.spotify
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
                        playUrl: result.preview,
                        copyUrl: result.link
                    };
                case 'album':
                    return {
                        thumbnail: result.cover_medium,
                        title: result.title,
                        details: `<p>Artist: ${result.artist?.name || 'Unknown Artist'}</p>`,
                        playUrl: `WIP`,
                        copyUrl: result.link
                    };
                case 'artist':
                    return {
                        thumbnail: result.picture_medium,
                        title: result.name,
                        details: `<p>Fans: ${result.nb_fan || 0}</p>`,
                        playUrl: `WIP`,
                        copyUrl: result.link
                    };
                case 'playlist':
                    return {
                        thumbnail: result.picture_medium,
                        title: result.title,
                        details: `<p>Tracks: ${result.nb_tracks || 0}</p>`,
                        playUrl: `WIP`,
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
                    <p>Album: ${result.album?.title || 'Unknown Album'}</p>
                    <p>Artist: ${result.album?.artist?.name || 'Unknown Artist'}</p>
                `,
                        playUrl: `https://play.qobuz.com/track/${result.id}`,
                        copyUrl: `https://play.qobuz.com/track/${result.id}`
                    };
                case 'album':
                    return {
                        thumbnail: result.image?.small || result.Thumbnail,
                        title: result.title || 'Unknown Title',
                        details: `
                    <p>Artist: ${result.artist?.name || 'Unknown Artist'}</p>
                `,
                        playUrl: 'WIP',
                        copyUrl: `https://play.qobuz.com/album/${result.id}`,
                    };
                case 'playlist':
                    return {
                        thumbnail: result.image_rectangle[0],
                        title: result.name,
                        details: `<p>${result.genres[0].percent}% ${result.genres[0].name}</p>`,
                        playUrl: `WIP`,
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
                    <p>Album: ${result.album?.title || 'Unknown Album'}</p>
                    <p>Artist: ${result.artists[0]?.name || 'Unknown Artist'}</p>
                `,
                        playUrl: result.id,
                        copyUrl: result.tidalUrl
                    };
                case 'album':
                    return {
                        thumbnail: result.imageCover[2].url, // Using the 160x160 version
                        title: result.title,
                        details: `
                    <p>Artist: ${result.artists[0]?.name || 'Unknown Artist'}</p>
                    <p>Release: ${result.releaseDate}</p>
                    <p>${result.numberOfTracks} / ${result.numberOfVolumes} </p>
                `,
                        playUrl: "WIP",
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
    const container = document.getElementById('floating-search-notifications');
    const notification = document.createElement('div');
    notification.className = 'notification';
    notification.textContent = message;

    container.appendChild(notification);

    setTimeout(() => {
        notification.remove();
    }, 3000);
}


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
    const qualityDropdown = document.getElementById('youtube-quality');
    const quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value || 'bestvideo+bestaudio/best';
    window.electronAPI.send('start-yt-video-download', { url, quality });
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


// Initialize the video tab when the DOM is loaded
document.addEventListener('DOMContentLoaded', initializeVideoTab);
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
            const order = event.target.dataset.order;  // Assuming the button has a dataset attribute for the batch order
            showBatchDownloadNotification(order);      // Call the function when batch download is clicked
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



// Function to display errors in the floating notification container
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

    // Auto-remove the notification after some time (e.g., 5 seconds)
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
    const qualityDropdown = document.getElementById('youtube-quality');
    const qualityDropdownBtn = qualityDropdown.querySelector('.dropdown-btn');
    const qualityLink = qualityDropdown.querySelector('.dropdown-content a[data-value]');
    const quality = qualityLink ? qualityLink.getAttribute('data-value') : null;

    try {
        if (!url) {
            throw new Error('YouTube URL is required.');
        }
        window.electronAPI.send('start-yt-music-download', { url, quality });

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

// Updated handleDeezerDownload function
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
                btn.dataset.value = option.dataset.value; // Save the value to the button's dataset
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
        }

        // Only show the overall progress for batch downloads in main view
        const batchButton = download.isBatch ?
            `<button class="batch-download-btn" data-order="${download.order}">View Progress</button>` : '';

        downloadDiv.innerHTML = `
            <img src="${download.thumbnail || 'placeholder.png'}" class="thumbnail" alt="${download.title || 'Unknown Title'}" />
            <div class="download-info">
                <h3>${download.title || 'Unknown Title'}</h3>
                <p class="uploader">${download.uploader || download.artist || 'Unknown Artist'}</p>
                ${download.album ? `<p class="album">${download.album}</p>` : ''}
                <p>Download #${download.order}</p>
                ${batchButton}
                <div class="progress-bar"><div class="progress" style="width: ${download.progress}%;"></div></div>
                <p class="progress-text">${download.progress.toFixed(1)}%</p>
            </div>
        `;
    });

    // Add event listeners for batch download buttons
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
    const { tracksProgress, order, isBatch } = data;

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

document.getElementById('minimizeBtn').addEventListener('click', () => {
    window.electronAPI.send('minimize-window');
});

document.getElementById('maximizeBtn').addEventListener('click', () => {
    window.electronAPI.send('maximize-window');
});

document.getElementById('closeBtn').addEventListener('click', () => {
    window.electronAPI.send('close-window');
});
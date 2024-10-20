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
        // Check system theme and apply accordingly
        const isDarkMode = window.matchMedia('(prefers-color-scheme: dark)').matches;
        document.body.classList.add(isDarkMode ? 'dark' : 'light');
    } else if (theme === 'dark') {
        document.body.classList.add('dark');
    } else {
        document.body.classList.add('light');
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
    downloads: 'download_status.html',
    settings: 'settings.html',
    help: 'help.html'
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
            initializeHelpTab();
        }
        else if (pageName === 'downloads') {
            await initializeDownloadStatusPage();
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
function initializeHelpTab(){

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
            }
        }
    } catch (error) {
        console.error('Error selecting folder location:', error);
    }
}
async function openFileLocation(inputId){
    try {
        // Get the folder path using system dialog
        const result = await window.electronAPI.fileSelectLocation();

        if (result) {
            // Update the corresponding input field with the selected path
            const inputElement = document.getElementById(inputId);
            if (inputElement) {
                inputElement.value = result;
            }
        }
    } catch (error) {
        console.error('Error selecting folder location:', error);
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
        {id: 'speed_limit_type', key:'speed_limit_type', type: 'value'}
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


    // Initialize dropdowns
    initializeDropdowns();

    // Add event listeners for download buttons
    document.getElementById('youtube-download-button').addEventListener('click', handleYoutubeDownload);
    document.getElementById('qobuz-download-button').addEventListener('click', handleQobuzDownload);
    document.getElementById('tidal-download-button').addEventListener('click', handleTidalDownload);
    document.getElementById('deezer-download-button').addEventListener('click', handleDeezerDownload);
    document.getElementById('streamrip-btn').addEventListener('click', handleStreamripDownload);

    renderDownloads();
}

function handleYoutubeDownload() {

    const url = document.getElementById('youtube-url').value;
    const qualityDropdown = document.getElementById('youtube-quality');
    const qualityDropdownBtn = qualityDropdown.querySelector('.dropdown-btn');
    const qualityLink = qualityDropdown.querySelector('.dropdown-content a[data-value]');
    const quality = qualityLink ? qualityLink.getAttribute('data-value') : null;

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
}

function handleQobuzDownload() {
    const url = document.getElementById('qobuz-url').value;
    const qualityDropdown = document.getElementById('qobuz-quality'); // Get the correct dropdown
    let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value; // Fetch quality value

    if (quality === undefined) {
        quality = "1";
    }
    window.electronAPI.send('start-qobuz-download', { url, quality });
}




function handleTidalDownload() {
    const url = document.getElementById('tidal-url').value;
    const qualityDropdown = document.getElementById('tidal-quality');
    let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value; // Fetch quality value

    if (quality === undefined) {
        quality = "1";
    }
    window.electronAPI.send('start-tidal-download', { url, quality });
}

function handleDeezerDownload() {
    const url = document.getElementById('deezer-url').value;
    const qualityDropdown = document.getElementById('deezer-quality');
    let quality = qualityDropdown.querySelector('.dropdown-btn').dataset.value; // Fetch quality value
    if (quality === undefined) {
        quality = "1";
    }
    window.electronAPI.send('start-deezer-download', { url, quality });
}

function handleStreamripDownload() {
    const command = document.getElementById('streamrip-input').value;
    window.electronAPI.send('start-streamrip', command);
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
}


function renderDownloads() {
    const downloadContainer = document.getElementById('download-container');
    if (!downloadContainer) return;

    state.downloads.forEach(download => {
        let downloadDiv = document.getElementById(`download-${download.order}`);
        if (!downloadDiv) {
            downloadDiv = document.createElement('div');
            downloadDiv.id = `download-${download.order}`;
            downloadDiv.classList.add('download-entry');
            downloadContainer.appendChild(downloadDiv);
        }

        downloadDiv.innerHTML = `
            <img src="${download.thumbnail || 'placeholder.png'}" class="thumbnail" alt="${download.title || 'Unknown Title'}" />
            <div class="download-info">
                <h3>${download.title || 'Unknown Title'}</h3>
                <p class="uploader">${download.uploader || download.artist || 'Unknown Artist'}</p>
                ${download.album ? `<p class="album">${download.album}</p>` : ''}
                ${download.bitDepth && download.samplingRate ? `
                    <p>Quality: ${download.bitDepth}-bit / ${download.samplingRate} kHz</p>
                ` : ''}
                <p>Download #${download.order}</p>
                <div class="progress-bar"><div class="progress" style="width: ${download.progress}%;"></div></div>
                <p class="progress-text">${download.progress.toFixed(1)}%</p>
            </div>
        `;
    });
}

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
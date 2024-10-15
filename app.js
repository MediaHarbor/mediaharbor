// Global state
const state = {
    downloads: [],
    currentPage: 'music'
};

// theme checker
window.addEventListener('DOMContentLoaded', () => {
    if (window.theme.isDarkMode()) {
        document.body.classList.add('dark');
    }

    // Listen for theme changes
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', event => {
        if (event.matches) {
            document.body.classList.add('dark');
        } else {
            document.body.classList.remove('dark');
        }
    });
});

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

function initializeSettingsTab(){
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
        title: 'Generic Download',
        uploader: data.url,
        thumbnail: '/placeholder.png',
        order: data.order,
        progress: 0
    });
});


// Initialize the video tab when the DOM is loaded
document.addEventListener('DOMContentLoaded', initializeVideoTab);
function initializeMusicTab() {
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


    const tabButtons = document.querySelectorAll('.tab-button');
    const tabContents = document.querySelectorAll('.tab-content');

    // Hide all tab contents except the first one
    tabContents.forEach((content, index) => {
        if (index === 0) {
            content.classList.add('active');
        } else {
            content.classList.remove('active');
        }
    });

    // Set the first tab button as active
    tabButtons[0].classList.add('active');

    tabButtons.forEach(button => {
        button.addEventListener('click', () => {
            tabButtons.forEach(btn => btn.classList.remove('active'));
            tabContents.forEach(content => content.classList.remove('active'));

            button.classList.add('active');
            const tabId = button.dataset.tab;
            const activeContent = document.getElementById(tabId);
            if (activeContent) {
                activeContent.classList.add('active');
            }
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
    const quality = qualityDropdown.querySelector('.dropdown-btn').textContent;
    window.electronAPI.send('start-yt-music-download', { url, quality });

    // Initial placeholder for YouTube download
    const downloadCount = state.downloads.length + 1;
    updateDownload({
        title: 'Fetching Title...',
        uploader: 'Fetching Uploader...',
        thumbnail: 'path/to/placeholder/image.png',
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
            <img src="${download.thumbnail || '/placeholder.png'}" class="thumbnail" alt="${download.title || 'Unknown Title'}" />
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

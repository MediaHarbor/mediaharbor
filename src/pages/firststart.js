const PAGES = ['welcome-page', 'dependencies-page', 'completion-page'];
let currentPageIndex = 0;

const pageElements = PAGES.map(pageId => document.getElementById(pageId));
const progressOverlay = document.getElementById('progress-overlay');
const progressFill = document.querySelector('.progress-fill');
const progressStatus = document.getElementById('progress-status');

document.addEventListener('DOMContentLoaded', async () => {
    try {
        await window.electronAPI.checkDependencies();
        showPage(currentPageIndex, true);
    } catch (error) {
        displayError(`Initialization failed: ${error.message}`);
    }
});

window.electronAPI.handleDependencyStatus((event, status) => {
    Object.entries(status).forEach(([dep, isInstalled]) => {
        const depElement = document.getElementById(`${dep}-dep`);
        if (depElement) {
            const statusIcon = depElement.querySelector('.status-icon');
            const installBtn = depElement.querySelector('.install-btn');

            if (isInstalled) {
                statusIcon.textContent = '✅';
                depElement.classList.add('success');
                installBtn.disabled = true;
                installBtn.textContent = 'Installed';
            } else {
                statusIcon.textContent = '⭕';
                depElement.classList.remove('success');
                installBtn.disabled = false;
                installBtn.textContent = 'Install';
            }
        }
    });
    validateRequiredDependencies();
});

document.querySelectorAll('.install-btn').forEach(button => {
    button.addEventListener('click', async ({ target }) => {
        const dep = target.dataset.dep;
        if (!dep) return;

        showProgress();

        try {
            await window.electronAPI.installDependency(dep);
            await window.electronAPI.checkDependencies();
        } catch (error) {
            displayError(`Failed to install ${dep}: ${error.message}`);
        } finally {
            hideProgress();
        }
    });
});

document.querySelectorAll('.install-btn').forEach(button => {
    button.addEventListener('mousedown', function() {
        this.style.transform = 'scale(0.95)';
    });

    button.addEventListener('mouseup', function() {
        this.style.transform = 'scale(1)';
    });

    button.addEventListener('mouseleave', function() {
        this.style.transform = 'scale(1)';
    });
});

const navigateTo = (targetPageIndex) => {
    if (targetPageIndex < 0 || targetPageIndex >= PAGES.length || targetPageIndex === currentPageIndex) return;

    const currentPage = pageElements[currentPageIndex];
    const nextPage = pageElements[targetPageIndex];

    currentPage.classList.remove('active');

    const onTransitionEnd = () => {
        currentPage.style.display = 'none';
        currentPage.removeEventListener('transitionend', onTransitionEnd);


        nextPage.style.display = 'block';

        requestAnimationFrame(() => {
            nextPage.classList.add('active');
        });

        currentPageIndex = targetPageIndex;
    };

    currentPage.addEventListener('transitionend', onTransitionEnd);
};

const nextPage = () => {
    // check if required dependencies are installed
    if (currentPageIndex === 1) {
        if (!validateRequiredDependencies()) {
            displayError("Please install all required dependencies before proceeding.");
            return;
        }
    }

    navigateTo(currentPageIndex + 1);
};
const prevPage = () => navigateTo(currentPageIndex - 1);

const showPage = (pageIndex, immediate = false) => {
    pageElements.forEach((page, index) => {
        if (index === pageIndex) {
            page.style.display = 'block';
            if (!immediate) {
                requestAnimationFrame(() => {
                    page.classList.add('active');
                });
            } else {
                page.classList.add('active');
            }
        } else {
            page.style.display = 'none';
            page.classList.remove('active');
        }
    });
};

const finishSetup = async () => {
    showProgress();
    try {
        await window.electronAPI.completeSetup();
        window.electronAPI.restartApp();
    } catch (error) {
        displayError(`Failed to complete setup: ${error.message}`);
    } finally {
        hideProgress();
    }
};

const showProgress = () => {
    progressOverlay.classList.add('active');
};

const hideProgress = () => {
    progressOverlay.classList.remove('active');
};

const updateProgress = (percent, status) => {
    progressFill.style.width = `${percent}%`;
    progressStatus.textContent = status;
};

const displayError = (message) => {
    alert(message);
};

window.electronAPI.onProgress((event, rawData) => {
    console.log('Raw progress data received:', rawData);
    
    try {
        // Parse the JSON string
        const data = typeof rawData === 'string' ? JSON.parse(rawData) : rawData;
        console.log('Parsed progress data:', data);
        
        if (data && typeof data === 'object') {
            const { percent, status } = data;
            updateProgress(
                percent || 0, 
                status || 'Installing...'
            );
        } else {
            console.error('Invalid progress data format:', data);
        }
    } catch (error) {
        console.error('Error processing progress data:', error);
        console.error('Received data:', rawData);
    }
});

window.electronAPI.onError((event, message) => {
    displayError(message);
});

function openManualInstallModal() {
    const modal = document.getElementById('manual-install-modal');
    modal.style.display = 'block';
    document.body.style.overflow = 'hidden';
}

function closeManualInstallModal() {
    const modal = document.getElementById('manual-install-modal');
    modal.style.display = 'none';
    document.body.style.overflow = 'auto';
}

function validateRequiredDependencies() {
    const requiredDeps = ['python', 'git', 'ffmpeg', 'ytdlp'];
    let allInstalled = true;

    requiredDeps.forEach(dep => {
        const depElement = document.getElementById(`${dep}-dep`);
        if (depElement && !depElement.classList.contains('success')) {
            allInstalled = false;
        }
    });

    const nextButton = document.querySelector('#dependencies-page .next-btn');
    if (nextButton) {
        nextButton.disabled = !allInstalled;

        if (!allInstalled) {
            nextButton.title = "Please install all required dependencies first";
        } else {
            nextButton.title = "";
        }
    }

    return allInstalled;
}

document.addEventListener('DOMContentLoaded', function() {
    const warningDiv = document.querySelector('.warning-div');
    if (!warningDiv) return;
    const manualInstallLink = warningDiv.querySelector('a');
    const manualInstallBtn = document.createElement('button');
    manualInstallBtn.classList.add('manual-install-btn');
    manualInstallBtn.textContent = 'Manual Installation Guide';
    manualInstallBtn.addEventListener('click', openManualInstallModal);
    warningDiv.innerHTML = '';
    const warningText = document.createElement('h3');
    warningText.classList.add('warning');
    warningText.textContent = 'Having trouble with automatic installation? Use our detailed manual guide:';
    warningDiv.appendChild(warningText);
    warningDiv.appendChild(manualInstallBtn);

    const modal = document.getElementById('manual-install-modal');
    if (!modal) return;

    const closeBtn = modal.querySelector('.close-modal');
    const closeModalBtn = modal.querySelector('.close-modal-btn');
    closeBtn.addEventListener('click', closeManualInstallModal);
    closeModalBtn.addEventListener('click', closeManualInstallModal);

    window.addEventListener('click', function(event) {
        if (event.target === modal) {
            closeManualInstallModal();
        }
    });

    validateRequiredDependencies();
});

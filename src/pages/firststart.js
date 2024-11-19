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

const nextPage = () => navigateTo(currentPageIndex + 1);
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

window.electronAPI.onProgress((event, { percent, status }) => {
    updateProgress(percent, status);
});

window.electronAPI.onError((event, message) => {
    displayError(message);
});

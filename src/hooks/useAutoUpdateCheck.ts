import { useEffect, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { useNotificationStore } from '@/stores/useNotificationStore';
import { logError, logInfo } from '@/utils/logger';
import { errorDetail } from '@/utils/errors';
import { tauriAPI } from '@/tauri-bridge';

const LAST_CHECK_KEY = 'mh-auto-update-last-check';
const ONE_DAY_MS = 24 * 60 * 60 * 1000;
const RECHECK_MS = 6 * 60 * 60 * 1000;

interface Options {
  enabled: boolean | undefined;
  autoDownload: boolean | undefined;
}

export function useAutoUpdateCheck({ enabled, autoDownload }: Options) {
  const addNotification = useNotificationStore((s) => s.addNotification);
  const navigate = useNavigate();
  const launchCheckDone = useRef(false);
  // Read inside the interval callback so toggling the setting takes effect
  // without tearing down and re-arming the timer.
  const autoDownloadRef = useRef(autoDownload);
  useEffect(() => {
    autoDownloadRef.current = autoDownload;
  }, [autoDownload]);

  useEffect(() => {
    if (!enabled) return;

    const offerRestart = (version: string) => {
      addNotification({
        type: 'success',
        title: `MediaHarbor ${version} is ready`,
        message: 'Restart to finish updating.',
        dedupeKey: `update:${version}`,
        duration: 0,
        cta: {
          label: 'Restart now',
          onClick: () => {
            void tauriAPI.updates.apply().catch((err: unknown) => {
              logError('system', 'Failed to install update', errorDetail(err));
            });
          },
        },
        secondaryCta: { label: 'Later', onClick: () => {} },
      });
    };

    const runCheck = async () => {
      const status = await tauriAPI.updates.status();
      if (!status.available || !status.version) return;
      const version = status.version;

      if (!status.supported) {
        addNotification({
          type: 'info',
          title: `MediaHarbor ${version} is available`,
          message: 'You are running an older release.',
          dedupeKey: `update:${version}`,
          duration: 15000,
          cta: { label: 'View release', onClick: () => navigate('/updates') },
        });
        return;
      }

      if (autoDownloadRef.current) {
        await tauriAPI.updates.download();
        logInfo('system', `Update ${version} downloaded`, 'Restart to finish updating.');
        offerRestart(version);
        return;
      }

      addNotification({
        type: 'info',
        title: `MediaHarbor ${version} is available`,
        message: 'Download and install it now?',
        dedupeKey: `update:${version}`,
        duration: 0,
        cta: {
          label: 'Install',
          onClick: () => {
            void tauriAPI.updates
              .download()
              .then(() => offerRestart(version))
              .catch((err: unknown) => {
                logError('system', 'Failed to download update', errorDetail(err));
              });
          },
        },
        secondaryCta: { label: 'Later', onClick: () => {} },
      });
    };

    const check = () => {
      void runCheck().catch((err: unknown) => {
        logError('system', 'Update check failed', errorDetail(err));
      });
    };

    // The launch check stays rate-limited across restarts; the interval covers
    // long-running sessions, which is where an update is most likely to land.
    if (!launchCheckDone.current) {
      launchCheckDone.current = true;
      const last = Number(localStorage.getItem(LAST_CHECK_KEY) ?? 0);
      if (!Number.isFinite(last) || Date.now() - last >= ONE_DAY_MS) {
        localStorage.setItem(LAST_CHECK_KEY, String(Date.now()));
        check();
      }
    }

    const timer = setInterval(() => {
      localStorage.setItem(LAST_CHECK_KEY, String(Date.now()));
      check();
    }, RECHECK_MS);
    return () => clearInterval(timer);
  }, [enabled, addNotification, navigate]);
}

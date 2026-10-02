import { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  useCredentialHealthStore,
  credentialGroup,
  dismissCredentialNotice,
  isCredentialNoticeDismissed,
  type CredentialStatus,
} from '@/stores/useCredentialHealthStore';
import { useNotificationStore } from '@/stores/useNotificationStore';
import { tauriAPI } from '@/tauri-bridge';
import { settingsTabOf } from '@/utils/platform-data';

const EXPIRING_TOAST_DAY_KEY = (platform: string) => `mh-cred-expiring-toast:${platform}`;
const ONE_DAY_MS = 24 * 60 * 60 * 1000;

export function useCredentialHealthBootstrap() {
  const setAll = useCredentialHealthStore((s) => s.setAll);
  const setOne = useCredentialHealthStore((s) => s.setOne);
  const addNotification = useNotificationStore((s) => s.addNotification);
  const dismissByDedupeKey = useNotificationStore((s) => s.dismissByDedupeKey);
  const navigate = useNavigate();

  useEffect(() => {
    let cancelled = false;

    tauriAPI.credentials
      .snapshot()
      .then((snap) => {
        if (cancelled) return;
        const fingerprints = snap.fingerprints ?? {};
        setAll(snap.services, fingerprints, snap.generatedAt);
        for (const [platform, status] of Object.entries(snap.services)) {
          maybeNotify(
            platform,
            status,
            fingerprints[platform],
            addNotification,
            dismissByDedupeKey,
            navigate
          );
        }
      })
      .catch(() => {});

    const off = tauriAPI.credentials.onStatusChanged((ev) => {
      setOne(ev.platform, ev.status, ev.fingerprint);
      maybeNotify(
        ev.platform,
        ev.status,
        ev.fingerprint ?? undefined,
        addNotification,
        dismissByDedupeKey,
        navigate
      );
    });
    return () => {
      cancelled = true;
      off?.();
    };
  }, [setAll, setOne, addNotification, dismissByDedupeKey, navigate]);
}

function maybeNotify(
  platform: string,
  status: CredentialStatus,
  fingerprint: string | undefined,
  addNotification: ReturnType<typeof useNotificationStore.getState>['addNotification'],
  dismissByDedupeKey: ReturnType<typeof useNotificationStore.getState>['dismissByDedupeKey'],
  navigate: ReturnType<typeof useNavigate>
) {
  const { key: groupKey, display } = credentialGroup(platform);
  const dedupeKey = `cred:${groupKey}`;
  const goToSettings = () => navigate(`/settings?tab=${settingsTabOf(platform)}`);

  if (status.kind === 'ok' || status.kind === 'notConfigured') {
    dismissByDedupeKey(dedupeKey);
    return;
  }

  if (isCredentialNoticeDismissed(groupKey, fingerprint)) return;

  const dontShowAgain = fingerprint
    ? { label: "Don't show again", onClick: () => dismissCredentialNotice(groupKey, fingerprint) }
    : undefined;

  if (status.kind === 'expired') {
    addNotification({
      type: 'error',
      title: `${display} sign-in expired`,
      message: `Reconnect in Settings to keep using ${display}.`,
      duration: 0,
      cta: { label: 'Open Settings', onClick: goToSettings },
      secondaryCta: dontShowAgain,
      dedupeKey,
    });
    return;
  }

  if (status.kind === 'expiringSoon') {
    const last = Number(localStorage.getItem(EXPIRING_TOAST_DAY_KEY(groupKey)) ?? '0');
    if (Date.now() - last < ONE_DAY_MS) return;
    localStorage.setItem(EXPIRING_TOAST_DAY_KEY(groupKey), String(Date.now()));
    const days = Math.max(1, Math.floor((status.expiresAt * 1000 - Date.now()) / ONE_DAY_MS));
    addNotification({
      type: 'warning',
      title: `${display} sign-in expires soon`,
      message: `Expires in ${days} day${days === 1 ? '' : 's'}. Reconnect when convenient.`,
      duration: 8000,
      cta: { label: 'Open Settings', onClick: goToSettings },
      secondaryCta: dontShowAgain,
      dedupeKey,
    });
  }
}

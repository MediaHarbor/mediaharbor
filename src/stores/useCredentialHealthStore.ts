import { create } from 'zustand';
import { devtools } from 'zustand/middleware';
import type { CredentialStatus } from '@/tauri-bridge';
import { PLATFORM_LABELS, toClientPlatform } from '@/utils/platform-data';

export type { CredentialStatus };

interface CredentialHealthState {
  statuses: Record<string, CredentialStatus>;
  fingerprints: Record<string, string>;
  generatedAt: number;
  setAll: (
    statuses: Record<string, CredentialStatus>,
    fingerprints: Record<string, string>,
    generatedAt: number
  ) => void;
  setOne: (platform: string, status: CredentialStatus, fingerprint?: string | null) => void;
}

export const useCredentialHealthStore = create<CredentialHealthState>()(
  devtools(
    (set) => ({
      statuses: {},
      fingerprints: {},
      generatedAt: 0,
      setAll: (statuses, fingerprints, generatedAt) => set({ statuses, fingerprints, generatedAt }),
      setOne: (platform, status, fingerprint) =>
        set((s) => ({
          statuses: { ...s.statuses, [platform]: status },
          fingerprints: fingerprint
            ? { ...s.fingerprints, [platform]: fingerprint }
            : s.fingerprints,
        })),
    }),
    { name: 'CredentialHealthStore' }
  )
);

export function useCredentialStatus(platform: string): CredentialStatus | undefined {
  return useCredentialHealthStore((s) => s.statuses[platform]);
}

const DISMISS_KEY = (platform: string) => `mh-cred-dismissed:${platform}`;

export function isCredentialNoticeDismissed(platform: string, fingerprint?: string): boolean {
  if (!fingerprint) return false;
  try {
    return localStorage.getItem(DISMISS_KEY(platform)) === fingerprint;
  } catch {
    return false;
  }
}

export function dismissCredentialNotice(platform: string, fingerprint?: string) {
  if (!fingerprint) return;
  try {
    localStorage.setItem(DISMISS_KEY(platform), fingerprint);
  } catch {
    void 0;
  }
}

const SHARED_CREDENTIAL_GROUPS: Record<string, { key: string; display: string }> = {
  ytmusic: { key: 'youtube', display: 'YouTube & YouTube Music' },
  youtube: { key: 'youtube', display: 'YouTube & YouTube Music' },
};

export function credentialGroup(platform: string): { key: string; display: string } {
  return (
    SHARED_CREDENTIAL_GROUPS[platform] ?? {
      key: platform,
      display: PLATFORM_LABELS[toClientPlatform(platform)] ?? platform,
    }
  );
}

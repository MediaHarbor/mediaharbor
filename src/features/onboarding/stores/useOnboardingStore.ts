import { create } from 'zustand';
import { invalidateSettingsDerived } from '@/lib/queryClient';
import { applyThemePreference } from '@/stores/useThemeStore';
import { useTourStore } from './useTourStore';
import { tauriAPI } from '@/tauri-bridge';

interface OnboardingState {
  isOpen: boolean;
  currentStep: number;
  direction: number;
  downloadLocation: string;
  theme: 'auto' | 'dark' | 'light';
  enabledServices: string[];
  open(): void;
  close(): void;
  nextStep(): void;
  prevStep(): void;
  setDownloadLocation(path: string): void;
  setTheme(t: 'auto' | 'dark' | 'light'): void;
  toggleService(platform: string): void;
  finishWizard(opts?: { startTour?: boolean }): Promise<void>;
}

const TOTAL_STEPS = 6;

export const useOnboardingStore = create<OnboardingState>((set, get) => ({
  isOpen: false,
  currentStep: 0,
  direction: 1,
  downloadLocation: '',
  theme: 'auto',
  enabledServices: [],

  open: () => set({ isOpen: true, currentStep: 0, direction: 1 }),
  close: () => set({ isOpen: false }),

  nextStep: () =>
    set((s) => ({
      currentStep: Math.min(s.currentStep + 1, TOTAL_STEPS - 1),
      direction: 1,
    })),

  prevStep: () =>
    set((s) => ({
      currentStep: Math.max(s.currentStep - 1, 0),
      direction: -1,
    })),

  setDownloadLocation: (path) => set({ downloadLocation: path }),

  setTheme: (t) => set({ theme: t }),

  toggleService: (platform) =>
    set((s) => ({
      enabledServices: s.enabledServices.includes(platform)
        ? s.enabledServices.filter((p) => p !== platform)
        : [...s.enabledServices, platform],
    })),

  finishWizard: async ({ startTour = true } = {}) => {
    const { downloadLocation, theme, enabledServices } = get();
    set({ isOpen: false });
    const data = await tauriAPI.settings.get().catch(() => null);
    if (data) {
      await tauriAPI.settings
        .set({
          ...data,
          downloadLocation: downloadLocation || data.downloadLocation,
          theme,
          enabledServices,
        })
        .catch(() => null);
      invalidateSettingsDerived();
    }
    applyThemePreference(theme);

    if (startTour) {
      setTimeout(() => useTourStore.getState().start(), 250);
    } else {
      await useTourStore.getState().persistCompleted();
    }
  },
}));

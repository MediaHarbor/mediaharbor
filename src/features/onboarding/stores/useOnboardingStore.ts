import { create } from 'zustand';
import { useThemeStore } from '@/stores/useThemeStore';
import { useTourStore } from './useTourStore';

interface OnboardingState {
  isOpen: boolean;
  currentStep: number;
  direction: number;
  downloadLocation: string;
  theme: 'auto' | 'dark' | 'light';
  open(): void;
  close(): void;
  nextStep(): void;
  prevStep(): void;
  setDownloadLocation(path: string): void;
  setTheme(t: 'auto' | 'dark' | 'light'): void;
  finishWizard(opts?: { startTour?: boolean }): Promise<void>;
}

const TOTAL_STEPS = 5;

export const useOnboardingStore = create<OnboardingState>((set, get) => ({
  isOpen: false,
  currentStep: 0,
  direction: 1,
  downloadLocation: '',
  theme: 'auto',

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

  finishWizard: async ({ startTour = true } = {}) => {
    const { downloadLocation, theme } = get();
    set({ isOpen: false });
    const data = await window.electron?.settings.get().catch(() => null);
    if (data) {
      await window.electron?.settings
        .set({
          ...data,
          downloadLocation: downloadLocation || data.downloadLocation,
          theme,
        })
        .catch(() => null);
    }
    const resolvedTheme =
      theme === 'auto'
        ? window.matchMedia('(prefers-color-scheme: dark)').matches
          ? 'dark'
          : 'light'
        : theme;
    useThemeStore.getState().setTheme(resolvedTheme);

    if (startTour) {
      setTimeout(() => useTourStore.getState().start(), 250);
    } else {
      await useTourStore.getState().persistCompleted();
    }
  },
}));

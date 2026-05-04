import { create } from 'zustand';
import { TOUR_STEPS } from '../tour/steps';

interface TourState {
  isOpen: boolean;
  currentStep: number;
  start(): void;
  next(): void;
  prev(): void;
  end(): Promise<void>;
  persistCompleted(): Promise<void>;
}

export const useTourStore = create<TourState>((set, get) => ({
  isOpen: false,
  currentStep: 0,

  start: () => set({ isOpen: true, currentStep: 0 }),

  next: () => {
    const { currentStep } = get();
    if (currentStep >= TOUR_STEPS.length - 1) {
      void get().end();
      return;
    }
    set({ currentStep: currentStep + 1 });
  },

  prev: () =>
    set((s) => ({ currentStep: Math.max(0, s.currentStep - 1) })),

  end: async () => {
    set({ isOpen: false, currentStep: 0 });
    await get().persistCompleted();
  },

  persistCompleted: async () => {
    const data = await window.electron?.settings.get().catch(() => null);
    if (!data) return;
    await window.electron?.settings
      .set({ ...data, onboarding_completed: true })
      .catch(() => null);
  },
}));

import { create } from 'zustand';
import { persist } from 'zustand/middleware';

type Theme = 'light' | 'dark';

export type ThemePreference = 'auto' | Theme;

interface ThemeState {
  theme: Theme;
  setTheme: (theme: Theme) => void;
}

export const useThemeStore = create<ThemeState>()(
  persist(
    (set) => ({
      theme: 'dark',
      setTheme: (theme) => set({ theme }),
    }),
    {
      name: 'mediaharbor-theme',
    }
  )
);

const prefersDark = () => window.matchMedia('(prefers-color-scheme: dark)');

export function resolveThemePreference(pref: ThemePreference): Theme {
  return pref === 'auto' ? (prefersDark().matches ? 'dark' : 'light') : pref;
}

export function applyThemePreference(pref: ThemePreference) {
  useThemeStore.getState().setTheme(resolveThemePreference(pref));
}

export function watchSystemTheme(): () => void {
  const mq = prefersDark();
  const handler = (e: MediaQueryListEvent) =>
    useThemeStore.getState().setTheme(e.matches ? 'dark' : 'light');
  mq.addEventListener('change', handler);
  return () => mq.removeEventListener('change', handler);
}

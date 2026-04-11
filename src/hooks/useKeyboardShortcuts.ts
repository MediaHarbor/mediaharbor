import { useEffect } from 'react';
import { usePlayerStore } from '@/stores/usePlayerStore';

export function useKeyboardShortcuts() {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (
        target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable
      ) return;

      const { isPlaying, setPlaying, streamUrl, toggleNowPlaying } = usePlayerStore.getState();

      switch (e.code) {
        case 'Space':
          if (streamUrl) {
            e.preventDefault();
            setPlaying(!isPlaying);
          }
          break;
        case 'KeyN':
          if (streamUrl) {
            e.preventDefault();
            toggleNowPlaying();
          }
          break;
      }
    };

    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []);
}

import { useEffect } from 'react';
import { usePlayerStore } from '@/stores/usePlayerStore';
import { useLibraryBack } from '@/features/library/hooks/useLibraryBack';

export function useKeyboardShortcuts() {
  const { canGoBack, goBack } = useLibraryBack();

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)
        return;

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
        case 'ArrowLeft':
          if (e.altKey && canGoBack) {
            e.preventDefault();
            goBack();
          }
          break;
      }
    };

    const mouseHandler = (e: MouseEvent) => {
      if (e.button !== 3) return;
      if (!canGoBack) return;
      e.preventDefault();
      goBack();
    };

    window.addEventListener('keydown', handler);
    window.addEventListener('mouseup', mouseHandler);
    return () => {
      window.removeEventListener('keydown', handler);
      window.removeEventListener('mouseup', mouseHandler);
    };
  }, [canGoBack, goBack]);
}

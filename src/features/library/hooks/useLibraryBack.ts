import { useCallback } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useLibraryStore } from '@/stores/useLibraryStore';

export interface LibraryBack {
  canGoBack: boolean;
  goBack: () => void;
}

export function useLibraryBack(): LibraryBack {
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const depth = useLibraryStore((s) => s.viewStack.length);
  const returnPath = useLibraryStore((s) => s.returnPath);
  const popView = useLibraryStore((s) => s.popView);
  const clearViews = useLibraryStore((s) => s.clearViews);

  const active = pathname === '/library' && depth > 0;

  const goBack = useCallback(() => {
    if (!active) return;
    if (depth === 1 && returnPath) {
      clearViews();
      navigate(returnPath);
      return;
    }
    popView();
  }, [active, depth, returnPath, clearViews, navigate, popView]);

  return { canGoBack: active, goBack };
}

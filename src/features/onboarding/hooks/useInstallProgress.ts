import { useState, useEffect } from 'react';

export function useInstallProgress(): Map<string, { percent: number; status: string }> {
  const [progressMap, setProgressMap] = useState<Map<string, { percent: number; status: string }>>(
    new Map()
  );

  useEffect(() => {
    const unsub = window.electron?.updates.onInstallProgress((data) => {
      setProgressMap((prev) => {
        const next = new Map(prev);
        next.set(data.dependency, { percent: data.percent, status: data.status });
        return next;
      });
    });
    return () => unsub?.();
  }, []);

  return progressMap;
}

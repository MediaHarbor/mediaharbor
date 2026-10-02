import { useEffect, useMemo, useState } from 'react';
import { resolveCoverUrl } from './api';

export function useCoverUrls(
  coverIds: (string | null | undefined)[]
): Record<string, string | null> {
  const [urls, setUrls] = useState<Record<string, string | null>>({});

  useEffect(() => {
    let cancelled = false;
    const ids = Array.from(
      new Set(coverIds.filter((c): c is string => typeof c === 'string' && c.length > 0))
    );
    if (ids.length === 0) return;
    Promise.all(ids.map((id) => resolveCoverUrl(id).then((u) => [id, u] as const))).then(
      (pairs) => {
        if (cancelled) return;
        setUrls((prev) => {
          let changed = false;
          const next = { ...prev };
          for (const [id, url] of pairs) {
            const value = url ?? null;
            if (next[id] !== value) {
              next[id] = value;
              changed = true;
            }
          }
          return changed ? next : prev;
        });
      }
    );
    return () => {
      cancelled = true;
    };
  }, [coverIds]);

  return urls;
}

export function useVisibleCoverUrls(
  coverIds: (string | null | undefined)[]
): Record<string, string | null> {
  const key = [
    ...new Set(coverIds.filter((c): c is string => typeof c === 'string' && c.length > 0)),
  ]
    .sort()
    .join('|');
  const stableIds = useMemo<(string | null | undefined)[]>(
    () => (key === '' ? [] : key.split('|')),
    [key]
  );
  return useCoverUrls(stableIds);
}

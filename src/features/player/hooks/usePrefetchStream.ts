import { useCallback, useRef } from 'react';

import { tauriAPI } from '@/tauri-bridge';
import { usePlayerStore } from '@/stores/usePlayerStore';
import { toIpcPlatform } from '@/utils/platform-data';

interface PrefetchTarget {
  url: string;
  platform?: string | null;
}

interface PrefetchOptions {
  /**
   * Re-checked when the fetch returns. The queue can move while a prefetch is
   * in flight, and caching a stream nobody is going to play wastes the slot.
   */
  stillWanted?: () => boolean;
  onCached?: () => void;
  onError?: () => void;
}

/**
 * Prefetches the stream for an upcoming track, at most once per URL.
 *
 * The in-flight set is owned here so the idle timer, the media-element
 * crossfade and the native crossfade cannot each start the same fetch.
 */
export function usePrefetchStream() {
  const inFlightRef = useRef<Set<string>>(new Set());

  const prefetch = useCallback((track: PrefetchTarget, opts: PrefetchOptions = {}): boolean => {
    const { url } = track;
    if (!url) return false;
    if (usePlayerStore.getState().getCachedStream(url)) return false;
    if (inFlightRef.current.has(url)) return false;

    inFlightRef.current.add(url);
    tauriAPI.player
      .prefetchMedia({ url, platform: toIpcPlatform(track.platform ?? '') })
      .then((data) => {
        if (!data?.streamUrl) return;
        if (opts.stillWanted && !opts.stillWanted()) return;
        usePlayerStore.getState().cacheStream(url, data.streamUrl, data.mediaType, data.isLive);
        opts.onCached?.();
      })
      .catch(() => opts.onError?.())
      .finally(() => {
        inFlightRef.current.delete(url);
      });
    return true;
  }, []);

  return prefetch;
}

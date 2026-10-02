import { useRef, useEffect } from 'react';

import { usePlayerStore } from '@/stores/usePlayerStore';

/**
 * Drives word-accurate lyric highlighting at frame rate.
 *
 * Position updates arrive ~10x per second, which is far too coarse for
 * karaoke, so each one becomes an anchor that the loop extrapolates from using
 * a wall clock. Re-anchoring on every update keeps it from drifting — the
 * player's clock counts frames actually written to the device, so it stays
 * truer than the media element's own `currentTime` ever did.
 */
export function useMediaSyncLoop(tick: (time: number) => void) {
  const rafRef = useRef<number>(0);
  const anchorRef = useRef<{ position: number; perfTime: number } | null>(null);
  const playingRef = useRef(false);
  const tickRef = useRef(tick);

  useEffect(() => {
    tickRef.current = tick;
  }, [tick]);

  useEffect(() => {
    const getTime = () => {
      const anchor = anchorRef.current;
      if (!anchor) return usePlayerStore.getState().position;
      if (!playingRef.current) return anchor.position;
      return Math.max(0, anchor.position + (performance.now() - anchor.perfTime) / 1000);
    };

    const loop = () => {
      tickRef.current(getTime());
      rafRef.current = playingRef.current ? requestAnimationFrame(loop) : 0;
    };

    const start = () => {
      if (rafRef.current) return;
      rafRef.current = requestAnimationFrame(loop);
    };

    const unsubscribe = usePlayerStore.subscribe((state, prev) => {
      if (state.position !== prev.position) {
        anchorRef.current = { position: state.position, perfTime: performance.now() };
        if (!playingRef.current) tickRef.current(getTime());
      }
      if (state.isPlaying !== prev.isPlaying) {
        playingRef.current = state.isPlaying;
        anchorRef.current = { position: state.position, perfTime: performance.now() };
        if (state.isPlaying) start();
        else tickRef.current(getTime());
      }
    });

    const initial = usePlayerStore.getState();
    playingRef.current = initial.isPlaying;
    anchorRef.current = { position: initial.position, perfTime: performance.now() };
    tickRef.current(getTime());
    if (playingRef.current) start();

    return () => {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = 0;
      unsubscribe();
    };
  }, []);
}

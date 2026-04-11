import { useRef, useEffect } from 'react';

export function useMediaSyncLoop(
  tick: (time: number) => void,
  mediaElement: HTMLMediaElement | null,
) {
  const rafRef = useRef<number>(0);
  const anchorRef = useRef<{ mediaTime: number; perfTime: number } | null>(null);

  useEffect(() => {
    if (!mediaElement) return;
    anchorRef.current = null;

    const getTime = () => {
      const anchor = anchorRef.current;
      return (anchor && !mediaElement.paused && !mediaElement.ended)
        ? Math.max(0, anchor.mediaTime + (performance.now() - anchor.perfTime) / 1000)
        : mediaElement.currentTime;
    };

    const loop = () => {
      tick(getTime());
      if (!mediaElement.paused && !mediaElement.ended) {
        rafRef.current = requestAnimationFrame(loop);
      }
    };

    const onPlay = () => {
      anchorRef.current = { mediaTime: mediaElement.currentTime, perfTime: performance.now() };
      cancelAnimationFrame(rafRef.current);
      rafRef.current = requestAnimationFrame(loop);
    };

    const onSeeked = () => {
      anchorRef.current = { mediaTime: mediaElement.currentTime, perfTime: performance.now() };
      cancelAnimationFrame(rafRef.current);
      tick(getTime());
      if (!mediaElement.paused) {
        rafRef.current = requestAnimationFrame(loop);
      }
    };

    mediaElement.addEventListener('playing', onPlay);
    mediaElement.addEventListener('seeked', onSeeked);

    if (!mediaElement.paused && !mediaElement.ended) {
      anchorRef.current = { mediaTime: mediaElement.currentTime, perfTime: performance.now() };
      rafRef.current = requestAnimationFrame(loop);
    } else {
      tick(mediaElement.currentTime);
    }

    return () => {
      cancelAnimationFrame(rafRef.current);
      mediaElement.removeEventListener('playing', onPlay);
      mediaElement.removeEventListener('seeked', onSeeked);
    };
  }, [mediaElement, tick]);
}

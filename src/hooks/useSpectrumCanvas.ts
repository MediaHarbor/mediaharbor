import { useCallback, useEffect, useRef } from 'react';
import { getSpectrum, subscribeSpectrum } from '@/utils/audioAnalyser';
import { parseHex } from '@/utils/color';

export const BAR_COUNT = 64;
const SMOOTHING = 0.8;
/// Below this a bar is visually at rest, so a paused visualizer can stop drawing.
const AT_REST = 0.001;

export interface SpectrumFrame {
  ctx: CanvasRenderingContext2D;
  /// CSS pixels — the DPR transform is already applied.
  w: number;
  h: number;
  /// Smoothed 0-1 magnitude per bar, already blended against the previous frame.
  bars: Float32Array;
  rgb: [number, number, number];
}

/**
 * Drives a spectrum-analyser canvas: sizing, DPR, smoothing and the animation frame.
 *
 * Both visualisers had written this out — the same five refs, the same colour-sync
 * effect, the same bail-and-reschedule blocks, the same `ResizeObserver`, the same
 * smoothing loop — leaving only the ~20 lines that actually paint different.
 *
 * The loop runs only while something is playing, and then for as long as it takes the
 * bars to settle. Previously it re-armed unconditionally, so a paused Now Playing
 * screen still rebuilt 64 gradients and ran 64 shadow-blurred fills sixty times a
 * second, forever.
 */
export function useSpectrumCanvas(
  platformColor: string,
  isPlaying: boolean,
  paint: (frame: SpectrumFrame) => void
) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animRef = useRef(0);
  const prevBarsRef = useRef(new Float32Array(BAR_COUNT));
  const sizeRef = useRef({ w: 0, h: 0 });
  const colorRef = useRef(parseHex(platformColor));
  const paintRef = useRef(paint);
  const playingRef = useRef(isPlaying);

  useEffect(() => {
    colorRef.current = parseHex(platformColor);
  }, [platformColor]);

  useEffect(() => {
    paintRef.current = paint;
  });

  useEffect(() => subscribeSpectrum(), []);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext('2d');
    const { w, h } = sizeRef.current;
    if (!canvas || !ctx || w === 0 || h === 0) {
      animRef.current = requestAnimationFrame(draw);
      return;
    }

    const dpr = window.devicePixelRatio || 1;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const bars = prevBarsRef.current;
    const spectrum = getSpectrum();
    let peak = 0;
    for (let i = 0; i < BAR_COUNT; i++) {
      bars[i] = bars[i] * SMOOTHING + ((spectrum[i] ?? 0) / 255) * (1 - SMOOTHING);
      if (bars[i] > peak) peak = bars[i];
    }

    paintRef.current({ ctx, w, h, bars, rgb: colorRef.current });

    if (playingRef.current || peak > AT_REST) {
      animRef.current = requestAnimationFrame(draw);
    } else {
      animRef.current = 0;
    }
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      const pw = Math.round(rect.width * dpr);
      const ph = Math.round(rect.height * dpr);
      if (canvas.width !== pw || canvas.height !== ph) {
        canvas.width = pw;
        canvas.height = ph;
        sizeRef.current = { w: rect.width, h: rect.height };
      }
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    playingRef.current = isPlaying;
    if (isPlaying && animRef.current === 0) {
      animRef.current = requestAnimationFrame(draw);
    }
  }, [isPlaying, draw]);

  useEffect(() => {
    prevBarsRef.current = new Float32Array(BAR_COUNT);
    animRef.current = requestAnimationFrame(draw);
    return () => {
      cancelAnimationFrame(animRef.current);
      animRef.current = 0;
    };
  }, [draw]);

  return canvasRef;
}

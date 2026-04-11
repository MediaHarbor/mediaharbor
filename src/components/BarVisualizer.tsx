import { useRef, useEffect, useCallback } from 'react';
import { getAnalyser } from '@/utils/audioAnalyser';

const BAR_COUNT = 64;
const SMOOTHING = 0.80;

function parseHex(hex: string): [number, number, number] {
  const c = hex.replace('#', '');
  return [
    parseInt(c.slice(0, 2), 16) || 136,
    parseInt(c.slice(2, 4), 16) || 136,
    parseInt(c.slice(4, 6), 16) || 136,
  ];
}

interface Props {
  platformColor: string;
}

export function BarVisualizer({ platformColor }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animRef = useRef(0);
  const prevBarsRef = useRef(new Float32Array(BAR_COUNT));
  const dataRef = useRef<Uint8Array<ArrayBuffer> | null>(null);
  const colorRef = useRef(parseHex(platformColor));

  useEffect(() => {
    colorRef.current = parseHex(platformColor);
  }, [platformColor]);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) { animRef.current = requestAnimationFrame(draw); return; }
    const ctx = canvas.getContext('2d');
    if (!ctx) { animRef.current = requestAnimationFrame(draw); return; }

    const dpr = window.devicePixelRatio || 1;
    const w = canvas.width / dpr;
    const h = canvas.height / dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const analyser = getAnalyser();
    const rawBars = new Float32Array(BAR_COUNT);

    if (analyser) {
      const len = analyser.frequencyBinCount;
      if (!dataRef.current || dataRef.current.length !== len) {
        dataRef.current = new Uint8Array(len);
      }
      analyser.getByteFrequencyData(dataRef.current);
      for (let i = 0; i < BAR_COUNT; i++) {
        const t = i / BAR_COUNT;
        const idx = Math.floor(t * t * len * 0.85);
        rawBars[i] = dataRef.current[Math.min(idx, len - 1)] / 255;
      }
    }

    const prev = prevBarsRef.current;
    const [r, g, b] = colorRef.current;
    const totalW = w;
    const barW = totalW / BAR_COUNT;
    const gap = Math.max(1, barW * 0.22);

    ctx.shadowBlur = 8;
    ctx.shadowColor = `rgba(${r}, ${g}, ${b}, 0.3)`;

    for (let i = 0; i < BAR_COUNT; i++) {
      const smoothed = prev[i] * SMOOTHING + rawBars[i] * (1 - SMOOTHING);
      prev[i] = smoothed;
      const barH = Math.max(3, smoothed * h * 0.92);
      const x = i * barW + gap / 2;
      const alpha = 0.3 + smoothed * 0.7;

      const grad = ctx.createLinearGradient(0, h - barH, 0, h);
      grad.addColorStop(0, `rgba(${r}, ${g}, ${b}, ${alpha})`);
      grad.addColorStop(1, `rgba(${r}, ${g}, ${b}, ${alpha * 0.25})`);

      ctx.fillStyle = grad;
      ctx.beginPath();
      const bw = barW - gap;
      const radius = Math.min(bw / 2, 3);
      ctx.roundRect(x, h - barH, bw, barH, [radius, radius, 0, 0]);
      ctx.fill();
    }

    animRef.current = requestAnimationFrame(draw);
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.round(rect.width * dpr);
      canvas.height = Math.round(rect.height * dpr);
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    prevBarsRef.current = new Float32Array(BAR_COUNT);
    animRef.current = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(animRef.current);
  }, [draw]);

  return <canvas ref={canvasRef} className="w-full h-full" />;
}

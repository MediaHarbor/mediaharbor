import { useRef, useEffect, useCallback } from 'react';
import { ensureAnalyser, getAnalyser, resumeAudioContext } from '@/utils/audioAnalyser';

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
  mediaElement: HTMLMediaElement | null;
  isPlaying: boolean;
}

export function RadialVisualizer({ platformColor, mediaElement, isPlaying: _isPlaying }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animRef = useRef(0);
  const prevBarsRef = useRef(new Float32Array(BAR_COUNT));
  const dataRef = useRef<Uint8Array<ArrayBuffer> | null>(null);
  const sizeRef = useRef({ w: 0, h: 0 });
  const colorRef = useRef(parseHex(platformColor));

  useEffect(() => {
    colorRef.current = parseHex(platformColor);
  }, [platformColor]);

  useEffect(() => {
    if (!mediaElement) return;
    ensureAnalyser(mediaElement);
    resumeAudioContext();
  }, [mediaElement]);

  useEffect(() => {
    const resume = () => resumeAudioContext();
    document.addEventListener('click', resume, { once: true });
    document.addEventListener('keydown', resume, { once: true });
    return () => {
      document.removeEventListener('click', resume);
      document.removeEventListener('keydown', resume);
    };
  }, []);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) { animRef.current = requestAnimationFrame(draw); return; }
    const ctx2d = canvas.getContext('2d');
    if (!ctx2d) { animRef.current = requestAnimationFrame(draw); return; }
    const { w, h } = sizeRef.current;
    if (w === 0 || h === 0) { animRef.current = requestAnimationFrame(draw); return; }

    const dpr = window.devicePixelRatio || 1;
    ctx2d.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx2d.clearRect(0, 0, w, h);

    const cx = w / 2;
    const cy = h / 2;
    const halfMin = Math.min(cx, cy);
    const innerR = halfMin * 0.72;
    const maxBarH = halfMin * 0.24;
    const prev = prevBarsRef.current;
    const [r, g, b] = colorRef.current;

    const rawBars = new Float32Array(BAR_COUNT);
    const analyser = getAnalyser();

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

    const angleStep = (Math.PI * 2) / BAR_COUNT;
    const gap = angleStep * 0.25;
    const barArc = angleStep - gap;

    ctx2d.save();
    ctx2d.shadowBlur = 18;
    ctx2d.shadowColor = `rgba(${r}, ${g}, ${b}, 0.5)`;

    for (let i = 0; i < BAR_COUNT; i++) {
      const smoothed = prev[i] * SMOOTHING + rawBars[i] * (1 - SMOOTHING);
      prev[i] = smoothed;

      const barH = Math.max(4, smoothed * maxBarH);
      const startAngle = i * angleStep - Math.PI / 2;
      const endAngle = startAngle + barArc;
      const outerR = innerR + barH;
      const alpha = 0.55 + smoothed * 0.45;

      const grad = ctx2d.createRadialGradient(cx, cy, innerR, cx, cy, outerR);
      grad.addColorStop(0, `rgba(${r}, ${g}, ${b}, ${alpha})`);
      grad.addColorStop(1, `rgba(${r}, ${g}, ${b}, ${alpha * 0.2})`);

      ctx2d.beginPath();
      ctx2d.arc(cx, cy, innerR, startAngle, endAngle);
      ctx2d.arc(cx, cy, outerR, endAngle, startAngle, true);
      ctx2d.closePath();
      ctx2d.fillStyle = grad;
      ctx2d.fill();
    }
    ctx2d.restore();

    ctx2d.beginPath();
    ctx2d.arc(cx, cy, innerR, 0, Math.PI * 2);
    ctx2d.strokeStyle = `rgba(${r}, ${g}, ${b}, 0.10)`;
    ctx2d.lineWidth = 1;
    ctx2d.stroke();

    animRef.current = requestAnimationFrame(draw);
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
    prevBarsRef.current = new Float32Array(BAR_COUNT);
    animRef.current = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(animRef.current);
  }, [draw]);

  return (
    <canvas
      ref={canvasRef}
      className="absolute inset-0 w-full h-full"
    />
  );
}

import { useCallback } from 'react';
import { BAR_COUNT, useSpectrumCanvas, type SpectrumFrame } from '@/hooks/useSpectrumCanvas';

interface Props {
  platformColor: string;
  isPlaying: boolean;
}

export function RadialVisualizer({ platformColor, isPlaying }: Props) {
  const paint = useCallback(({ ctx, w, h, bars, rgb: [r, g, b] }: SpectrumFrame) => {
    const cx = w / 2;
    const cy = h / 2;
    const halfMin = Math.min(cx, cy);
    const innerR = halfMin * 0.72;
    const maxBarH = halfMin * 0.24;

    const angleStep = (Math.PI * 2) / BAR_COUNT;
    const gap = angleStep * 0.25;
    const barArc = angleStep - gap;

    ctx.save();
    ctx.shadowBlur = 18;
    ctx.shadowColor = `rgba(${r}, ${g}, ${b}, 0.5)`;

    for (let i = 0; i < BAR_COUNT; i++) {
      const smoothed = bars[i];
      const barH = Math.max(4, smoothed * maxBarH);
      const startAngle = i * angleStep - Math.PI / 2;
      const endAngle = startAngle + barArc;
      const outerR = innerR + barH;
      const alpha = 0.55 + smoothed * 0.45;

      const grad = ctx.createRadialGradient(cx, cy, innerR, cx, cy, outerR);
      grad.addColorStop(0, `rgba(${r}, ${g}, ${b}, ${alpha})`);
      grad.addColorStop(1, `rgba(${r}, ${g}, ${b}, ${alpha * 0.2})`);

      ctx.beginPath();
      ctx.arc(cx, cy, innerR, startAngle, endAngle);
      ctx.arc(cx, cy, outerR, endAngle, startAngle, true);
      ctx.closePath();
      ctx.fillStyle = grad;
      ctx.fill();
    }
    ctx.restore();

    ctx.beginPath();
    ctx.arc(cx, cy, innerR, 0, Math.PI * 2);
    ctx.strokeStyle = `rgba(${r}, ${g}, ${b}, 0.10)`;
    ctx.lineWidth = 1;
    ctx.stroke();
  }, []);

  const canvasRef = useSpectrumCanvas(platformColor, isPlaying, paint);
  return <canvas ref={canvasRef} className="absolute inset-0 w-full h-full" />;
}

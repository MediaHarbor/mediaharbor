import { useCallback } from 'react';
import { BAR_COUNT, useSpectrumCanvas, type SpectrumFrame } from '@/hooks/useSpectrumCanvas';

interface Props {
  platformColor: string;
  isPlaying: boolean;
}

export function BarVisualizer({ platformColor, isPlaying }: Props) {
  const paint = useCallback(({ ctx, w, h, bars, rgb: [r, g, b] }: SpectrumFrame) => {
    const barW = w / BAR_COUNT;
    const gap = Math.max(1, barW * 0.22);

    ctx.shadowBlur = 8;
    ctx.shadowColor = `rgba(${r}, ${g}, ${b}, 0.3)`;

    for (let i = 0; i < BAR_COUNT; i++) {
      const smoothed = bars[i];
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
  }, []);

  const canvasRef = useSpectrumCanvas(platformColor, isPlaying, paint);
  return <canvas ref={canvasRef} className="w-full h-full" />;
}

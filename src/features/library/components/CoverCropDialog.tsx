import { useCallback, useEffect, useRef, useState } from 'react';
import { errorMessage } from '@/utils/errors';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import { tauriAPI } from '@/tauri-bridge';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  file: File | null;
  onConfirm: (jpegBase64: string) => Promise<void>;
}

const BOX = 320;
const OUT = 640;

export function CoverCropDialog({ open, onOpenChange, file, onConfirm }: Props) {
  const [img, setImg] = useState<HTMLImageElement | null>(null);
  const [zoom, setZoom] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dragRef = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    if (!open || !file) return;
    setError(null);
    setZoom(1);
    setOffset({ x: 0, y: 0 });
    setImg(null);
    let revoked: string | null = null;
    let cancelled = false;

    const loadFromUrl = (url: string, cleanup?: () => void) => {
      const image = new Image();
      image.onload = () => {
        if (!cancelled) setImg(image);
        cleanup?.();
      };
      image.onerror = () => {
        cleanup?.();
        void file
          .arrayBuffer()
          .then((buf) => tauriAPI.normalizeCoverImage?.(new Uint8Array(buf)))
          .then((b64) => {
            if (!b64) throw new Error('decode failed');
            const jpeg = new Image();
            jpeg.onload = () => {
              if (!cancelled) setImg(jpeg);
            };
            jpeg.onerror = () => setError('Could not read this image. Try a PNG or JPEG.');
            jpeg.src = `data:image/jpeg;base64,${b64}`;
          })
          .catch(() => setError('Could not read this image. Try a PNG or JPEG.'));
      };
      image.src = url;
    };

    const url = URL.createObjectURL(file);
    revoked = url;
    loadFromUrl(url, () => {
      if (revoked) URL.revokeObjectURL(revoked);
    });
    return () => {
      cancelled = true;
    };
  }, [open, file]);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas || !img) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, BOX, BOX);
    const base = Math.max(BOX / img.width, BOX / img.height);
    const scale = base * zoom;
    const dw = img.width * scale;
    const dh = img.height * scale;
    const dx = (BOX - dw) / 2 + offset.x;
    const dy = (BOX - dh) / 2 + offset.y;
    ctx.drawImage(img, dx, dy, dw, dh);
  }, [img, zoom, offset]);

  useEffect(() => {
    draw();
  }, [draw]);

  const onPointerDown = (e: React.PointerEvent) => {
    dragRef.current = { x: e.clientX - offset.x, y: e.clientY - offset.y };
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (!dragRef.current) return;
    setOffset({ x: e.clientX - dragRef.current.x, y: e.clientY - dragRef.current.y });
  };
  const onPointerUp = () => {
    dragRef.current = null;
  };

  const confirm = async () => {
    if (!img) return;
    setBusy(true);
    try {
      const out = document.createElement('canvas');
      out.width = OUT;
      out.height = OUT;
      const ctx = out.getContext('2d');
      if (!ctx) throw new Error('canvas unavailable');
      const ratio = OUT / BOX;
      const base = Math.max(BOX / img.width, BOX / img.height);
      const scale = base * zoom * ratio;
      const dw = img.width * scale;
      const dh = img.height * scale;
      const dx = (OUT - dw) / 2 + offset.x * ratio;
      const dy = (OUT - dh) / 2 + offset.y * ratio;
      ctx.fillStyle = '#000';
      ctx.fillRect(0, 0, OUT, OUT);
      ctx.drawImage(img, dx, dy, dw, dh);
      const dataUrl = out.toDataURL('image/jpeg', 0.9);
      const b64 = dataUrl.split(',')[1] ?? '';
      await onConfirm(b64);
      onOpenChange(false);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Choose cover image</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col items-center gap-4">
          {error && <p className="text-sm text-destructive">{error}</p>}
          <div
            className="relative overflow-hidden rounded-lg bg-black touch-none cursor-grab active:cursor-grabbing"
            style={{ width: BOX, height: BOX }}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerLeave={onPointerUp}
          >
            <canvas ref={canvasRef} width={BOX} height={BOX} />
            {!img && !error && (
              <div className="absolute inset-0 grid place-items-center text-sm text-muted-foreground/50">
                Loading…
              </div>
            )}
          </div>
          <div className="w-full max-w-[320px] flex items-center gap-3">
            <span className="text-xs text-muted-foreground/60">Zoom</span>
            <Slider
              value={[zoom]}
              min={1}
              max={4}
              step={0.01}
              onValueChange={(v) => setZoom(v[0] ?? 1)}
            />
          </div>
          <p className="text-xs text-muted-foreground/50">
            Drag to reposition · scroll or slide to zoom
          </p>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={confirm} disabled={!img || busy}>
            {busy ? 'Uploading…' : 'Set cover'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

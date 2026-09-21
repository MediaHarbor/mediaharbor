import type { ReactNode } from 'react';
import { AlertTriangle, CheckCircle2, ClipboardPaste, Plus, RotateCcw, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { cn } from '@/utils/cn';
import type { RadioStreamProbe } from '@/tauri-bridge';

export function Field({
  label,
  overridden,
  onRevert,
  children,
}: {
  label: string;
  overridden: boolean;
  onRevert: () => void;
  children: ReactNode;
}) {
  return (
    <div className="space-y-1">
      <div className="flex items-center gap-2">
        <Label className="text-xs font-medium text-muted-foreground">{label}</Label>
        {overridden && (
          <button
            type="button"
            onClick={onRevert}
            title="Back to the directory's value"
            className="inline-flex items-center gap-1 text-[10px] text-[var(--service-accent)] hover:underline"
          >
            <RotateCcw className="h-2.5 w-2.5" /> changed
          </button>
        )}
      </div>
      {children}
    </div>
  );
}

export function ProbeResult({ probe }: { probe: RadioStreamProbe }) {
  const facts = [
    probe.contentType,
    probe.codec,
    probe.bitrate ? `${probe.bitrate}k` : null,
    probe.icyName,
  ].filter(Boolean);
  return (
    <p
      className={cn(
        'flex items-start gap-1.5 rounded-md border p-2 text-xs',
        probe.ok
          ? 'border-emerald-500/30 bg-emerald-500/5'
          : 'border-destructive/30 bg-destructive/5'
      )}
    >
      {probe.ok ? (
        <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0 text-emerald-500" />
      ) : (
        <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-destructive" />
      )}
      <span className="min-w-0 break-words">
        {probe.ok ? facts.join(' · ') || 'Stream reachable' : probe.message}
      </span>
    </p>
  );
}

export function UrlList({
  urls,
  overridden,
  onRevert,
  onChange,
}: {
  urls: string[];
  overridden: boolean;
  onRevert: () => void;
  onChange: (urls: string[]) => void;
}) {
  return (
    <Field label="Backup streams" overridden={overridden} onRevert={onRevert}>
      <div className="space-y-1.5">
        {urls.map((url, i) => (
          <div key={i} className="flex gap-2">
            <Input
              value={url}
              className="flex-1 font-mono text-xs"
              onChange={(e) => onChange(urls.map((u, j) => (j === i ? e.target.value : u)))}
            />
            <Button
              variant="ghost"
              size="icon"
              aria-label="Remove this backup stream"
              onClick={() => onChange(urls.filter((_, j) => j !== i))}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          </div>
        ))}
        <Button variant="outline" size="sm" onClick={() => onChange([...urls, ''])}>
          <Plus className="mr-1.5 h-3.5 w-3.5" /> Add a mirror
        </Button>
      </div>
    </Field>
  );
}

export function HeaderTable({
  headers,
  onChange,
  onPasteCurl,
}: {
  headers: [string, string][];
  onChange: (headers: [string, string][]) => void;
  onPasteCurl: () => void;
}) {
  return (
    <div className="space-y-2 rounded-lg border border-border/40 p-3">
      <div className="flex items-center justify-between gap-2">
        <div>
          <p className="text-sm font-medium">Request headers</p>
          <p className="text-xs text-muted-foreground/70">
            Some broadcasters check <code>Referer</code> or <code>Origin</code> and refuse without
            them.
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={onPasteCurl}>
          <ClipboardPaste className="mr-1.5 h-3.5 w-3.5" /> Paste cURL
        </Button>
      </div>
      {headers.map(([name, value], i) => (
        <div key={i} className="flex gap-2">
          <Input
            value={name}
            placeholder="Referer"
            className="w-44 font-mono text-xs"
            onChange={(e) =>
              onChange(headers.map((h, j) => (j === i ? [e.target.value, h[1]] : h)))
            }
          />
          <Input
            value={value}
            placeholder="https://example.com/"
            className="flex-1 font-mono text-xs"
            onChange={(e) =>
              onChange(headers.map((h, j) => (j === i ? [h[0], e.target.value] : h)))
            }
          />
          <Button
            variant="ghost"
            size="icon"
            aria-label="Remove this header"
            onClick={() => onChange(headers.filter((_, j) => j !== i))}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      ))}
      <Button variant="outline" size="sm" onClick={() => onChange([...headers, ['', '']])}>
        <Plus className="mr-1.5 h-3.5 w-3.5" /> Add a header
      </Button>
    </div>
  );
}

import { Check, Loader2 } from 'lucide-react';
import { cn } from '@/utils/cn';
import { errorMessage } from '@/utils/errors';
import type { RadioDirectorySource } from '@/features/radio/api';

/**
 * The directory toggles, inline.
 *
 * These write the same `radioDirectorySources` setting the Settings page does —
 * the page is where you configure radio, so making the user leave it to turn a
 * directory off would be the wrong shape.
 *
 * Sources that cannot be switched off — the user's own stations — are shown as
 * plain labels rather than dead buttons.
 */
export function SourceChips({
  sources,
  loading,
  error,
  pending,
  onToggle,
}: {
  sources: RadioDirectorySource[];
  loading: boolean;
  error: unknown;
  pending: boolean;
  onToggle: (id: string, enabled: boolean) => void;
}) {
  if (sources.length === 0) {
    // Distinct on purpose: an empty roster because the backend is unreachable
    // used to sit under a spinner that would never stop.
    if (error) {
      return (
        <span className="text-xs text-destructive/80" title={errorMessage(error)}>
          Sources unavailable
        </span>
      );
    }
    if (loading) {
      return (
        <div className="flex items-center gap-2 text-xs text-muted-foreground/50">
          <Loader2 className="h-3 w-3 animate-spin" /> Loading sources…
        </div>
      );
    }
    return null;
  }
  return (
    <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Station sources">
      {sources.map((source) =>
        source.toggleable ? (
          <button
            key={source.id}
            type="button"
            aria-pressed={source.enabled}
            disabled={pending}
            title={source.status === 'loading' ? 'Building its station list…' : undefined}
            onClick={() => onToggle(source.id, !source.enabled)}
            className={cn(
              'inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] transition-colors',
              'disabled:opacity-60 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
              source.enabled
                ? 'border-[var(--service-accent)] bg-[var(--service-accent)]/10 text-foreground'
                : 'border-border/50 text-muted-foreground/70 hover:border-border hover:text-foreground'
            )}
          >
            {source.status === 'loading' ? (
              <Loader2 className="h-3 w-3 animate-spin" />
            ) : (
              source.enabled && <Check className="h-3 w-3" />
            )}
            {source.label}
          </button>
        ) : (
          <span
            key={source.id}
            className="rounded-full border border-border/30 px-2.5 py-1 text-[11px] text-muted-foreground/60"
            title="Your own stations are always shown"
          >
            {source.label}
          </span>
        )
      )}
    </div>
  );
}

import type { ReactNode } from 'react';
import { AlertTriangle, type LucideIcon } from 'lucide-react';
import { errorMessage } from '@/utils/errors';

/**
 * The upstream message, in full.
 *
 * A canned "something went wrong" hides the one fact worth having — that the
 * mirror answered 502, or that DNS failed — so the real text is shown as it
 * arrived, wrapped rather than truncated.
 */
export function RadioError({ error, what = 'stations' }: { error: unknown; what?: string }) {
  return (
    <div className="mx-auto my-10 max-w-xl rounded-lg border border-destructive/30 bg-destructive/5 p-4">
      <p className="flex items-center gap-2 text-sm font-medium">
        <AlertTriangle className="h-4 w-4 text-destructive" />
        Could not load {what}
      </p>
      <p className="mt-2 whitespace-pre-wrap break-words text-sm text-muted-foreground">
        {errorMessage(error)}
      </p>
    </div>
  );
}

export function RadioEmpty({
  icon: Icon,
  title,
  hint,
  action,
}: {
  icon: LucideIcon;
  title: string;
  hint: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-3 py-20 text-center">
      <Icon className="h-8 w-8 text-muted-foreground/30" />
      <div>
        <p className="text-sm font-medium">{title}</p>
        <p className="mt-1 max-w-sm text-sm text-muted-foreground/70">{hint}</p>
      </div>
      {action}
    </div>
  );
}

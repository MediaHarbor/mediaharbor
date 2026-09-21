import { Grid2x2, List, Minus, Plus } from 'lucide-react';
import { cn } from '@/utils/cn';
import {
  AUTO_COLUMNS,
  MAX_COLUMNS,
  MIN_COLUMNS,
  useRadioStore,
  type RadioView,
} from '@/features/radio/stores/useRadioStore';

/**
 * Grid/list, and how many columns.
 *
 * The toggle matches `LibraryHeader`'s so the two pages read the same. The
 * stepper is radio's own: a directory of 40,000 stations is a different browsing
 * problem from a shelf of albums, and one person's "show me everything" is
 * another's "make them big enough to read".
 */
export function ViewControls({ resolvedColumns }: { resolvedColumns: number }) {
  const view = useRadioStore((s) => s.view);
  const setView = useRadioStore((s) => s.setView);
  const columns = useRadioStore((s) => s.columns);
  const setColumns = useRadioStore((s) => s.setColumns);

  const step = (delta: number) => {
    // Stepping off "auto" starts from what auto is currently showing, so the
    // first press changes the layout by exactly one column.
    const from = columns === AUTO_COLUMNS ? resolvedColumns : columns;
    const next = from + delta;
    setColumns(next < MIN_COLUMNS || next > MAX_COLUMNS ? from : next);
  };

  return (
    <div className="flex shrink-0 items-center gap-2">
      {view === 'grid' && (
        <div className="flex items-center rounded-lg bg-muted/30 p-0.5">
          <StepButton
            label="Fewer columns"
            onClick={() => step(-1)}
            disabled={resolvedColumns <= MIN_COLUMNS}
          >
            <Minus className="h-3 w-3" />
          </StepButton>
          <button
            type="button"
            onClick={() => setColumns(AUTO_COLUMNS)}
            title={columns === AUTO_COLUMNS ? 'Columns fit the window' : 'Back to automatic'}
            className={cn(
              'min-w-8 px-1 text-center text-[11px] tabular-nums transition-colors',
              columns === AUTO_COLUMNS
                ? 'text-muted-foreground/60'
                : 'font-medium text-foreground hover:text-[var(--service-accent)]'
            )}
          >
            {columns === AUTO_COLUMNS ? 'auto' : columns}
          </button>
          <StepButton
            label="More columns"
            onClick={() => step(1)}
            disabled={resolvedColumns >= MAX_COLUMNS}
          >
            <Plus className="h-3 w-3" />
          </StepButton>
        </div>
      )}

      <div className="flex rounded-lg bg-muted/30 p-0.5">
        {(
          [
            ['grid', Grid2x2, 'Grid'],
            ['list', List, 'List'],
          ] as [RadioView, typeof Grid2x2, string][]
        ).map(([value, Icon, label]) => (
          <button
            key={value}
            type="button"
            aria-pressed={view === value}
            title={label}
            onClick={() => setView(value)}
            className={cn(
              'rounded-md p-1.5 transition-all duration-150 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
              view === value
                ? 'bg-background text-foreground shadow-sm'
                : 'text-muted-foreground hover:text-foreground'
            )}
          >
            <Icon className="h-3.5 w-3.5" />
          </button>
        ))}
      </div>
    </div>
  );
}

function StepButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      disabled={disabled}
      className="rounded-md p-1.5 text-muted-foreground transition-colors hover:text-foreground disabled:opacity-30 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
    >
      {children}
    </button>
  );
}

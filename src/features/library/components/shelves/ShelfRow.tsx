import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { cn } from '@/utils/cn';

interface ShelfSectionProps {
  title: string;
  subtitle?: string | null;
  children: ReactNode;
  rowClassName?: string;
  staticBody?: boolean;
  onShowAll?: () => void;
}

const ctrl =
  'grid h-8 w-8 place-items-center rounded-full border border-border/60 bg-card/60 ' +
  'text-foreground/70 transition-colors hover:bg-card hover:text-foreground ' +
  'disabled:opacity-30 disabled:pointer-events-none';

export function ShelfSection({
  title,
  subtitle,
  children,
  rowClassName,
  staticBody = false,
  onShowAll,
}: ShelfSectionProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [atStart, setAtStart] = useState(true);
  const [atEnd, setAtEnd] = useState(true);

  const sync = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    const max = el.scrollWidth - el.clientWidth;
    setAtStart(el.scrollLeft <= 1);
    setAtEnd(el.scrollLeft >= max - 1);
  }, []);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    sync();
    const observer = new ResizeObserver(sync);
    observer.observe(el);
    return () => observer.disconnect();
  }, [sync, children]);

  const page = (dir: 1 | -1) => {
    const el = ref.current;
    if (!el) return;
    el.scrollBy({ left: dir * el.clientWidth * 0.9, behavior: 'smooth' });
  };

  const scrollable = !staticBody && !(atStart && atEnd);

  return (
    <section>
      <div className="mb-3 flex items-end justify-between gap-4">
        <div className="min-w-0">
          {title && (
            <h2 className="truncate text-[22px] font-bold tracking-tight leading-tight">{title}</h2>
          )}
          {subtitle && (
            <p className="mt-0.5 truncate text-[12px] text-muted-foreground/60">{subtitle}</p>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {onShowAll && (
            <button
              type="button"
              onClick={onShowAll}
              className="text-[11px] font-semibold uppercase tracking-wider text-muted-foreground/70 hover:text-foreground hover:underline"
            >
              Show all
            </button>
          )}
          {scrollable && (
            <>
              <button
                type="button"
                aria-label="Scroll left"
                className={ctrl}
                disabled={atStart}
                onClick={() => page(-1)}
              >
                <ChevronLeft className="h-4 w-4" />
              </button>
              <button
                type="button"
                aria-label="Scroll right"
                className={ctrl}
                disabled={atEnd}
                onClick={() => page(1)}
              >
                <ChevronRight className="h-4 w-4" />
              </button>
            </>
          )}
        </div>
      </div>

      {staticBody ? (
        children
      ) : (
        <div
          ref={ref}
          onScroll={sync}
          className={cn('flex overflow-x-auto scrollbar-shelf pb-2 -mx-1 px-1', rowClassName)}
        >
          {children}
        </div>
      )}
    </section>
  );
}

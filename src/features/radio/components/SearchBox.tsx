import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { Search, X } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { cn } from '@/utils/cn';
import { useTagSuggestions } from '@/features/radio/hooks/useRadioSuggest';

/** Long enough for a click on a suggestion to land before the list unmounts. */
const BLUR_GRACE_MS = 120;

/**
 * The search box, in the page header on every tab.
 *
 * It used to be a tab of its own, which meant navigating somewhere in order to
 * type. Suggestions come from the tag corpus cached for the day, so they land
 * with no network in the typing path.
 *
 * The blur handling is the same shape `LibraryHeader` uses: `onMouseDown`
 * preventDefault on a suggestion keeps focus in the input, and the short delay
 * on blur lets the click land before the list unmounts.
 */
export function SearchBox({
  value,
  onChange,
  onPickTag,
  className,
}: {
  value: string;
  onChange: (value: string) => void;
  onPickTag: (tag: string) => void;
  className?: string;
}) {
  const listId = useId();
  const [focused, setFocused] = useState(false);
  const [highlight, setHighlight] = useState(-1);
  const suggestions = useTagSuggestions(value);
  const inputRef = useRef<HTMLInputElement>(null);

  // Held in a ref so it can be cancelled: the whole tab unmounts when the search
  // is cleared, and a timer still pending then would set state on a gone tree.
  const blurTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const closeSoon = useCallback(() => {
    clearTimeout(blurTimer.current);
    blurTimer.current = setTimeout(() => setFocused(false), BLUR_GRACE_MS);
  }, []);
  useEffect(() => () => clearTimeout(blurTimer.current), []);

  const open = focused && value.trim().length >= 2 && suggestions.length > 0;

  useEffect(() => {
    setHighlight(-1);
  }, [value]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      if (open) {
        setHighlight(-1);
        setFocused(false);
      } else {
        onChange('');
        inputRef.current?.blur();
      }
      return;
    }
    if (!open) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setHighlight((i) => (i + 1) % suggestions.length);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlight((i) => (i <= 0 ? suggestions.length - 1 : i - 1));
    } else if (e.key === 'Enter' && highlight >= 0) {
      e.preventDefault();
      onPickTag(suggestions[highlight].name);
      setHighlight(-1);
    }
  };

  return (
    <div className={cn('relative', className)}>
      <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground/40" />
      <Input
        ref={inputRef}
        role="combobox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        aria-activedescendant={open && highlight >= 0 ? `${listId}-${highlight}` : undefined}
        aria-autocomplete="list"
        aria-label="Search stations and genres"
        className="h-9 rounded-lg border-0 bg-muted/30 pl-9 pr-8 text-sm placeholder:text-muted-foreground/40"
        placeholder="Search stations and genres…"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onFocus={() => setFocused(true)}
        onBlur={closeSoon}
        onKeyDown={onKeyDown}
      />
      {value && (
        <button
          type="button"
          onClick={() => onChange('')}
          aria-label="Clear search"
          className="absolute right-2 top-1/2 -translate-y-1/2 rounded-full p-1 text-muted-foreground/50 transition-colors hover:text-foreground"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      )}
      {open && (
        <ul
          id={listId}
          role="listbox"
          className="absolute z-50 mt-1 w-full overflow-hidden rounded-lg border border-border/60 bg-popover shadow-lg"
        >
          {suggestions.map((facet, index) => (
            <li
              key={facet.name}
              id={`${listId}-${index}`}
              role="option"
              aria-selected={index === highlight}
            >
              <button
                type="button"
                tabIndex={-1}
                className={cn(
                  'flex w-full items-center gap-2 px-3 py-2 text-left text-sm transition-colors',
                  index === highlight ? 'bg-accent/60' : 'hover:bg-accent/40'
                )}
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => setHighlight(index)}
                onClick={() => onPickTag(facet.name)}
              >
                <Search className="h-3.5 w-3.5 shrink-0 text-muted-foreground/40" />
                <span className="truncate">{facet.label}</span>
                <span className="ml-auto shrink-0 text-[10px] text-muted-foreground/40">
                  {facet.stationCount.toLocaleString()}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

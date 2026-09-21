import type { ReactNode } from 'react';
import { Compass, Heart, House, ListMusic } from 'lucide-react';
import { cn } from '@/utils/cn';
import type { RadioTab } from '@/features/radio/stores/useRadioStore';

const RADIO_TABS: RadioTab[] = ['home', 'browse', 'favorites', 'playlists'];

const ICONS: Record<RadioTab, ReactNode> = {
  home: <House className="h-3.5 w-3.5" />,
  browse: <Compass className="h-3.5 w-3.5" />,
  favorites: <Heart className="h-3.5 w-3.5" />,
  playlists: <ListMusic className="h-3.5 w-3.5" />,
};

const LABELS: Record<RadioTab, string> = {
  home: 'Home',
  browse: 'Browse',
  favorites: 'Favourites',
  playlists: 'Playlists',
};

/**
 * Hand-rolled to match `LibraryTabs`, not the Radix `Tabs` primitive: the
 * underline and the `--service-accent` colouring are what make the two pages
 * read as the same app.
 */
export function RadioTabs({
  value,
  onChange,
}: {
  value: RadioTab;
  onChange: (tab: RadioTab) => void;
}) {
  return (
    <div className="border-b border-border/30 -mx-1 px-1 overflow-x-auto scrollbar-none">
      <div role="tablist" className="flex items-end gap-1">
        {RADIO_TABS.map((tab) => {
          const active = value === tab;
          return (
            <button
              key={tab}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => onChange(tab)}
              className={cn(
                'group relative inline-flex items-center gap-2 px-3 py-2.5 text-[12px] font-semibold tracking-wide uppercase whitespace-nowrap transition-colors',
                active ? 'text-foreground' : 'text-muted-foreground/60 hover:text-foreground'
              )}
              style={active ? { color: 'var(--service-accent)' } : undefined}
            >
              {ICONS[tab]}
              <span>{LABELS[tab]}</span>
              <span
                aria-hidden
                className={cn(
                  'absolute left-2 right-2 -bottom-px h-[2px] rounded-t-full transition-opacity',
                  active ? 'opacity-100' : 'opacity-0'
                )}
                style={{ backgroundColor: 'var(--service-accent)' }}
              />
            </button>
          );
        })}
      </div>
    </div>
  );
}

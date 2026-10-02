import type { ReactNode } from 'react';
import {
  Home,
  ListMusic,
  Disc3,
  User,
  ListChecks,
  Film,
  Users,
  Activity,
  Bookmark,
} from 'lucide-react';
import { LIBRARY_TABS, SERVICE_LIBRARY_TABS, type LibraryTab } from '@/stores/useLibraryStore';
import { cn } from '@/utils/cn';

const ICONS: Record<LibraryTab, ReactNode> = {
  home: <Home className="h-3.5 w-3.5" />,
  tracks: <ListMusic className="h-3.5 w-3.5" />,
  albums: <Disc3 className="h-3.5 w-3.5" />,
  artists: <User className="h-3.5 w-3.5" />,
  playlists: <ListChecks className="h-3.5 w-3.5" />,
  videos: <Film className="h-3.5 w-3.5" />,
  following: <Users className="h-3.5 w-3.5" />,
  activity: <Activity className="h-3.5 w-3.5" />,
  bookmarks: <Bookmark className="h-3.5 w-3.5" />,
};

const LABELS: Record<LibraryTab, string> = {
  home: 'Home',
  tracks: 'Tracks',
  albums: 'Albums',
  artists: 'Artists',
  playlists: 'Playlists',
  videos: 'Videos',
  following: 'Following',
  activity: 'Activity',
  bookmarks: 'Bookmarks',
};

interface LibraryTabsProps {
  value: LibraryTab;
  onChange: (t: LibraryTab) => void;
  serviceMode?: boolean;
  tabs?: LibraryTab[];
}

export function LibraryTabs({
  value,
  onChange,
  serviceMode = false,
  tabs: override,
}: LibraryTabsProps) {
  const tabs = override ?? (serviceMode ? SERVICE_LIBRARY_TABS : LIBRARY_TABS);
  return (
    <div className="border-b border-border/30 -mx-1 px-1 overflow-x-auto scrollbar-none">
      <div role="tablist" className="flex items-end gap-1">
        {tabs.map((t) => {
          const active = value === t;
          return (
            <button
              key={t}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => onChange(t)}
              className={cn(
                'group relative inline-flex items-center gap-2 px-3 py-2.5 text-[12px] font-semibold tracking-wide uppercase whitespace-nowrap transition-colors',
                active ? 'text-foreground' : 'text-muted-foreground/60 hover:text-foreground'
              )}
              style={active ? { color: 'var(--service-accent)' } : undefined}
            >
              {ICONS[t]}
              <span>{LABELS[t]}</span>
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

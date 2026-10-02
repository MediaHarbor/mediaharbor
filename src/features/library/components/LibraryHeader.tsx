import { useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Search, ScanLine, RefreshCw, Grid2x2, List, Library } from 'lucide-react';
import { PlatformIcon } from '@/utils/platforms';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { LibraryTabs } from '@/features/library/components/LibraryTabs';
import { librarySourcesFor, toClientPlatform } from '@/utils/platform-data';
import { labelFor } from '@/features/library/serviceAccent';
import { useMusicSuggestions } from '@/hooks/useMusicSuggestions';
import { useSearchStore } from '@/features/search/stores/searchStore';
import { useEnabledServices } from '@/hooks/useAppSettings';
import type { LibraryTab } from '@/stores/useLibraryStore';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

export type SortType = string;
export type ViewType = 'grid' | 'list';

interface LibraryHeaderProps {
  search: string;
  onSearchChange: (value: string) => void;
  sort: SortType;
  onSortChange: (value: SortType) => void;
  sortOptions: { value: string; label: string }[];
  view: ViewType;
  onViewChange: (value: ViewType) => void;
  showViewToggle: boolean;
  showSort: boolean;
  albumCount: number;
  trackCount: number;
  videoCount: number;
  isScanning: boolean;
  isRefreshing: boolean;
  canScan: boolean;
  onRescan: () => void;
  activeSource: string;
  onSourceChange: (source: string) => void;
  tab: LibraryTab;
  onTabChange: (t: LibraryTab) => void;
  serviceMode: boolean;
  tabs?: LibraryTab[];
}

export function LibraryHeader({
  search,
  onSearchChange,
  sort,
  onSortChange,
  sortOptions,
  view,
  onViewChange,
  showViewToggle,
  showSort,
  albumCount,
  trackCount,
  videoCount,
  isScanning,
  isRefreshing,
  canScan,
  onRescan,
  activeSource,
  onSourceChange,
  tab,
  onTabChange,
  serviceMode,
  tabs,
}: LibraryHeaderProps) {
  const navigate = useNavigate();
  const setPendingQuery = useSearchStore((s) => s.setPendingQuery);
  const setSelectedPlatform = useSearchStore((s) => s.setSelectedPlatform);
  const setSearchType = useSearchStore((s) => s.setSearchType);
  const [focused, setFocused] = useState(false);
  const enabledServices = useEnabledServices();

  const librarySources = useMemo(() => librarySourcesFor(enabledServices ?? []), [enabledServices]);

  const searchPlatform = toClientPlatform(activeSource);
  const suggestions = useMusicSuggestions(
    serviceMode && focused ? search : '',
    serviceMode ? searchPlatform : undefined
  );
  const showSuggestions = serviceMode && focused && suggestions.length > 0;

  const jumpToSearch = (q: string) => {
    const term = q.trim();
    if (!term) return;
    setSelectedPlatform(searchPlatform);
    setSearchType('track');
    setPendingQuery(term);
    setFocused(false);
    navigate('/search');
  };

  const statsText = [
    albumCount > 0 && `${albumCount} album${albumCount !== 1 ? 's' : ''}`,
    trackCount > 0 && `${trackCount} track${trackCount !== 1 ? 's' : ''}`,
    videoCount > 0 && `${videoCount} video${videoCount !== 1 ? 's' : ''}`,
  ]
    .filter(Boolean)
    .join(', ');
  const placeholder = serviceMode
    ? `Search ${labelFor(activeSource as Parameters<typeof labelFor>[0])}…`
    : statsText
      ? `Search ${statsText}…`
      : 'Search your library…';

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-4">
        <h1 className="text-2xl font-bold tracking-tight shrink-0">Library</h1>

        <div className="flex-1 relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground/40 pointer-events-none" />
          <Input
            className="pl-9 h-9 bg-muted/30 border-0 rounded-lg text-sm placeholder:text-muted-foreground/40"
            placeholder={placeholder}
            value={search}
            onChange={(e) => onSearchChange(e.target.value)}
            onFocus={() => setFocused(true)}
            onBlur={() => setTimeout(() => setFocused(false), 120)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && serviceMode) {
                e.preventDefault();
                jumpToSearch(search);
              }
            }}
          />
          {showSuggestions && (
            <div className="absolute z-50 mt-1 w-full rounded-lg border border-border/60 bg-popover shadow-lg overflow-hidden">
              {suggestions.map((s) => (
                <button
                  key={`${s.type}:${s.text}`}
                  type="button"
                  className="w-full flex items-center gap-2 px-3 py-2 text-left text-sm hover:bg-accent/40 transition-colors"
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => jumpToSearch(s.text)}
                >
                  <Search className="h-3.5 w-3.5 text-muted-foreground/40 shrink-0" />
                  <span className="truncate">{s.text}</span>
                </button>
              ))}
            </div>
          )}
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {isRefreshing && !isScanning && (
            <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground/50">
              <RefreshCw className="h-3 w-3 animate-spin" />
              Syncing
            </span>
          )}
          <Button
            variant="outline"
            size="sm"
            disabled={isScanning || !canScan}
            onClick={onRescan}
            className="gap-2 rounded-lg h-9 border-border/40 text-xs"
          >
            {isScanning ? (
              <RefreshCw className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <ScanLine className="h-3.5 w-3.5" />
            )}
            {isScanning ? 'Scanning…' : 'Rescan'}
          </Button>
        </div>
      </div>

      <div className="flex items-center gap-3">
        <Select value={activeSource} onValueChange={onSourceChange}>
          <SelectTrigger className="w-[140px] h-7 rounded-md bg-muted/30 border-0 text-[11px] shrink-0">
            <Library className="h-3 w-3 shrink-0 mr-1.5" />
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="local">Local Library</SelectItem>
            {librarySources.map((s) => (
              <SelectItem key={s.value} value={s.value}>
                <span className="inline-flex items-center gap-2">
                  <PlatformIcon platform={toClientPlatform(s.value)} size={12} />{' '}
                  {labelFor(s.value)}
                  {s.wip && (
                    <span className="text-[8px] font-bold bg-amber-500/10 text-amber-500/70 px-1 py-0.5 rounded leading-none">
                      WIP
                    </span>
                  )}
                </span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <div className="flex-1 min-w-0">
          <LibraryTabs value={tab} onChange={onTabChange} serviceMode={serviceMode} tabs={tabs} />
        </div>

        {showSort && sortOptions.length > 0 && (
          <Select value={sort} onValueChange={(v) => onSortChange(v as SortType)}>
            <SelectTrigger className="w-[130px] h-8 rounded-lg bg-muted/30 border-0 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {sortOptions.map((o) => (
                <SelectItem key={o.value} value={o.value}>
                  {o.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}

        {showViewToggle && (
          <div className="flex rounded-lg bg-muted/30 p-0.5">
            <button
              onClick={() => onViewChange('grid')}
              className={`p-1.5 rounded-md transition-all duration-150 ${
                view === 'grid'
                  ? 'bg-background text-foreground shadow-sm'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
            >
              <Grid2x2 className="h-3.5 w-3.5" />
            </button>
            <button
              onClick={() => onViewChange('list')}
              className={`p-1.5 rounded-md transition-all duration-150 ${
                view === 'list'
                  ? 'bg-background text-foreground shadow-sm'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
            >
              <List className="h-3.5 w-3.5" />
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

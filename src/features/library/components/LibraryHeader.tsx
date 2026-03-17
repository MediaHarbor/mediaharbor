import { Search, ScanLine, RefreshCw, Grid2x2, List, Music2, FileVideo, Library } from 'lucide-react';
import { PlatformIcon } from '@/utils/platforms';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

export type FilterType = 'music' | 'video';
export type SortType = 'name' | 'date' | 'size' | 'artist';
export type ViewType = 'grid' | 'list';

interface LibraryHeaderProps {
  search: string;
  onSearchChange: (value: string) => void;
  filter: FilterType;
  onFilterChange: (value: FilterType) => void;
  sort: SortType;
  onSortChange: (value: SortType) => void;
  view: ViewType;
  onViewChange: (value: ViewType) => void;
  albumCount: number;
  trackCount: number;
  videoCount: number;
  isScanning: boolean;
  isRefreshing: boolean;
  canScan: boolean;
  onRescan: () => void;
  activeSource: string;
  onSourceChange: (source: string) => void;
}

export function LibraryHeader({
  search,
  onSearchChange,
  filter,
  onFilterChange,
  sort,
  onSortChange,
  view,
  onViewChange,
  albumCount,
  trackCount,
  videoCount,
  isScanning,
  isRefreshing,
  canScan,
  onRescan,
  activeSource,
  onSourceChange,
}: LibraryHeaderProps) {
  const filters: { value: FilterType; label: string; icon: typeof Music2 }[] = [
    { value: 'music', label: 'Music', icon: Music2 },
    { value: 'video', label: 'Videos', icon: FileVideo },
  ];

  const statsText = [
    albumCount > 0 && `${albumCount} album${albumCount !== 1 ? 's' : ''}`,
    trackCount > 0 && `${trackCount} track${trackCount !== 1 ? 's' : ''}`,
    videoCount > 0 && `${videoCount} video${videoCount !== 1 ? 's' : ''}`,
  ].filter(Boolean).join(', ');
  const placeholder = statsText ? `Search ${statsText}…` : 'Search your library…';

  return (
    <div className="space-y-3">
      {/* Row 1: Title + search bar + scan */}
      <div className="flex items-center gap-4">
        <h1 className="text-2xl font-bold tracking-tight shrink-0">Library</h1>

        <div className="flex-1 relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground/40 pointer-events-none" />
          <Input
            className="pl-9 h-9 bg-muted/30 border-0 rounded-lg text-sm placeholder:text-muted-foreground/40"
            placeholder={placeholder}
            value={search}
            onChange={(e) => onSearchChange(e.target.value)}
          />
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

      {/* Row 2: Source dropdown + filters + sort + view */}
      <div className="flex items-center gap-3">
        <Select value={activeSource} onValueChange={onSourceChange}>
          <SelectTrigger className="w-[150px] h-8 rounded-lg bg-muted/30 border-0 text-xs">
            <Library className="h-3.5 w-3.5 shrink-0 mr-1.5" />
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="local">Local Library</SelectItem>
            <SelectItem value="spotify">
              <span className="inline-flex items-center gap-2">
                <PlatformIcon platform="spotify" size={12} />
                Spotify
                <span className="text-[8px] font-bold bg-amber-500/10 text-amber-500/70 px-1 py-0.5 rounded leading-none">WIP</span>
              </span>
            </SelectItem>
            <SelectItem value="deezer">
              <span className="inline-flex items-center gap-2">
                <PlatformIcon platform="deezer" size={12} />
                Deezer
                <span className="text-[8px] font-bold bg-amber-500/10 text-amber-500/70 px-1 py-0.5 rounded leading-none">WIP</span>
              </span>
            </SelectItem>
            <SelectItem value="tidal">
              <span className="inline-flex items-center gap-2">
                <PlatformIcon platform="tidal" size={12} />
                Tidal
                <span className="text-[8px] font-bold bg-amber-500/10 text-amber-500/70 px-1 py-0.5 rounded leading-none">WIP</span>
              </span>
            </SelectItem>
          </SelectContent>
        </Select>

        <div className="h-4 w-px bg-border/30" />

        <div className="flex items-center gap-1 bg-muted/30 rounded-lg p-0.5">
          {filters.map((f) => (
            <button
              key={f.value}
              onClick={() => onFilterChange(f.value)}
              className={`inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-all duration-150 ${
                filter === f.value
                  ? 'bg-background text-foreground shadow-sm'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
            >
              <f.icon className="h-3.5 w-3.5" />
              {f.label}
            </button>
          ))}
        </div>

        <div className="flex-1" />

        <Select value={sort} onValueChange={(v) => onSortChange(v as SortType)}>
          <SelectTrigger className="w-[110px] h-8 rounded-lg bg-muted/30 border-0 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="name">Name</SelectItem>
            <SelectItem value="artist">Artist</SelectItem>
            <SelectItem value="date">Date Added</SelectItem>
            <SelectItem value="size">Size</SelectItem>
          </SelectContent>
        </Select>

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
      </div>
    </div>
  );
}

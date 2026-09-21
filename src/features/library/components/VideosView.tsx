import { useRef, type ReactNode } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { FileVideo, Play, Copy } from 'lucide-react';
import {
  ipcPlatformOf,
  type TrackDto,
  type ServicePlatform,
  toPlayable,
} from '@/features/library/api';
import { useInfiniteRows, useVisibleGridCovers } from '@/features/library/hooks/useVirtualRows';
import { useLibraryInfiniteQuery } from '@/features/library/hooks/useLibraryInfiniteQuery';
import { useElementWidth } from '@/features/library/hooks/useElementWidth';
import { renderLibraryQueryState } from '@/features/library/components/LibraryQueryState';
import { formatDurationShort as fmt } from '@/utils/formatters';
import { usePlayerStore } from '@/stores/usePlayerStore';
import { copyText } from '@/utils/clipboard';
import {
  ContextMenu,
  ContextMenuTrigger,
  ContextMenuContent,
  ContextMenuItem,
} from '@/components/ui/context-menu';

function VideoMenu({
  onPlay,
  link,
  children,
}: {
  onPlay: () => void;
  link?: string | null;
  children: ReactNode;
}) {
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={onPlay}>
          <Play className="h-4 w-4 mr-2" /> Play
        </ContextMenuItem>
        {link && (
          <ContextMenuItem
            onSelect={() => {
              void copyText(link);
            }}
          >
            <Copy className="h-4 w-4 mr-2" /> Copy link
          </ContextMenuItem>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}

interface VideosViewProps {
  view: 'grid' | 'list';
  search: string;
  sort: string;
  source?: ServicePlatform;
}

function columnsFor(width: number): number {
  if (width >= 1024) return 5;
  if (width >= 768) return 4;
  if (width >= 640) return 3;
  return 2;
}

export function VideosView({ view, search, sort, source = 'local' }: VideosViewProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const width = useElementWidth(parentRef);
  const setQueue = usePlayerStore((s) => s.setQueue);
  const { query, items } = useLibraryInfiniteQuery<TrackDto>('videos', {
    sort,
    search,
    source,
  });

  const cols = view === 'list' ? 1 : columnsFor(width);
  const rowCount = view === 'list' ? items.length : Math.ceil(items.length / cols);
  const rowHeight = view === 'list' ? 64 : Math.round((width / cols) * 0.5625 + 56);

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => parentRef.current,
    estimateSize: () => rowHeight,
    overscan: 4,
  });

  const virtualItems = virtualizer.getVirtualItems();

  useInfiniteRows(query, virtualItems, rowCount);

  const coverUrls = useVisibleGridCovers(virtualItems, items, cols);

  const play = (index: number) => {
    const t = items[index];
    if (!t) return;
    const platform = source === 'local' ? 'local' : ipcPlatformOf(source);
    const playable = toPlayable(t, platform, coverUrls, { mediaType: 'video' });
    setQueue([playable], 0);
  };

  const stateView = renderLibraryQueryState({
    query,
    entity: 'videos',
    source,
    count: items.length,
  });
  if (stateView) return stateView;

  return (
    <div ref={parentRef} className="h-full overflow-y-auto">
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
        {virtualizer.getVirtualItems().map((vi) => {
          if (view === 'list') {
            const item = items[vi.index];
            const url = item?.cover_id ? (coverUrls[item.cover_id] ?? null) : null;
            if (!item) return null;
            return (
              <VideoMenu key={`video-${item.path}`} onPlay={() => play(vi.index)} link={item.path}>
                <div
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    right: 0,
                    height: vi.size,
                    transform: `translateY(${vi.start}px)`,
                  }}
                  className="group flex items-center gap-3 px-3 hover:bg-card/50 cursor-pointer"
                  onDoubleClick={() => play(vi.index)}
                >
                  {url ? (
                    <img
                      src={url}
                      alt=""
                      loading="lazy"
                      className="h-10 w-[72px] rounded object-cover shrink-0"
                    />
                  ) : (
                    <div className="h-10 w-[72px] rounded bg-muted flex items-center justify-center shrink-0">
                      <FileVideo className="h-4 w-4 text-muted-foreground/30" />
                    </div>
                  )}
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium truncate">{item.title ?? ''}</p>
                    <p className="text-[11px] text-muted-foreground/40">
                      {fmt(item.duration_secs)}
                    </p>
                  </div>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      play(vi.index);
                    }}
                    className="h-8 w-8 rounded-full flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity hover:bg-primary/10"
                  >
                    <Play className="h-3.5 w-3.5 ml-0.5" />
                  </button>
                </div>
              </VideoMenu>
            );
          }
          const start = vi.index * cols;
          const slice = items.slice(start, start + cols);
          return (
            <div
              key={vi.index}
              className="grid gap-1 px-px"
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                right: 0,
                height: vi.size,
                transform: `translateY(${vi.start}px)`,
                gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`,
              }}
            >
              {slice.map((item, j) => {
                const url = item.cover_id ? (coverUrls[item.cover_id] ?? null) : null;
                return (
                  <VideoMenu
                    key={`video-${item.path}-${start + j}`}
                    onPlay={() => play(start + j)}
                    link={item.path}
                  >
                    <div
                      className="group p-2 rounded-md cursor-pointer hover:bg-card/50"
                      onClick={() => play(start + j)}
                    >
                      <div className="relative aspect-video rounded-md overflow-hidden mb-2 bg-muted">
                        {url ? (
                          <img
                            src={url}
                            alt=""
                            loading="lazy"
                            className="w-full h-full object-cover"
                          />
                        ) : (
                          <div className="w-full h-full flex items-center justify-center">
                            <FileVideo className="h-8 w-8 text-muted-foreground/30" />
                          </div>
                        )}
                      </div>
                      <p className="text-sm font-medium truncate">{item.title ?? ''}</p>
                      <p className="text-[11px] text-muted-foreground/40">
                        {fmt(item.duration_secs)}
                      </p>
                    </div>
                  </VideoMenu>
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
}

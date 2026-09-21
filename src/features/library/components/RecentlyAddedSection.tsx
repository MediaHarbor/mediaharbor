import { useRef, useState, type ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ChevronLeft, ChevronRight, Play, Disc3, FileVideo } from 'lucide-react';
import { motion } from 'framer-motion';
import { Button } from '@/components/ui/button';
import { queryLibrary, libraryKeys, type AlbumDto, type TrackDto } from '@/features/library/api';
import { useCoverUrls } from '@/features/library/useCoverUrls';
import {
  ContextMenu,
  ContextMenuTrigger,
  ContextMenuContent,
  ContextMenuItem,
} from '@/components/ui/context-menu';
import { AlbumContextMenu } from '@/features/library/actions/RowContextMenu';

interface RecentlyAddedSectionProps {
  kind: 'albums' | 'videos';
  onSelectAlbum?: (album_key: string) => void;
  onPlayAlbum?: (album_key: string) => void;
  onPlayVideo?: (track: TrackDto) => void;
}

export function RecentlyAddedSection({
  kind,
  onSelectAlbum,
  onPlayAlbum,
  onPlayVideo,
}: RecentlyAddedSectionProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(true);

  const query = useQuery<{ items: Array<AlbumDto | TrackDto>; total: number | null }>({
    queryKey: libraryKeys.recent(kind),
    queryFn: async () => {
      if (kind === 'albums') {
        return queryLibrary<AlbumDto>({ kind: 'albums', limit: 15, sort: 'recent' });
      }
      return queryLibrary<TrackDto>({ kind: 'videos', limit: 15 });
    },
  });

  const items: Array<AlbumDto | TrackDto> = query.data?.items ?? [];
  const coverIds = items.map((i) => (i as { cover_id?: string | null }).cover_id);
  const coverUrls = useCoverUrls(coverIds);

  if (items.length === 0) return null;

  const checkScroll = () => {
    if (!scrollRef.current) return;
    const { scrollLeft, scrollWidth, clientWidth } = scrollRef.current;
    setCanScrollLeft(scrollLeft > 10);
    setCanScrollRight(scrollLeft < scrollWidth - clientWidth - 10);
  };

  const scroll = (direction: 'left' | 'right') => {
    if (!scrollRef.current) return;
    const amount = direction === 'left' ? -300 : 300;
    scrollRef.current.scrollBy({ left: amount, behavior: 'smooth' });
    setTimeout(checkScroll, 350);
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-semibold tracking-tight">Recently Added</h3>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 rounded-full"
            disabled={!canScrollLeft}
            onClick={() => scroll('left')}
          >
            <ChevronLeft className="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 rounded-full"
            disabled={!canScrollRight}
            onClick={() => scroll('right')}
          >
            <ChevronRight className="h-4 w-4" />
          </Button>
        </div>
      </div>

      <div
        ref={scrollRef}
        onScroll={checkScroll}
        className="flex gap-3 overflow-x-auto scrollbar-none pb-1 -mx-1 px-1"
      >
        {kind === 'albums'
          ? (items as AlbumDto[]).map((a, i) => {
              const url = a.cover_id ? (coverUrls[a.cover_id] ?? null) : null;
              return (
                <AlbumContextMenu
                  key={a.album_key}
                  platform="local"
                  context="library"
                  album={{ id: a.album_key, title: a.title, artist: a.artist, cover_url: url }}
                  onPlay={() => onPlayAlbum?.(a.album_key)}
                >
                  <div className="shrink-0">
                    <Card
                      index={i}
                      aspect="square"
                      thumb={url}
                      title={a.title}
                      subtitle={a.artist}
                      onClick={() => onSelectAlbum?.(a.album_key)}
                      onPlay={() => onPlayAlbum?.(a.album_key)}
                    />
                  </div>
                </AlbumContextMenu>
              );
            })
          : (items as TrackDto[]).map((t, i) => {
              const url = t.cover_id ? (coverUrls[t.cover_id] ?? null) : null;
              return (
                <CardMenu
                  key={t.path}
                  onOpen={() => onPlayVideo?.(t)}
                  onPlay={() => onPlayVideo?.(t)}
                >
                  <Card
                    index={i}
                    aspect="video"
                    thumb={url}
                    title={t.title ?? ''}
                    subtitle={''}
                    onClick={() => onPlayVideo?.(t)}
                    onPlay={() => onPlayVideo?.(t)}
                  />
                </CardMenu>
              );
            })}
      </div>
    </div>
  );
}

function Card({
  index,
  aspect,
  thumb,
  title,
  subtitle,
  onClick,
  onPlay,
}: {
  index: number;
  aspect: 'square' | 'video';
  thumb: string | null;
  title: string;
  subtitle: string;
  onClick: () => void;
  onPlay: () => void;
}) {
  return (
    <motion.div
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.25, delay: Math.min(index * 0.05, 0.5) }}
      className="group shrink-0 w-[140px] cursor-pointer"
      onClick={onClick}
    >
      <div
        className={`relative ${aspect === 'square' ? 'aspect-square' : 'aspect-video'} rounded-xl overflow-hidden bg-muted mb-2`}
      >
        {thumb ? (
          <img
            src={thumb}
            alt={title}
            loading="lazy"
            className="w-full h-full object-cover transition-transform duration-300 group-hover:scale-105"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center">
            {aspect === 'square' ? (
              <Disc3 className="h-8 w-8 text-muted-foreground/30" />
            ) : (
              <FileVideo className="h-8 w-8 text-muted-foreground/30" />
            )}
          </div>
        )}
        <div
          className="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 transition-opacity duration-200 flex items-center justify-center"
          onClick={(e) => {
            e.stopPropagation();
            onPlay();
          }}
        >
          <div className="h-10 w-10 rounded-full bg-primary flex items-center justify-center shadow-lg">
            <Play className="h-4 w-4 text-primary-foreground ml-0.5" />
          </div>
        </div>
      </div>
      <p className="text-xs font-medium truncate leading-tight">{title || 'Unknown'}</p>
      <p className="text-[10px] text-muted-foreground/70 truncate mt-0.5">{subtitle}</p>
    </motion.div>
  );
}

function CardMenu({
  onOpen,
  onPlay,
  children,
}: {
  onOpen: () => void;
  onPlay: () => void;
  children: ReactNode;
}) {
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div className="shrink-0">{children}</div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem onSelect={onPlay}>
          <Play className="h-4 w-4 mr-2" /> Play
        </ContextMenuItem>
        <ContextMenuItem onSelect={onOpen}>Open</ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

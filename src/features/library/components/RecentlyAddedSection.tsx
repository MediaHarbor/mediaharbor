import { useRef, useState } from 'react';
import { ChevronLeft, ChevronRight, Play, Disc3, FileVideo } from 'lucide-react';
import { motion } from 'framer-motion';
import { Button } from '@/components/ui/button';
import type { AlbumItem, MediaItem, VideoItem } from '@/stores/useLibraryStore';

function thumbnailSrc(thumb?: { data: string; format: string }): string | null {
  if (!thumb) return null;
  return `data:${thumb.format};base64,${thumb.data}`;
}

interface RecentlyAddedSectionProps {
  items: MediaItem[];
  onPlay: (item: AlbumItem | VideoItem, trackIndex?: number) => void;
  onSelect?: (item: AlbumItem) => void;
}

export function RecentlyAddedSection({ items, onPlay, onSelect }: RecentlyAddedSectionProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(true);

  if (items.length === 0) return null;

  const recent = items.slice(0, 15);

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
        {recent.map((item, i) => (
          <RecentCard key={i} item={item} index={i} onPlay={onPlay} onSelect={onSelect} />
        ))}
      </div>
    </div>
  );
}

function RecentCard({
  item,
  index,
  onPlay,
  onSelect,
}: {
  item: MediaItem;
  index: number;
  onPlay: (item: AlbumItem | VideoItem, trackIndex?: number) => void;
  onSelect?: (item: AlbumItem) => void;
}) {
  const isAlbum = item.type === 'music';
  const album = isAlbum ? (item as AlbumItem) : null;
  const video = !isAlbum ? (item as VideoItem) : null;
  const thumb = thumbnailSrc(isAlbum ? album!.thumbnail : video!.thumbnail);

  const handleClick = () => {
    if (isAlbum && onSelect) {
      onSelect(item as AlbumItem);
    } else {
      onPlay(item as AlbumItem | VideoItem);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.25, delay: Math.min(index * 0.05, 0.5) }}
      className="group shrink-0 w-[140px] cursor-pointer"
      onClick={handleClick}
    >
      <div className={`relative ${isAlbum ? 'aspect-square' : 'aspect-video'} rounded-xl overflow-hidden bg-muted mb-2`}>
        {thumb ? (
          <img
            src={thumb}
            alt={album?.album || video?.title || ''}
            className="w-full h-full object-cover transition-transform duration-300 group-hover:scale-105"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center">
            {isAlbum ? (
              <Disc3 className="h-8 w-8 text-muted-foreground/30" />
            ) : (
              <FileVideo className="h-8 w-8 text-muted-foreground/30" />
            )}
          </div>
        )}
        <div className="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 transition-opacity duration-200 flex items-center justify-center">
          <div className="h-10 w-10 rounded-full bg-primary flex items-center justify-center shadow-lg">
            <Play className="h-4 w-4 text-primary-foreground ml-0.5" />
          </div>
        </div>
      </div>
      <p className="text-xs font-medium truncate leading-tight">
        {album?.album || video?.title || 'Unknown'}
      </p>
      <p className="text-[10px] text-muted-foreground/70 truncate mt-0.5">
        {album?.artist || video?.size || ''}
      </p>
    </motion.div>
  );
}

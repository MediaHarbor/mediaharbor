import { Play, FolderOpen, FileVideo } from 'lucide-react';
import { memo, useMemo } from 'react';
import { motion } from 'framer-motion';
import { Button } from '@/components/ui/button';
import type { VideoItem } from '@/stores/useLibraryStore';

function thumbnailSrc(thumb?: { data: string; format: string }): string | null {
  if (!thumb) return null;
  return `data:${thumb.format};base64,${thumb.data}`;
}

function formatDuration(seconds?: number): string {
  if (!seconds) return '';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
  return `${m}:${s.toString().padStart(2, '0')}`;
}

interface VideoCardProps {
  item: VideoItem;
  view: 'grid' | 'list';
  onPlay: (item: VideoItem) => void;
  onOpen: (path: string) => void;
  index?: number;
}

export const VideoCard = memo(function VideoCard({ item, view, onPlay, onOpen, index = 0 }: VideoCardProps) {
  const thumb = useMemo(() => thumbnailSrc(item.thumbnail), [item.thumbnail]);

  if (view === 'list') {
    return (
      <motion.div
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.2, delay: Math.min(index * 0.02, 0.2) }}
        className="flex items-center gap-3 rounded-lg bg-card/50 px-3 py-2.5 hover:bg-card/80 transition-colors duration-200 cursor-pointer"
        onClick={() => onPlay(item)}
      >
        <div className="relative shrink-0 group/art">
          {thumb ? (
            <img src={thumb} alt={item.title} loading="lazy" className="h-10 w-[72px] rounded object-cover" />
          ) : (
            <div className="h-10 w-[72px] rounded bg-muted flex items-center justify-center">
              <FileVideo className="h-4 w-4 text-muted-foreground/40" />
            </div>
          )}
          <div className="absolute inset-0 rounded bg-black/50 flex items-center justify-center opacity-0 group-hover/art:opacity-100 transition-opacity">
            <Play className="h-3.5 w-3.5 text-white ml-0.5" />
          </div>
        </div>
        <div className="flex-1 min-w-0">
          <p className="font-medium text-[13px] truncate leading-tight">{item.title}</p>
          <p className="text-[11px] text-muted-foreground/50 mt-0.5">{item.size}</p>
        </div>
        {item.duration && (
          <span className="text-[11px] text-muted-foreground/40 tabular-nums shrink-0">
            {formatDuration(item.duration)}
          </span>
        )}
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 rounded-full opacity-0 group-hover:opacity-100 text-muted-foreground/60 shrink-0"
          onClick={(e) => { e.stopPropagation(); onOpen(item.path); }}
        >
          <FolderOpen className="h-3.5 w-3.5" />
        </Button>
      </motion.div>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3, delay: Math.min(index * 0.03, 0.3) }}
      className="group relative p-3 rounded-lg bg-card/40 hover:bg-card/80 transition-all duration-300 cursor-pointer"
      onClick={() => onPlay(item)}
    >
      <div className="relative aspect-video rounded-md overflow-hidden mb-3 shadow-md shadow-black/20 group-hover:shadow-lg group-hover:shadow-black/30 transition-shadow duration-300">
        {thumb ? (
          <img src={thumb} alt={item.title} loading="lazy" className="w-full h-full object-cover" />
        ) : (
          <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-muted to-muted/60">
            <FileVideo className="h-10 w-10 text-muted-foreground/20" />
          </div>
        )}

        <div className="absolute bottom-2 right-2 translate-y-2 opacity-0 group-hover:translate-y-0 group-hover:opacity-100 transition-all duration-300">
          <button
            className="h-10 w-10 rounded-full bg-primary flex items-center justify-center shadow-xl shadow-primary/30 hover:scale-105 hover:bg-primary/90 transition-transform active:scale-95"
            onClick={(e) => { e.stopPropagation(); onPlay(item); }}
          >
            <Play className="h-4 w-4 text-primary-foreground ml-0.5" />
          </button>
        </div>

        {item.duration && (
          <div className="absolute bottom-2 left-2 bg-black/70 backdrop-blur-sm rounded px-1.5 py-0.5 text-[10px] font-medium text-white/90 tabular-nums">
            {formatDuration(item.duration)}
          </div>
        )}
      </div>

      <div className="min-w-0">
        <p className="font-semibold text-sm truncate leading-tight">{item.title}</p>
        <p className="text-[12px] text-muted-foreground/50 mt-1">{item.size}</p>
      </div>
    </motion.div>
  );
});

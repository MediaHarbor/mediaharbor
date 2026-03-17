import { useState, memo, useMemo } from 'react';
import { Play, FolderOpen, Disc3, ChevronDown } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { Button } from '@/components/ui/button';
import type { AlbumItem, TrackItem } from '@/stores/useLibraryStore';

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

function formatQuality(quality?: string): { label: string; tier: 'hires' | 'lossless' | 'standard' } | null {
  if (!quality) return null;
  const q = quality.toLowerCase();
  if (q.includes('24bit') || q.includes('24-bit') || q.includes('hi-res')) {
    return { label: 'Hi-Res', tier: 'hires' };
  }
  if (q.includes('flac') || q.includes('16bit') || q.includes('lossless') || q.includes('alac')) {
    return { label: 'Lossless', tier: 'lossless' };
  }
  return { label: quality.split('/')[0]?.trim() || quality, tier: 'standard' };
}

interface AlbumCardProps {
  item: AlbumItem;
  view: 'grid' | 'list';
  onPlay: (item: AlbumItem, trackIndex?: number) => void;
  onSelect?: (item: AlbumItem) => void;
  onOpen: (path: string) => void;
  index?: number;
}

export const AlbumCard = memo(function AlbumCard({ item, view, onPlay, onSelect, onOpen, index = 0 }: AlbumCardProps) {
  const [expanded, setExpanded] = useState(false);
  const thumb = useMemo(() => thumbnailSrc(item.thumbnail), [item.thumbnail]);
  const totalDuration = useMemo(() => item.tracks.reduce((acc, t) => acc + (t.duration || 0), 0), [item.tracks]);
  const quality = useMemo(() => formatQuality(item.tracks[0]?.quality), [item.tracks]);

  if (view === 'list') {
    return (
      <motion.div
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.2, delay: Math.min(index * 0.02, 0.2) }}
        className="rounded-lg bg-card/50 overflow-hidden hover:bg-card/80 transition-colors duration-200"
      >
        <div
          className="flex items-center gap-3 px-3 py-2.5 cursor-pointer"
          onClick={() => setExpanded((e) => !e)}
        >
          <div className="relative shrink-0 group/art">
            {thumb ? (
              <img src={thumb} alt={item.album} loading="lazy" className="h-12 w-12 rounded-md object-cover" />
            ) : (
              <div className="h-12 w-12 rounded-md bg-muted flex items-center justify-center">
                <Disc3 className="h-5 w-5 text-muted-foreground/40" />
              </div>
            )}
            <button
              className="absolute inset-0 rounded-md bg-black/50 flex items-center justify-center opacity-0 group-hover/art:opacity-100 transition-opacity"
              onClick={(e) => { e.stopPropagation(); onPlay(item); }}
            >
              <Play className="h-4 w-4 text-white ml-0.5" />
            </button>
          </div>
          <div className="flex-1 min-w-0">
            <p className="font-medium text-[13px] truncate leading-tight">{item.album}</p>
            <p className="text-xs text-muted-foreground/70 truncate mt-0.5">{item.artist}</p>
          </div>
          <div className="flex items-center gap-3 shrink-0">
            <span className="text-[11px] text-muted-foreground/50 tabular-nums">
              {item.tracks.length} track{item.tracks.length !== 1 ? 's' : ''}
            </span>
            {totalDuration > 0 && (
              <span className="text-[11px] text-muted-foreground/40 tabular-nums">
                {formatDuration(totalDuration)}
              </span>
            )}
            {quality && (
              <span className={`text-[9px] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded ${
                quality.tier === 'hires' ? 'bg-amber-500/15 text-amber-500' :
                quality.tier === 'lossless' ? 'bg-emerald-500/15 text-emerald-500' :
                'bg-muted text-muted-foreground/60'
              }`}>
                {quality.label}
              </span>
            )}
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 rounded-full opacity-0 group-hover:opacity-100 text-muted-foreground/60"
              onClick={(e) => { e.stopPropagation(); onOpen(item.tracks[0]?.path ?? ''); }}
            >
              <FolderOpen className="h-3.5 w-3.5" />
            </Button>
            <ChevronDown className={`h-3.5 w-3.5 text-muted-foreground/40 transition-transform duration-200 ${expanded ? 'rotate-180' : ''}`} />
          </div>
        </div>
        <AnimatePresence>
          {expanded && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: 'auto', opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              transition={{ duration: 0.2 }}
              className="border-t border-border/20"
            >
              {item.tracks.map((track, i) => (
                <TrackRow key={track.path} track={track} index={i} onPlay={() => onPlay(item, i)} onOpen={onOpen} />
              ))}
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3, delay: Math.min(index * 0.03, 0.3) }}
      className="group relative p-3 rounded-lg bg-card/40 hover:bg-card/80 transition-all duration-300 cursor-pointer"
      onClick={() => onSelect ? onSelect(item) : onPlay(item)}
    >
      <div className="relative aspect-square rounded-md overflow-hidden mb-3 shadow-md shadow-black/20 group-hover:shadow-lg group-hover:shadow-black/30 transition-shadow duration-300">
        {thumb ? (
          <img
            src={thumb}
            alt={item.album}
            loading="lazy"
            className="w-full h-full object-cover"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-muted to-muted/60">
            <Disc3 className="h-12 w-12 text-muted-foreground/20" />
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

        {quality && quality.tier === 'hires' && (
          <div className="absolute top-2 left-2 text-[8px] font-bold uppercase tracking-widest px-1.5 py-0.5 rounded-sm backdrop-blur-md bg-amber-500/80 text-white">
            {quality.label}
          </div>
        )}
      </div>

      <div className="min-w-0">
        <p className="font-semibold text-sm truncate leading-tight">{item.album}</p>
        <p className="text-[12px] text-muted-foreground/60 truncate mt-1">{item.artist}</p>
        <p className="text-[11px] text-muted-foreground/40 mt-1">
          {item.tracks.length} track{item.tracks.length !== 1 ? 's' : ''}
          {totalDuration > 0 && ` · ${formatDuration(totalDuration)}`}
          {item.year !== 'Unknown' && ` · ${item.year}`}
        </p>
      </div>
    </motion.div>
  );
});

const TrackRow = memo(function TrackRow({ track, index, onPlay, onOpen }: {
  track: TrackItem;
  index: number;
  onPlay: () => void;
  onOpen: (path: string) => void;
}) {
  return (
    <div
      className="flex items-center gap-3 px-4 py-2 hover:bg-muted/20 transition-colors group/track cursor-pointer"
      onClick={onPlay}
    >
      <span className="text-[11px] text-muted-foreground/40 w-5 text-right shrink-0 tabular-nums">
        {index + 1}
      </span>
      <div className="flex-1 min-w-0">
        <p className="text-[13px] truncate">{track.title}</p>
      </div>
      {track.duration && (
        <span className="text-[11px] text-muted-foreground/40 shrink-0 tabular-nums">
          {formatDuration(track.duration)}
        </span>
      )}
      <Button
        variant="ghost"
        size="icon"
        className="h-6 w-6 shrink-0 opacity-0 group-hover/track:opacity-100 transition-opacity rounded-full"
        onClick={(e) => { e.stopPropagation(); onOpen(track.path); }}
      >
        <FolderOpen className="h-3 w-3" />
      </Button>
    </div>
  );
});

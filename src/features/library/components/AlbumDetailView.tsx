import { Play, Shuffle, ArrowLeft, FolderOpen, Disc3, Clock } from 'lucide-react';
import { motion } from 'framer-motion';
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

function formatTotalDuration(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h > 0) return `${h} hr ${m} min`;
  return `${m} min`;
}

interface AlbumDetailViewProps {
  album: AlbumItem;
  onBack: () => void;
  onPlay: (album: AlbumItem, trackIndex?: number) => void;
  onOpen: (path: string) => void;
}

export function AlbumDetailView({ album, onBack, onPlay, onOpen }: AlbumDetailViewProps) {
  const thumb = thumbnailSrc(album.thumbnail);
  const totalDuration = album.tracks.reduce((acc, t) => acc + (t.duration || 0), 0);

  const handleShuffle = () => {
    const randomIndex = Math.floor(Math.random() * album.tracks.length);
    onPlay(album, randomIndex);
  };

  return (
    <motion.div
      initial={{ opacity: 0, x: 40 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: -40 }}
      transition={{ duration: 0.25 }}
      className="space-y-6"
    >
      {/* Back button */}
      <button
        onClick={onBack}
        className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
      >
        <ArrowLeft className="h-4 w-4" />
        Library
      </button>

      {/* Album header */}
      <div className="flex gap-6 items-end">
        {/* Album art */}
        <div className="shrink-0">
          {thumb ? (
            <img
              src={thumb}
              alt={album.album}
              className="h-48 w-48 rounded-lg object-cover shadow-xl"
            />
          ) : (
            <div className="h-48 w-48 rounded-lg bg-gradient-to-br from-muted to-muted/60 flex items-center justify-center shadow-xl">
              <Disc3 className="h-16 w-16 text-muted-foreground/20" />
            </div>
          )}
        </div>

        {/* Album info */}
        <div className="flex-1 min-w-0 pb-1">
          <p className="text-xs font-medium uppercase tracking-widest text-muted-foreground/60 mb-1">Album</p>
          <h1 className="text-3xl font-bold truncate leading-tight">{album.album}</h1>
          <p className="text-base text-muted-foreground mt-1.5">{album.artist}</p>
          <div className="flex items-center gap-2 mt-2 text-sm text-muted-foreground/60">
            {album.year !== 'Unknown' && <span>{album.year}</span>}
            {album.year !== 'Unknown' && <span>·</span>}
            <span>{album.tracks.length} song{album.tracks.length !== 1 ? 's' : ''}</span>
            {totalDuration > 0 && (
              <>
                <span>·</span>
                <span>{formatTotalDuration(totalDuration)}</span>
              </>
            )}
          </div>

          {/* Action buttons */}
          <div className="flex items-center gap-3 mt-5">
            <Button
              size="sm"
              className="rounded-full gap-2 px-6 shadow-md"
              onClick={() => onPlay(album)}
            >
              <Play className="h-4 w-4 ml-0.5" />
              Play
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="rounded-full gap-2 px-5"
              onClick={handleShuffle}
            >
              <Shuffle className="h-3.5 w-3.5" />
              Shuffle
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 rounded-full"
              onClick={() => onOpen(album.tracks[0]?.path ?? '')}
              title="Show in folder"
            >
              <FolderOpen className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </div>

      {/* Track list */}
      <div className="rounded-lg overflow-hidden">
        {/* Table header */}
        <div className="flex items-center gap-3 px-4 py-2 text-[11px] uppercase tracking-wider text-muted-foreground/40 border-b border-border/20">
          <span className="w-8 text-right">#</span>
          <span className="flex-1">Title</span>
          <Clock className="h-3 w-3 mr-1" />
        </div>

        {/* Tracks */}
        {album.tracks.map((track, i) => (
          <TrackRow
            key={track.path}
            track={track}
            index={i}
            onPlay={() => onPlay(album, i)}
            onOpen={onOpen}
          />
        ))}
      </div>
    </motion.div>
  );
}

function TrackRow({ track, index, onPlay, onOpen }: {
  track: TrackItem;
  index: number;
  onPlay: () => void;
  onOpen: (path: string) => void;
}) {
  return (
    <div
      className="group flex items-center gap-3 px-4 py-2.5 hover:bg-card/60 transition-colors cursor-pointer rounded-md"
      onClick={onPlay}
    >
      {/* Track number / play icon */}
      <div className="w-8 text-right shrink-0">
        <span className="text-[13px] text-muted-foreground/40 tabular-nums group-hover:hidden">
          {index + 1}
        </span>
        <Play className="h-3.5 w-3.5 text-foreground hidden group-hover:inline-block ml-auto" />
      </div>

      {/* Track info */}
      <div className="flex-1 min-w-0">
        <p className="text-[13px] truncate font-medium">{track.title}</p>
      </div>

      {/* Folder button */}
      <Button
        variant="ghost"
        size="icon"
        className="h-7 w-7 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity rounded-full"
        onClick={(e) => { e.stopPropagation(); onOpen(track.path); }}
      >
        <FolderOpen className="h-3 w-3" />
      </Button>

      {/* Duration */}
      {track.duration && (
        <span className="text-[12px] text-muted-foreground/40 shrink-0 tabular-nums w-12 text-right">
          {formatDuration(track.duration)}
        </span>
      )}
    </div>
  );
}

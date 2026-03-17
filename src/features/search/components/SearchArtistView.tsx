import { motion } from 'framer-motion';
import { ArrowLeft, Disc3, Play } from 'lucide-react';

export interface AlbumInfo {
  id: string;
  title: string;
  thumbnail?: string;
  releaseDate?: string;
  trackCount?: number;
  url?: string;
}

interface SearchArtistViewProps {
  artistName: string;
  artistThumbnail?: string;
  followerCount?: number;
  genre?: string;
  albums: AlbumInfo[];
  onBack: () => void;
  onAlbumSelect: (album: AlbumInfo) => void;
}

export function SearchArtistView({
  artistName,
  artistThumbnail,
  followerCount,
  genre,
  albums,
  onBack,
  onAlbumSelect,
}: SearchArtistViewProps) {
  return (
    <motion.div
      initial={{ opacity: 0, x: 40 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: -40 }}
      transition={{ duration: 0.25 }}
      className="space-y-6"
    >
      <button
        onClick={onBack}
        className="flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground transition-colors"
      >
        <ArrowLeft className="h-4 w-4" />
        Search Results
      </button>

      <div className="flex items-center gap-3">
        <div className="h-10 w-10 rounded-full overflow-hidden shrink-0 bg-muted">
          {artistThumbnail ? (
            <img src={artistThumbnail} alt={artistName} className="w-full h-full object-cover" />
          ) : (
            <div className="w-full h-full flex items-center justify-center text-muted-foreground/30">
              <Disc3 className="h-5 w-5" />
            </div>
          )}
        </div>
        <div>
          <h3 className="font-semibold text-sm">{artistName}</h3>
          <p className="text-[11px] text-muted-foreground/50">
            {albums.length} album{albums.length !== 1 ? 's' : ''}
            {followerCount != null && ` · ${followerCount.toLocaleString()} followers`}
            {genre && ` · ${genre}`}
          </p>
        </div>
      </div>

      {albums.length === 0 ? (
        <div className="text-center py-16 text-muted-foreground/50">
          <p className="text-sm">No albums found</p>
        </div>
      ) : (
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 2xl:grid-cols-7 gap-1">
          {albums.map((album, i) => (
            <motion.div
              key={album.id}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 0.3, delay: Math.min(i * 0.03, 0.3) }}
              className="group relative p-3 rounded-lg bg-card/40 hover:bg-card/80 transition-all duration-300 cursor-pointer"
              onClick={() => onAlbumSelect(album)}
            >
              <div className="relative aspect-square rounded-md overflow-hidden mb-3 shadow-md shadow-black/20 group-hover:shadow-lg group-hover:shadow-black/30 transition-shadow duration-300">
                {album.thumbnail ? (
                  <img
                    src={album.thumbnail}
                    alt={album.title}
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
                    onClick={(e) => { e.stopPropagation(); onAlbumSelect(album); }}
                  >
                    <Play className="h-4 w-4 text-primary-foreground ml-0.5" />
                  </button>
                </div>
              </div>
              <div className="min-w-0">
                <p className="font-semibold text-sm truncate leading-tight">{album.title}</p>
                <p className="text-[11px] text-muted-foreground/40 mt-1">
                  {album.trackCount != null && `${album.trackCount} track${album.trackCount !== 1 ? 's' : ''}`}
                  {album.releaseDate && (album.trackCount != null ? ' · ' : '') + album.releaseDate.slice(0, 4)}
                </p>
              </div>
            </motion.div>
          ))}
        </div>
      )}
    </motion.div>
  );
}

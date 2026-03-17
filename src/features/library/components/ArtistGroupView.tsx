import { useMemo, useState, memo } from 'react';
import { Play, Disc3, ChevronRight } from 'lucide-react';
import { motion } from 'framer-motion';
import { AlbumCard } from './AlbumCard';
import type { AlbumItem } from '@/stores/useLibraryStore';

function thumbnailSrc(thumb?: { data: string; format: string }): string | null {
  if (!thumb) return null;
  return `data:${thumb.format};base64,${thumb.data}`;
}

interface ArtistGroupViewProps {
  albums: AlbumItem[];
  view: 'grid' | 'list';
  onSelectAlbum: (album: AlbumItem) => void;
  onPlay: (album: AlbumItem, trackIndex?: number) => void;
  onOpen: (path: string) => void;
  search: string;
}

interface ArtistGroup {
  name: string;
  albums: AlbumItem[];
  totalTracks: number;
  thumbnail: string | null;
}

export const ArtistGroupView = memo(function ArtistGroupView({ albums, view, onSelectAlbum, onPlay, onOpen, search }: ArtistGroupViewProps) {
  const [expandedArtist, setExpandedArtist] = useState<string | null>(null);

  const artists = useMemo(() => {
    const map = new Map<string, AlbumItem[]>();
    for (const album of albums) {
      const key = album.artist.toLowerCase().trim();
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(album);
    }

    let groups: ArtistGroup[] = Array.from(map.entries()).map(([, items]) => ({
      name: items[0].artist,
      albums: items.sort((a, b) => a.album.localeCompare(b.album)),
      totalTracks: items.reduce((sum, a) => sum + a.tracks.length, 0),
      thumbnail: thumbnailSrc(items.find((a) => a.thumbnail)?.thumbnail),
    }));

    groups.sort((a, b) => a.name.localeCompare(b.name));

    if (search.trim()) {
      const q = search.toLowerCase();
      groups = groups.filter(
        (g) => g.name.toLowerCase().includes(q) ||
               g.albums.some((a) => a.album.toLowerCase().includes(q))
      );
    }

    return groups;
  }, [albums, search]);

  if (artists.length === 0) {
    return (
      <div className="text-center py-16 text-muted-foreground/50">
        <p className="text-sm">No artists found</p>
      </div>
    );
  }

  if (view === 'grid') {
    return (
      <div className="space-y-8">
        {artists.map((artist, i) => (
          <motion.section
            key={artist.name}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.2, delay: Math.min(i * 0.03, 0.3) }}
          >
            <div className="flex items-center gap-3 mb-3">
              <div className="h-10 w-10 rounded-full overflow-hidden shrink-0 bg-muted">
                {artist.thumbnail ? (
                  <img src={artist.thumbnail} alt={artist.name} className="w-full h-full object-cover" />
                ) : (
                  <div className="w-full h-full flex items-center justify-center text-muted-foreground/30">
                    <Disc3 className="h-5 w-5" />
                  </div>
                )}
              </div>
              <div>
                <h3 className="font-semibold text-sm">{artist.name}</h3>
                <p className="text-[11px] text-muted-foreground/50">
                  {artist.albums.length} album{artist.albums.length !== 1 ? 's' : ''} · {artist.totalTracks} track{artist.totalTracks !== 1 ? 's' : ''}
                </p>
              </div>
            </div>

            <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 2xl:grid-cols-7 gap-1">
              {artist.albums.map((album, j) => (
                <AlbumCard
                  key={`${album.album}-${album.artist}-${j}`}
                  item={album}
                  view="grid"
                  onPlay={onPlay}
                  onSelect={onSelectAlbum}
                  onOpen={onOpen}
                  index={j}
                />
              ))}
            </div>
          </motion.section>
        ))}
      </div>
    );
  }

  return (
    <div className="space-y-1">
      {artists.map((artist, i) => (
        <motion.div
          key={artist.name}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.2, delay: Math.min(i * 0.02, 0.3) }}
        >
          <div
            className="flex items-center gap-3 px-3 py-2.5 rounded-lg hover:bg-card/60 transition-colors cursor-pointer"
            onClick={() => setExpandedArtist(expandedArtist === artist.name ? null : artist.name)}
          >
            <div className="h-11 w-11 rounded-full overflow-hidden shrink-0 bg-muted">
              {artist.thumbnail ? (
                <img src={artist.thumbnail} alt={artist.name} className="w-full h-full object-cover" />
              ) : (
                <div className="w-full h-full flex items-center justify-center text-muted-foreground/30">
                  <Disc3 className="h-5 w-5" />
                </div>
              )}
            </div>

            <div className="flex-1 min-w-0">
              <p className="font-semibold text-sm truncate">{artist.name}</p>
              <p className="text-[11px] text-muted-foreground/50">
                {artist.albums.length} album{artist.albums.length !== 1 ? 's' : ''} · {artist.totalTracks} track{artist.totalTracks !== 1 ? 's' : ''}
              </p>
            </div>

            <ChevronRight className={`h-4 w-4 text-muted-foreground/30 transition-transform duration-200 shrink-0 ${
              expandedArtist === artist.name ? 'rotate-90' : ''
            }`} />
          </div>

          {expandedArtist === artist.name && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: 'auto', opacity: 1 }}
              transition={{ duration: 0.2 }}
              className="ml-6 pl-8 border-l border-border/20 py-2 space-y-1"
            >
              {artist.albums.map((album) => {
                const thumb = thumbnailSrc(album.thumbnail);
                return (
                  <div
                    key={`${album.album}-${album.artist}`}
                    className="group flex items-center gap-3 px-3 py-2 rounded-lg hover:bg-card/60 transition-colors cursor-pointer"
                    onClick={() => onSelectAlbum(album)}
                  >
                    {thumb ? (
                      <img src={thumb} alt={album.album} className="h-10 w-10 rounded object-cover shrink-0" />
                    ) : (
                      <div className="h-10 w-10 rounded bg-muted flex items-center justify-center shrink-0">
                        <Disc3 className="h-4 w-4 text-muted-foreground/30" />
                      </div>
                    )}
                    <div className="flex-1 min-w-0">
                      <p className="text-[13px] font-medium truncate">{album.album}</p>
                      <p className="text-[11px] text-muted-foreground/50">
                        {album.tracks.length} track{album.tracks.length !== 1 ? 's' : ''}
                        {album.year !== 'Unknown' && ` · ${album.year}`}
                      </p>
                    </div>
                    <button
                      className="h-8 w-8 rounded-full flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity hover:bg-primary/10"
                      onClick={(e) => { e.stopPropagation(); onPlay(album); }}
                    >
                      <Play className="h-3.5 w-3.5 ml-0.5" />
                    </button>
                  </div>
                );
              })}
            </motion.div>
          )}
        </motion.div>
      ))}
    </div>
  );
});

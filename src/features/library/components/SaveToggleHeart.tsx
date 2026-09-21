import { Heart } from 'lucide-react';
import { useSavedStateStore } from '@/stores/useSavedStateStore';
import {
  useToggleSavedTrack,
  useToggleSavedAlbum,
  useToggleFollowedArtist,
  useToggleFollowedPlaylist,
  useSavedStateHydrate,
} from '@/features/library/hooks/useLibraryMutations';
import { useServiceCapabilities } from '@/features/library/hooks/useServiceCapabilities';
import type { SaveKindStr } from '@/tauri-bridge';
import { cn } from '@/utils/cn';

interface Props {
  platform: string;
  kind: SaveKindStr;
  id: string;
  label?: string;
  size?: 'sm' | 'md';
  className?: string;
  visibility?: 'auto' | 'always';
}

export function SaveToggleHeart({
  platform,
  kind,
  id,
  label,
  size = 'sm',
  className,
  visibility = 'auto',
}: Props) {
  const caps = useServiceCapabilities(platform);
  useSavedStateHydrate(platform, kind);

  const isSaved = useSavedStateStore((s) => s.isSaved(platform, kind, id));

  const trackMut = useToggleSavedTrack(platform);
  const albumMut = useToggleSavedAlbum(platform);
  const artistMut = useToggleFollowedArtist(platform);
  const playlistMut = useToggleFollowedPlaylist(platform);

  const supported = (() => {
    switch (kind) {
      case 'track':
        return caps.mutations?.save_tracks ?? false;
      case 'album':
        return caps.mutations?.save_albums ?? false;
      case 'artist':
        return caps.mutations?.follow_artists ?? false;
      case 'playlist':
        return caps.mutations?.follow_playlists ?? false;
    }
  })();

  if (!supported || !platform || platform === 'local') return null;

  const dim = size === 'sm' ? 'h-7 w-7' : 'h-9 w-9';
  const iconCls = size === 'sm' ? 'h-3.5 w-3.5' : 'h-4 w-4';

  const onToggle = (e: React.MouseEvent) => {
    e.stopPropagation();
    switch (kind) {
      case 'track':
        return trackMut.mutate({ id, save: !isSaved });
      case 'album':
        return albumMut.mutate({ id, save: !isSaved });
      case 'artist':
        return artistMut.mutate({ id, save: !isSaved });
      case 'playlist':
        return playlistMut.mutate({ id, save: !isSaved });
    }
  };

  const opacityCls =
    visibility === 'always' || isSaved ? 'opacity-100' : 'opacity-0 group-hover:opacity-100';

  return (
    <button
      onClick={onToggle}
      title={label ?? (isSaved ? 'Remove from library' : 'Save to library')}
      className={cn(
        'flex items-center justify-center rounded-md transition-all duration-150',
        dim,
        opacityCls,
        isSaved
          ? 'text-rose-500 hover:text-rose-400'
          : 'text-muted-foreground hover:text-foreground hover:bg-accent',
        className
      )}
      aria-pressed={isSaved}
    >
      <Heart className={iconCls} fill={isSaved ? 'currentColor' : 'none'} />
    </button>
  );
}

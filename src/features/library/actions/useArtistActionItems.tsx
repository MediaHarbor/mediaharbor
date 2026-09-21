import { useMemo } from 'react';
import { Heart, Radio, User } from 'lucide-react';
import { useSavedStateStore } from '@/stores/useSavedStateStore';
import {
  useToggleFollowedArtist,
  useStartRadio,
} from '@/features/library/hooks/useLibraryMutations';
import { usePlayerStore } from '@/stores/usePlayerStore';
import { useServiceCapabilities } from '@/features/library/hooks/useServiceCapabilities';
import { buildShareItem, radioTracksToQueue } from './shared';
import type { ActionContext, ActionItem, ArtistRef } from './types';

interface Args {
  artist: ArtistRef;
  platform: string;
  context: ActionContext;
  copyLink?: string;
  onGoToArtist?: (artistId: string) => void;
}

const iconCls = 'h-4 w-4';

export function useArtistActionItems({
  artist,
  platform,
  copyLink,
  onGoToArtist,
}: Args): ActionItem[] {
  const caps = useServiceCapabilities(platform);
  const isFollowed = useSavedStateStore((s) => s.isSaved(platform, 'artist', artist.id));
  const toggle = useToggleFollowedArtist(platform);
  const startRadio = useStartRadio();
  const setQueue = usePlayerStore((s) => s.setQueue);

  return useMemo<ActionItem[]>(() => {
    const canFollow = caps.mutations?.follow_artists ?? false;
    const canRadio = caps.mutations?.radio ?? false;
    const shareItem = buildShareItem({
      platform,
      kind: 'artist',
      id: artist.id,
      fallbackUrl: copyLink,
    });
    return [
      {
        id: 'go-artist',
        label: 'Go to artist',
        icon: <User className={iconCls} />,
        hidden: !onGoToArtist || !artist.id,
        action: () => {
          if (artist.id) onGoToArtist?.(artist.id);
        },
      },
      {
        id: 'follow',
        label: isFollowed ? 'Unfollow' : 'Follow',
        icon: <Heart className={iconCls} fill={isFollowed ? 'currentColor' : 'none'} />,
        action: () => toggle.mutate({ id: artist.id, save: !isFollowed }),
        hidden: !canFollow,
      },
      {
        id: 'start-radio',
        label: 'Start artist radio',
        icon: <Radio className={iconCls} />,
        action: () =>
          startRadio.mutate(
            { platform, seedKind: 'artist', seedId: artist.id },
            {
              onSuccess: (res) => {
                void (async () => {
                  const q = await radioTracksToQueue(res, platform);
                  if (!q.length) return;
                  setQueue(q, 0);
                })();
              },
            }
          ),
        hidden: !canRadio,
      },
      { id: 'sep-1', label: '', separator: true, hidden: shareItem.hidden },
      shareItem,
    ];
  }, [
    artist.id,
    platform,
    isFollowed,
    caps.mutations?.follow_artists,
    caps.mutations?.radio,
    copyLink,
    onGoToArtist,
    toggle,
    startRadio,
    setQueue,
  ]);
}

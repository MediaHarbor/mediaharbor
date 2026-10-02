import { useCallback, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Mic, Play } from 'lucide-react';
import {
  ipcPlatformOf,
  libraryKeys,
  queryEpisodeBookmarks,
  type ServicePlatform,
  toPlayable,
} from '@/features/library/api';
import { useVisibleCoverUrls } from '@/features/library/useCoverUrls';
import { renderLibraryQueryState } from '@/features/library/components/LibraryQueryState';
import { usePlayerStore } from '@/stores/usePlayerStore';
import type { PlayableTrack } from '@/stores/usePlayerStore';
import { formatDurationShort } from '@/utils/formatters';

interface EpisodeBookmarksViewProps {
  source: ServicePlatform;
}

export function EpisodeBookmarksView({ source }: EpisodeBookmarksViewProps) {
  const setQueue = usePlayerStore((s) => s.setQueue);
  const query = useQuery({
    queryKey: libraryKeys.episodeBookmarks(source),
    queryFn: () => queryEpisodeBookmarks(source),
    enabled: source !== 'local',
  });

  const items = useMemo(() => query.data ?? [], [query.data]);
  const covers = useVisibleCoverUrls(items.map((t) => t.cover_id));

  const playFrom = useCallback(
    (startAt: number) => {
      if (items.length === 0) return;
      const platform = ipcPlatformOf(source);
      const playable: PlayableTrack[] = items.map((t) =>
        toPlayable(t, platform, covers, { mediaType: 'audio' })
      );
      setQueue(playable, Math.min(startAt, playable.length - 1), null);
    },
    [items, covers, source, setQueue]
  );

  const stateView = renderLibraryQueryState({
    query,
    entity: 'bookmarked episodes',
    source,
    count: items.length,
  });
  if (stateView) return stateView;

  return (
    <div className="h-full overflow-y-auto flex flex-col gap-3">
      <h2 className="text-[12px] font-semibold tracking-wide uppercase text-muted-foreground/60">
        Continue listening
      </h2>
      <div className="flex flex-col">
        {items.map((t, i) => {
          const url = t.cover_id ? (covers[t.cover_id] ?? null) : null;
          return (
            <button
              key={`${t.path}-${i}`}
              type="button"
              onClick={() => playFrom(i)}
              className="group flex items-center gap-3 p-2 rounded-lg text-left hover:bg-card/60 transition-colors"
            >
              <div className="relative h-12 w-12 rounded-md overflow-hidden bg-muted shrink-0">
                {url ? (
                  <img
                    src={url}
                    alt={t.title ?? ''}
                    loading="lazy"
                    decoding="async"
                    className="w-full h-full object-cover"
                  />
                ) : (
                  <div className="w-full h-full flex items-center justify-center text-muted-foreground/30">
                    <Mic className="h-5 w-5" />
                  </div>
                )}
                <div className="absolute inset-0 items-center justify-center bg-background/60 hidden group-hover:flex">
                  <Play className="h-5 w-5" />
                </div>
              </div>
              <div className="min-w-0 flex-1">
                <p className="text-sm font-semibold truncate">{t.title}</p>
                <p className="text-[11px] text-muted-foreground/60 truncate">{t.artist}</p>
              </div>
              <span className="text-[11px] text-muted-foreground/40 shrink-0">
                {formatDurationShort(t.duration_secs)}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

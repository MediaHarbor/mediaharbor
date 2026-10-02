import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Disc3, Mic } from 'lucide-react';
import {
  libraryKeys,
  queryActivityFeed,
  type ActivityFeedItem,
  type ServicePlatform,
} from '@/features/library/api';
import { useVisibleCoverUrls } from '@/features/library/useCoverUrls';
import { renderLibraryQueryState } from '@/features/library/components/LibraryQueryState';
import { cn } from '@/utils/cn';

interface ActivityFeedViewProps {
  source: ServicePlatform;
  onSelectAlbum: (albumKey: string) => void;
}

function releasedLabel(iso?: string | null): string {
  if (!iso) return '';
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return '';
  const days = Math.floor((Date.now() - then) / 86_400_000);
  if (days <= 0) return 'Today';
  if (days === 1) return 'Yesterday';
  if (days < 7) return `${days} days ago`;
  if (days < 30) return `${Math.floor(days / 7)} week${days < 14 ? '' : 's'} ago`;
  return new Date(iso).toLocaleDateString();
}

interface FeedCardProps {
  item: ActivityFeedItem;
  url: string | null;
  onOpen: (() => void) | null;
}

function FeedCard({ item, url, onOpen }: FeedCardProps) {
  const Icon = item.kind === 'episode' ? Mic : Disc3;
  return (
    <button
      type="button"
      disabled={!onOpen}
      onClick={() => onOpen?.()}
      className={cn(
        'group flex flex-col gap-2 p-3 rounded-xl text-left w-full transition-colors',
        onOpen ? 'hover:bg-card/60' : 'cursor-default'
      )}
    >
      <div className="relative aspect-square w-full rounded-lg overflow-hidden bg-muted">
        {url ? (
          <img
            src={url}
            alt={item.title}
            loading="lazy"
            decoding="async"
            className="w-full h-full object-cover"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center text-muted-foreground/30">
            <Icon className="h-10 w-10" />
          </div>
        )}
        {!item.seen && (
          <span
            className="absolute top-2 left-2 px-1.5 py-0.5 rounded text-[10px] font-bold uppercase tracking-wide text-background"
            style={{ backgroundColor: 'var(--service-accent)' }}
          >
            New
          </span>
        )}
      </div>
      <div className="min-w-0">
        <p className="text-sm font-semibold truncate">{item.title}</p>
        <p className="text-[11px] text-muted-foreground/60 truncate">{item.subtitle}</p>
        <p className="text-[11px] text-muted-foreground/40">{releasedLabel(item.occurred_at)}</p>
      </div>
    </button>
  );
}

export function ActivityFeedView({ source, onSelectAlbum }: ActivityFeedViewProps) {
  const query = useQuery({
    queryKey: libraryKeys.activityFeed(source),
    queryFn: () => queryActivityFeed(source),
    enabled: source !== 'local',
  });

  const items = useMemo(() => query.data ?? [], [query.data]);
  const covers = useVisibleCoverUrls(items.map((i) => i.cover_id));

  const stateView = renderLibraryQueryState({
    query,
    entity: 'activity',
    source,
    count: items.length,
  });
  if (stateView) return stateView;

  return (
    <div className="h-full overflow-y-auto">
      <div className="grid gap-3 grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
        {items.map((item) => (
          <FeedCard
            key={item.id}
            item={item}
            url={item.cover_id ? (covers[item.cover_id] ?? null) : null}
            onOpen={
              item.kind === 'album' && item.service_id
                ? () => onSelectAlbum(item.service_id as string)
                : null
            }
          />
        ))}
      </div>
    </div>
  );
}

import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { User } from 'lucide-react';
import {
  libraryKeys,
  queryFollowers,
  queryFollowing,
  type ServicePlatform,
  type ServiceUserDto,
} from '@/features/library/api';
import { useVisibleCoverUrls } from '@/features/library/useCoverUrls';
import { renderLibraryQueryState } from '@/features/library/components/LibraryQueryState';

interface FollowingViewProps {
  source: ServicePlatform;
}

interface UserGridProps {
  users: ServiceUserDto[];
  covers: Record<string, string | null>;
}

function UserGrid({ users, covers }: UserGridProps) {
  return (
    <div className="grid gap-3 grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
      {users.map((u) => {
        const url = u.cover_id ? (covers[u.cover_id] ?? null) : null;
        return (
          <div
            key={u.key}
            className="flex flex-col items-center text-center gap-2 p-3 rounded-xl hover:bg-card/60 transition-colors"
          >
            <div className="h-24 w-24 rounded-full overflow-hidden bg-muted shrink-0">
              {url ? (
                <img
                  src={url}
                  alt={u.display}
                  loading="lazy"
                  decoding="async"
                  className="w-full h-full object-cover"
                />
              ) : (
                <div className="w-full h-full flex items-center justify-center text-muted-foreground/30">
                  <User className="h-8 w-8" />
                </div>
              )}
            </div>
            <p className="text-sm font-semibold truncate w-full">{u.display || 'Unknown user'}</p>
          </div>
        );
      })}
    </div>
  );
}

export function FollowingView({ source }: FollowingViewProps) {
  const followers = useQuery({
    queryKey: libraryKeys.followers(source),
    queryFn: () => queryFollowers(source),
    enabled: source !== 'local',
  });
  const following = useQuery({
    queryKey: libraryKeys.following(source),
    queryFn: () => queryFollowing(source),
    enabled: source !== 'local',
  });

  const followerList = useMemo(() => followers.data ?? [], [followers.data]);
  const followingList = useMemo(() => following.data ?? [], [following.data]);

  const covers = useVisibleCoverUrls([
    ...followerList.map((u) => u.cover_id),
    ...followingList.map((u) => u.cover_id),
  ]);

  const stateView = renderLibraryQueryState({
    query: {
      isPending: followers.isPending || following.isPending,
      isError: followers.isError || following.isError,
      error: followers.error ?? following.error,
    },
    entity: 'connections',
    source,
    count: followerList.length + followingList.length,
  });
  if (stateView) return stateView;

  return (
    <div className="h-full overflow-y-auto flex flex-col gap-8">
      <section className="flex flex-col gap-3">
        <h2 className="text-[12px] font-semibold tracking-wide uppercase text-muted-foreground/60">
          Following · {followingList.length}
        </h2>
        {followingList.length > 0 ? (
          <UserGrid users={followingList} covers={covers} />
        ) : (
          <p className="text-sm text-muted-foreground/50">Not following anyone yet.</p>
        )}
      </section>

      <section className="flex flex-col gap-3">
        <h2 className="text-[12px] font-semibold tracking-wide uppercase text-muted-foreground/60">
          Followers · {followerList.length}
        </h2>
        {followerList.length > 0 ? (
          <UserGrid users={followerList} covers={covers} />
        ) : (
          <p className="text-sm text-muted-foreground/50">No followers yet.</p>
        )}
      </section>
    </div>
  );
}

import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { queryExplore, libraryKeys, type ServicePlatform } from '@/features/library/api';

type BrowseItem = { kind: string; api_path?: string; title?: string; icon?: string };
type BrowseShelf = { id: string; title: string; category?: string; items: BrowseItem[] };

/**
 * Browse/discovery surface shown in Search when there's no query yet.
 * Renders the service's genre/mood/decade navigation tiles (from the explore
 * endpoint) as pill groups. Clicking a pill drills into that category.
 * Only services whose explore returns data (Tidal today) show anything.
 */
export function SearchBrowse({
  platform,
  onOpenCategory,
  fallback = null,
}: {
  platform: string;
  onOpenCategory: (apiPath: string, title: string) => void;
  fallback?: ReactNode;
}) {
  const svc = platform as ServicePlatform;
  const { data, isPending } = useQuery({
    queryKey: libraryKeys.explore(svc),
    queryFn: () => queryExplore(svc),
    staleTime: 5 * 60_000,
  });

  const shelves = ((data?.shelves ?? []) as BrowseShelf[]).filter(
    (s) => s.category === 'genre' && s.items.length > 0
  );

  if (isPending || shelves.length === 0) {
    return <>{fallback}</>;
  }

  return (
    <div className="space-y-7 py-2">
      {shelves.map((shelf) => (
        <section key={shelf.id || shelf.title}>
          {shelf.title && (
            <h2 className="text-[13px] font-semibold tracking-tight text-muted-foreground/80 mb-2.5">
              {shelf.title}
            </h2>
          )}
          <div className="flex flex-wrap gap-2">
            {shelf.items.map((it, i) => (
              <button
                key={it.api_path || i}
                type="button"
                onClick={() => it.api_path && onOpenCategory(it.api_path, it.title ?? '')}
                className={
                  'inline-flex items-center rounded-full border border-border/60 bg-card/40 ' +
                  'px-4 py-2 text-[13px] font-medium text-foreground/80 ' +
                  'hover:text-foreground hover:border-[color:var(--platform-accent,theme(colors.primary.DEFAULT))] ' +
                  'hover:bg-card/80 transition-colors duration-150'
                }
              >
                {it.title ?? ''}
              </button>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

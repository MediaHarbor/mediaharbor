import { useMediaNavigation } from '@/features/library/actions/useMediaNavigation';
import { normalizePlatform } from '@/utils/platform-data';
import { cn } from '@/utils/cn';

interface EntityLinkProps {
  kind: 'artist' | 'album';
  id?: string | null;
  platform: string;
  source?: string;
  className?: string;
  children: React.ReactNode;
}

export function EntityLink({
  kind,
  id,
  platform,
  source = 'unknown',
  className,
  children,
}: EntityLinkProps) {
  const nav = useMediaNavigation();
  const linkable = !!id && source !== 'local';
  if (!linkable) {
    return <span className={cn('truncate', className)}>{children}</span>;
  }
  const target = normalizePlatform(platform);
  return (
    <button
      className={cn('truncate text-left hover:text-foreground hover:underline', className)}
      onClick={(e) => {
        e.stopPropagation();
        if (kind === 'artist') nav.goToArtist(target, id!);
        else nav.goToAlbum(target, id!);
      }}
    >
      {children}
    </button>
  );
}

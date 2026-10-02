import { useState } from 'react';
import { Radio } from 'lucide-react';
import { cn } from '@/utils/cn';

/**
 * A station's logo, or the placeholder.
 *
 * The failure is tracked in state rather than by mutating the element, because
 * React owns that node: hiding it left an empty square, and removing it outright
 * hands React a tree that no longer matches its own.
 */
export function StationIcon({
  src,
  className,
  iconClassName,
}: {
  src?: string | null;
  className?: string;
  iconClassName?: string;
}) {
  const [failed, setFailed] = useState(false);
  if (!src || failed) {
    return (
      <span className={cn('flex items-center justify-center', className)}>
        <Radio className={cn('text-muted-foreground/40', iconClassName)} />
      </span>
    );
  }
  return (
    <img
      src={src}
      alt=""
      loading="lazy"
      className={cn('object-cover', className)}
      onError={() => setFailed(true)}
    />
  );
}

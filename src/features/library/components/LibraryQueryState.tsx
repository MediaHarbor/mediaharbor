import type { ReactElement } from 'react';
import { errorMessage } from '@/features/library/api';
import { LibraryEmptyState } from './LibraryEmptyState';

const AUTH_ERROR_PREFIX = 'Authentication error:';
const NO_SUBSCRIPTION_MARKER = 'no-subscription:';

export function renderLibraryQueryState(opts: {
  query: { isPending: boolean; isError: boolean; error: unknown };
  entity: string;
  source: string;
  count: number;
}): ReactElement | null {
  const { query, entity, source, count } = opts;
  if (query.isPending) {
    return (
      <div className="py-16 text-center text-sm text-muted-foreground/50">Loading {entity}…</div>
    );
  }
  if (query.isError) {
    const message = errorMessage(query.error);
    if (message.includes(NO_SUBSCRIPTION_MARKER)) {
      const detail = message
        .slice(message.indexOf(NO_SUBSCRIPTION_MARKER) + NO_SUBSCRIPTION_MARKER.length)
        .trim();
      return <LibraryEmptyState variant="subscription" detail={detail || undefined} />;
    }
    if (message.includes(AUTH_ERROR_PREFIX)) {
      const detail = message
        .slice(message.indexOf(AUTH_ERROR_PREFIX) + AUTH_ERROR_PREFIX.length)
        .trim();
      return <LibraryEmptyState variant="auth" detail={detail || undefined} />;
    }
    return (
      <div className="py-16 text-center text-sm text-red-400/80 whitespace-pre-wrap">
        {`Failed to load ${entity} from ${source}:\n${message}`}
      </div>
    );
  }
  if (count === 0) {
    return (
      <div className="py-16 text-center text-sm text-muted-foreground/50">No {entity} found</div>
    );
  }
  return null;
}

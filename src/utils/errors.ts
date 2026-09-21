/**
 * Best human-readable message for a caught value. `catch` gives `unknown`, which
 * may be an Error, a bare string, a backend JSON payload, or anything else.
 */
export function errorMessage(err: unknown): string {
  if (err == null) return 'unknown error';
  if (typeof err === 'string') return err;
  if (
    typeof err === 'object' &&
    'message' in err &&
    typeof (err as { message?: unknown }).message === 'string'
  ) {
    return (err as { message: string }).message;
  }
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}

/**
 * As {@link errorMessage}, but prefers a stack when there is one — for the log
 * pane, where the call site matters more than brevity.
 */
export function errorDetail(err: unknown): string {
  if (err instanceof Error && err.stack) return err.stack;
  return errorMessage(err);
}

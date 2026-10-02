/**
 * Stable non-cryptographic hash of a string, for deriving a deterministic
 * number from an id — a placeholder gradient, a synthetic key. Always
 * positive, never 0, so callers can use it as a truthy seed directly.
 */
export function hashString(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (Math.imul(31, h) + s.charCodeAt(i)) | 0;
  return Math.abs(h) || 1;
}

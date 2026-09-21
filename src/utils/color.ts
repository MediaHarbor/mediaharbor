/**
 * Split a `#rrggbb` string into RGB components. Unparseable channels fall back
 * to 136 (`#888`), so a malformed platform colour still renders something.
 */
export function parseHex(hex: string): [number, number, number] {
  const c = hex.replace('#', '');
  return [
    parseInt(c.slice(0, 2), 16) || 136,
    parseInt(c.slice(2, 4), 16) || 136,
    parseInt(c.slice(4, 6), 16) || 136,
  ];
}

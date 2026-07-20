/** Next favorite level on click: 0→1→2→3→4→5→0. */
export function cycleFavoriteLevel(current: number): number {
  const n = Math.max(0, Math.min(5, Math.floor(current)));
  return n >= 5 ? 0 : n + 1;
}

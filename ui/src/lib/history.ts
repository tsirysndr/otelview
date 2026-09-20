/** Merge logic for recently applied search queries. The state itself lives
 * in a jotai atom family backed by localStorage (`queryHistoryFamily` in
 * state/atoms.ts) — this module only holds the pure dedupe/cap rule so it's
 * testable without mounting anything. */

export const HISTORY_LIMIT = 15;

/** Record one applied query: most recent first, no duplicates, capped. */
export function withAppliedQuery(history: string[], query: string): string[] {
  const q = query.trim();
  if (!q) return history;
  return [q, ...history.filter((h) => h !== q)].slice(0, HISTORY_LIMIT);
}

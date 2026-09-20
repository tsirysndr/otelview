/** Recently applied search queries, per input, persisted in localStorage.
 *
 * A tiny module rather than component state so the logic — dedupe to the
 * front, cap, survive malformed storage — is testable on its own. */

const LIMIT = 15;

const storageKey = (key: string) => `otelview.history.${key}`;

export function loadHistory(key: string): string[] {
  try {
    const raw = localStorage.getItem(storageKey(key));
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((q) => typeof q === "string") : [];
  } catch {
    return [];
  }
}

/** Record one applied query: most recent first, no duplicates, capped. */
export function pushHistory(key: string, query: string): string[] {
  const q = query.trim();
  if (!q) return loadHistory(key);
  const next = [q, ...loadHistory(key).filter((h) => h !== q)].slice(0, LIMIT);
  try {
    localStorage.setItem(storageKey(key), JSON.stringify(next));
  } catch {
    // Storage full or denied — history is a convenience, never an error.
  }
  return next;
}

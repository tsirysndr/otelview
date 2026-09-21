/** Named queries the user chose to keep, as opposed to the automatic
 * most-recent history in [`./history`].
 *
 * Stored browser-side for now. Ids and `createdAt` exist so a future sync
 * layer has something stable to merge on. */

export type QueryKind =
  | "traces.attributes"
  | "traces.traceql"
  | "traces.lucene"
  | "logs.kql"
  | "logs.lucene";

export interface SavedQuery {
  id: string;
  /** What the user calls it; falls back to the query text itself. */
  name: string;
  kind: QueryKind;
  query: string;
  /** Unix millis — absolute, so it survives being synced across machines. */
  createdAt: number;
}

export const SAVED_LIMIT = 100;

/** Human label for the signal a saved query belongs to. */
export const KIND_LABEL: Record<QueryKind, string> = {
  "traces.attributes": "traces",
  "traces.traceql": "traceql",
  "traces.lucene": "traces · lucene",
  "logs.kql": "logs",
  "logs.lucene": "logs · lucene",
};

export function newQueryId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `q-${Math.random().toString(36).slice(2)}${Date.now().toString(36)}`;
}

function isKind(v: unknown): v is QueryKind {
  return (
    v === "traces.attributes" ||
    v === "traces.traceql" ||
    v === "traces.lucene" ||
    v === "logs.kql" ||
    v === "logs.lucene"
  );
}

/** Tolerate anything in storage; drop entries that are not usable. */
export function normalizeSaved(raw: unknown): SavedQuery[] {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((r, i) => {
    if (typeof r !== "object" || r === null) return [];
    const o = r as Record<string, unknown>;
    if (typeof o.query !== "string" || !o.query.trim()) return [];
    if (!isKind(o.kind)) return [];
    return [
      {
        id: typeof o.id === "string" && o.id ? o.id : `saved-${i}`,
        name:
          typeof o.name === "string" && o.name.trim() ? o.name.trim() : o.query.trim(),
        kind: o.kind,
        query: o.query,
        createdAt: typeof o.createdAt === "number" ? o.createdAt : 0,
      },
    ];
  });
}

/** Save a query under a name. Re-saving the same query for the same signal
 * renames the existing entry instead of making a duplicate. */
export function saveQuery(
  all: SavedQuery[],
  entry: { name: string; kind: QueryKind; query: string; id?: string; createdAt?: number },
): SavedQuery[] {
  const query = entry.query.trim();
  if (!query) return all;
  const name = entry.name.trim() || query;
  const existing = all.find(
    (s) => s.kind === entry.kind && (entry.id ? s.id === entry.id : s.query === query),
  );
  if (existing) {
    return all.map((s) => (s.id === existing.id ? { ...s, name, query } : s));
  }
  const next: SavedQuery = {
    id: entry.id ?? newQueryId(),
    name,
    kind: entry.kind,
    query,
    createdAt: entry.createdAt ?? Date.now(),
  };
  // Newest first, matching how history reads.
  return [next, ...all].slice(0, SAVED_LIMIT);
}

export function removeSaved(all: SavedQuery[], id: string): SavedQuery[] {
  return all.filter((s) => s.id !== id);
}

export function savedFor(all: SavedQuery[], kind: QueryKind): SavedQuery[] {
  return all.filter((s) => s.kind === kind);
}

/** Is this exact query already saved for this signal? */
export function findSaved(
  all: SavedQuery[],
  kind: QueryKind,
  query: string,
): SavedQuery | undefined {
  const q = query.trim();
  return all.find((s) => s.kind === kind && s.query === q);
}

/** Saved queries whose name or text matches, for the command palette. */
export function searchSaved(all: SavedQuery[], term: string): SavedQuery[] {
  const t = term.trim().toLowerCase();
  if (!t) return all;
  return all.filter(
    (s) => s.name.toLowerCase().includes(t) || s.query.toLowerCase().includes(t),
  );
}

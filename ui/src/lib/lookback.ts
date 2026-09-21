/** The quick-lookback strings the time bar offers, as milliseconds.
 *
 * The server parses these too; this is the client-side half, needed only to
 * turn the window you are already viewing into an editable absolute range
 * when you switch to the custom picker. */
const UNIT_MS: Record<string, number> = {
  s: 1_000,
  m: 60_000,
  h: 3_600_000,
  d: 86_400_000,
};

/** `null` for "all", which has no start, and for anything unparseable. */
export function lookbackMs(lookback: string): number | null {
  const s = lookback.trim();
  if (!s || s === "all") return null;
  const m = /^(\d+(?:\.\d+)?)([smhd])$/.exec(s);
  if (!m) return null;
  return Number(m[1]) * UNIT_MS[m[2]];
}

/** How far back to seed the custom picker when leaving a lookback behind.
 * "all" has no start to carry over, so it falls back to a day. */
export const FALLBACK_WINDOW_MS = 24 * 60 * 60 * 1000;

/** The absolute window equivalent to a lookback, ending now. */
export function lookbackToRange(lookback: string, nowMs: number) {
  const span = lookbackMs(lookback) ?? FALLBACK_WINDOW_MS;
  return { from: nowMs - span, to: nowMs };
}

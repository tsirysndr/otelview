// Chart + service color assignment.
//
// The categorical palette is CVD-validated against both the dark (#1E1C3F)
// and light (#FFFFFF) surfaces (dataviz six-checks validator). Hues are
// assigned to services in FIRST-SEEN order and stay stable for the session —
// color follows the entity, never its rank.

export const CATEGORICAL = [
  "#0891B2", // cyan
  "#FF2A6D", // magenta
  "#A16207", // gold
  "#B026FF", // purple
  "#059669", // green
  "#C2410C", // orange
] as const;

const assigned = new Map<string, string>();

/** Stable color per service; beyond the palette, entries fold into gray. */
export function serviceColor(service: string): string {
  const existing = assigned.get(service);
  if (existing) return existing;
  const color =
    assigned.size < CATEGORICAL.length
      ? CATEGORICAL[assigned.size]
      : "#8B87B3"; // "Other" — muted, identified by label not hue
  assigned.set(service, color);
  return color;
}

/** Color for the i-th metric series (fixed order, folds into gray). */
export function seriesColor(i: number): string {
  return i < CATEGORICAL.length ? CATEGORICAL[i] : "#8B87B3";
}

export const STATUS = {
  error: "#FF3864",
  warn: "#FFD319",
  ok: "#05FFA1",
} as const;

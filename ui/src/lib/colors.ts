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

// Neon twins of the categorical slots — used for span bars and accents,
// where marks sit on dark chrome and get a matching glow. Charts keep the
// validated palette above.
export const NEON = [
  "#05D9E8", // cyan
  "#FF2A6D", // magenta
  "#FFD319", // yellow
  "#B026FF", // purple
  "#05FFA1", // green
  "#FF8B39", // orange
] as const;

const assigned = new Map<string, string>();
const indexOfService = new Map<string, number>();

/** Stable color per service; beyond the palette, entries fold into gray. */
export function serviceColor(service: string): string {
  const existing = assigned.get(service);
  if (existing) return existing;
  indexOfService.set(service, assigned.size);
  const color =
    assigned.size < CATEGORICAL.length
      ? CATEGORICAL[assigned.size]
      : "#8B87B3"; // "Other" — muted, identified by label not hue
  assigned.set(service, color);
  return color;
}

/** Neon variant of the service's hue plus a matching glow shadow. */
export function serviceNeon(service: string): { color: string; glow: string } {
  serviceColor(service); // ensure the slot is assigned
  const i = indexOfService.get(service) ?? NEON.length;
  const color = i < NEON.length ? NEON[i] : "#8B87B3";
  return { color, glow: `0 0 6px ${color}99` };
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

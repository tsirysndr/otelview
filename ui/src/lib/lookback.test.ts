import { describe, expect, it } from "vitest";
import { FALLBACK_WINDOW_MS, lookbackMs, lookbackToRange } from "./lookback";

describe("lookbackMs", () => {
  it("parses every lookback the bar offers", () => {
    expect(lookbackMs("5m")).toBe(5 * 60_000);
    expect(lookbackMs("15m")).toBe(15 * 60_000);
    expect(lookbackMs("1h")).toBe(3_600_000);
    expect(lookbackMs("6h")).toBe(6 * 3_600_000);
    expect(lookbackMs("24h")).toBe(24 * 3_600_000);
    expect(lookbackMs("7d")).toBe(7 * 86_400_000);
  });

  it("treats 'all' as having no start", () => {
    expect(lookbackMs("all")).toBeNull();
  });

  it("returns null rather than guessing at junk", () => {
    for (const v of ["", "  ", "nope", "5", "5x", "-5m", "m", "1h30m"]) {
      expect(lookbackMs(v), v).toBeNull();
    }
  });

  it("tolerates surrounding space", () => {
    expect(lookbackMs("  1h ")).toBe(3_600_000);
  });
});

describe("lookbackToRange", () => {
  const now = 1_700_000_000_000;

  it("ends now and starts one window back", () => {
    expect(lookbackToRange("1h", now)).toEqual({ from: now - 3_600_000, to: now });
  });

  it("falls back to a day for 'all', which has no start to carry over", () => {
    expect(lookbackToRange("all", now)).toEqual({
      from: now - FALLBACK_WINDOW_MS,
      to: now,
    });
  });

  it("produces integer bounds, since the API rejects fractional ms", () => {
    const r = lookbackToRange("7d", now);
    expect(Number.isInteger(r.from)).toBe(true);
    expect(Number.isInteger(r.to)).toBe(true);
  });
});

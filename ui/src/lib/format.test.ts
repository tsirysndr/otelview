import { describe, expect, it } from "vitest";
import { bodyPreview, fmtCount, fmtDuration, severityInfo } from "./format";

describe("fmtDuration", () => {
  it("scales units", () => {
    expect(fmtDuration(420)).toBe("420ns");
    expect(fmtDuration(4_200)).toBe("4.2µs");
    expect(fmtDuration(4_200_000)).toBe("4.2ms");
    expect(fmtDuration(4_200_000_000)).toBe("4.20s");
    expect(fmtDuration(90_000_000_000)).toBe("1.5m");
  });
});

describe("fmtCount", () => {
  it("abbreviates", () => {
    expect(fmtCount(999)).toBe("999");
    expect(fmtCount(1_500)).toBe("1.5k");
    expect(fmtCount(2_400_000)).toBe("2.4M");
  });
});

describe("severityInfo", () => {
  it("maps OTLP severity numbers to levels", () => {
    expect(severityInfo(9).level).toBe("INFO");
    expect(severityInfo(17).level).toBe("ERROR");
    expect(severityInfo(13).level).toBe("WARN");
    expect(severityInfo(0).level).toBe("—");
  });
  it("prefers explicit severity text", () => {
    expect(severityInfo(17, "Critical").level).toBe("CRITICAL");
  });
});

describe("bodyPreview", () => {
  it("passes strings through and serializes objects", () => {
    expect(bodyPreview("hello")).toBe("hello");
    expect(bodyPreview({ a: 1 })).toBe('{"a":1}');
    expect(bodyPreview(null)).toBe("");
  });
});

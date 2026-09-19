import { describe, expect, it } from "vitest";
import { CATEGORICAL, serviceColor } from "./colors";

describe("serviceColor", () => {
  it("assigns hues in first-seen order and keeps them stable", () => {
    const a = serviceColor("svc-test-a");
    const b = serviceColor("svc-test-b");
    expect(a).not.toBe(b);
    // stable on re-query — color follows the entity
    expect(serviceColor("svc-test-a")).toBe(a);
  });

  it("folds services beyond the palette into gray", () => {
    for (let i = 0; i < CATEGORICAL.length + 2; i++) {
      serviceColor(`overflow-${i}`);
    }
    expect(serviceColor("overflow-extra")).toBe("#8B87B3");
  });
});

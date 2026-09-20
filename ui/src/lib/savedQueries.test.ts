import { describe, expect, it } from "vitest";
import {
  findSaved,
  normalizeSaved,
  removeSaved,
  saveQuery,
  savedFor,
  searchSaved,
  SAVED_LIMIT,
  type SavedQuery,
} from "./savedQueries";

const base: SavedQuery[] = [
  {
    id: "1",
    name: "5xx",
    kind: "logs.kql",
    query: "status_code:>=500",
    createdAt: 10,
  },
  {
    id: "2",
    name: "slow checkout",
    kind: "traces.traceql",
    query: "{ duration > 1s }",
    createdAt: 20,
  },
];

describe("normalizing stored saved queries", () => {
  it("round-trips valid entries", () => {
    expect(normalizeSaved(base)).toEqual(base);
  });

  it("drops entries with no query or an unknown kind", () => {
    const got = normalizeSaved([
      base[0],
      { id: "x", name: "n", kind: "logs.kql", query: "   " },
      { id: "y", name: "n", kind: "metrics.promql", query: "up" },
      null,
      42,
    ]);
    expect(got).toHaveLength(1);
    expect(got[0].id).toBe("1");
  });

  it("falls back to the query text when a name is missing", () => {
    const got = normalizeSaved([
      { kind: "logs.kql", query: "level:error", createdAt: 1 },
    ]);
    expect(got[0].name).toBe("level:error");
    expect(got[0].id).toBeTruthy();
  });

  it("treats junk as empty", () => {
    for (const junk of [null, undefined, {}, "nope", 7]) {
      expect(normalizeSaved(junk)).toEqual([]);
    }
  });
});

describe("saving", () => {
  it("adds a new query newest-first", () => {
    const got = saveQuery(base, {
      name: "errors",
      kind: "traces.traceql",
      query: "{ status = error }",
    });
    expect(got).toHaveLength(3);
    expect(got[0].name).toBe("errors");
  });

  it("renames rather than duplicating the same query for the same signal", () => {
    const got = saveQuery(base, {
      name: "server errors",
      kind: "logs.kql",
      query: "status_code:>=500",
    });
    expect(got).toHaveLength(2);
    expect(findSaved(got, "logs.kql", "status_code:>=500")!.name).toBe("server errors");
  });

  it("keeps the same text under a different signal as a separate entry", () => {
    const got = saveQuery(base, {
      name: "dup",
      kind: "traces.attributes",
      query: "status_code:>=500",
    });
    expect(got).toHaveLength(3);
  });

  it("names an unnamed query after its text, and ignores blank queries", () => {
    const got = saveQuery(base, { name: "  ", kind: "logs.kql", query: "level:warn" });
    expect(got[0].name).toBe("level:warn");
    expect(saveQuery(base, { name: "x", kind: "logs.kql", query: "  " })).toEqual(base);
  });

  it("caps the list", () => {
    let all: SavedQuery[] = [];
    for (let i = 0; i < SAVED_LIMIT + 10; i++) {
      all = saveQuery(all, { name: `q${i}`, kind: "logs.kql", query: `q:${i}` });
    }
    expect(all).toHaveLength(SAVED_LIMIT);
    expect(all[0].name).toBe(`q${SAVED_LIMIT + 9}`);
  });
});

describe("reading", () => {
  it("filters by signal", () => {
    expect(savedFor(base, "logs.kql").map((s) => s.id)).toEqual(["1"]);
    expect(savedFor(base, "traces.attributes")).toEqual([]);
  });

  it("finds an exact query, ignoring surrounding space", () => {
    expect(findSaved(base, "logs.kql", "  status_code:>=500 ")!.id).toBe("1");
    expect(findSaved(base, "traces.traceql", "status_code:>=500")).toBeUndefined();
  });

  it("searches name and text", () => {
    expect(searchSaved(base, "5xx").map((s) => s.id)).toEqual(["1"]);
    expect(searchSaved(base, "duration").map((s) => s.id)).toEqual(["2"]);
    expect(searchSaved(base, "CHECKOUT").map((s) => s.id)).toEqual(["2"]);
    expect(searchSaved(base, "")).toEqual(base);
  });

  it("removes by id", () => {
    expect(removeSaved(base, "1").map((s) => s.id)).toEqual(["2"]);
    expect(removeSaved(base, "nope")).toEqual(base);
  });
});

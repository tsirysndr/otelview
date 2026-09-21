import { describe, expect, it } from "vitest";
import { filterGraph, matches } from "./search";
import type { ServiceGraph } from "./api";

const graph: ServiceGraph = {
  nodes: [
    { service: "gateway", span_count: 10, error_count: 0, avg_ms: 1 },
    { service: "checkout", span_count: 8, error_count: 1, avg_ms: 2 },
    { service: "payments", span_count: 6, error_count: 0, avg_ms: 3 },
  ],
  edges: [
    { source: "gateway", target: "checkout", calls: 5, errors: 0, avg_ms: 1 },
    { source: "checkout", target: "payments", calls: 3, errors: 1, avg_ms: 2 },
  ],
  sampled_traces: 42,
};

describe("matches", () => {
  it("is case-insensitive and substring-based", () => {
    expect(matches("rocksky-api", "API")).toBe(true);
    expect(matches("rocksky-api", "sky")).toBe(true);
    expect(matches("rocksky-api", "nope")).toBe(false);
  });

  it("treats an empty or blank term as matching everything", () => {
    expect(matches("anything", "")).toBe(true);
    expect(matches("anything", "   ")).toBe(true);
  });
});

describe("filterGraph", () => {
  it("returns the graph untouched for an empty term", () => {
    expect(filterGraph(graph, "")).toBe(graph);
  });

  it("keeps only matching services", () => {
    const g = filterGraph(graph, "pay");
    expect(g.nodes.map((n) => n.service)).toEqual(["payments"]);
  });

  it("drops edges whose other end was filtered out", () => {
    // checkout survives but both its edges point at services that did not,
    // so drawing them would leave arrows into empty space.
    const g = filterGraph(graph, "checkout");
    expect(g.nodes.map((n) => n.service)).toEqual(["checkout"]);
    expect(g.edges).toEqual([]);
  });

  it("keeps an edge when both endpoints survive", () => {
    const g = filterGraph(graph, "e"); // gateway, checkout, payments all match
    expect(g.nodes).toHaveLength(3);
    expect(g.edges).toHaveLength(2);
  });

  it("carries sampled_traces through, since it describes the source data", () => {
    expect(filterGraph(graph, "pay").sampled_traces).toBe(42);
  });

  it("can filter everything out without producing dangling edges", () => {
    const g = filterGraph(graph, "nothing-matches");
    expect(g.nodes).toEqual([]);
    expect(g.edges).toEqual([]);
  });
});

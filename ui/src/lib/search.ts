import type { ServiceGraph } from "./api";

/** Case-insensitive substring match, with an empty term matching all. */
export function matches(haystack: string, term: string): boolean {
  const t = term.trim().toLowerCase();
  return t === "" || haystack.toLowerCase().includes(t);
}

/** Narrow a dependency graph to the services matching `term`.
 *
 * Edges survive only when both endpoints do — a half-connected edge would
 * point at a node that is no longer drawn, which renders as an arrow into
 * empty space. `sampled_traces` is carried through untouched: it describes
 * how the graph was built, not what is being shown. */
export function filterGraph(graph: ServiceGraph, term: string): ServiceGraph {
  if (term.trim() === "") return graph;
  const nodes = graph.nodes.filter((n) => matches(n.service, term));
  const kept = new Set(nodes.map((n) => n.service));
  return {
    ...graph,
    nodes,
    edges: graph.edges.filter((e) => kept.has(e.source) && kept.has(e.target)),
  };
}

import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { http, HttpResponse } from "msw";
import { MetricsView } from "./MetricsView";
import { ServicesView } from "./ServicesView";
import { server } from "../test/server";
import { renderApp } from "../test/utils";

const METRICS = [
  { name: "http.server.duration", description: "", unit: "ms", metric_type: "histogram", services: ["gateway"] },
  { name: "http.server.requests", description: "", unit: "1", metric_type: "sum", services: ["gateway"] },
  { name: "go.memory.used", description: "", unit: "By", metric_type: "gauge", services: ["api"] },
];

const STATS = [
  { service: "gateway", span_count: 10, request_count: 5, error_count: 0, error_rate: 0, rate_per_sec: 1, p50_ms: 1, p95_ms: 2, p99_ms: 3 },
  { service: "payments", span_count: 8, request_count: 4, error_count: 1, error_rate: 0.1, rate_per_sec: 2, p50_ms: 4, p95_ms: 5, p99_ms: 6 },
];

const GRAPH = {
  nodes: [
    { service: "gateway", span_count: 10, error_count: 0, avg_ms: 1 },
    { service: "payments", span_count: 8, error_count: 1, avg_ms: 2 },
  ],
  edges: [{ source: "gateway", target: "payments", calls: 3, errors: 0, avg_ms: 2 }],
  sampled_traces: 9,
};

let apiCalls: string[] = [];

beforeEach(() => {
  apiCalls = [];
  server.use(
    http.get("/api/metrics", ({ request }) => {
      apiCalls.push(new URL(request.url).pathname);
      return HttpResponse.json(METRICS);
    }),
    http.get("/api/metrics/series", () => HttpResponse.json([])),
    http.get("/api/services/stats", ({ request }) => {
      apiCalls.push(new URL(request.url).pathname);
      return HttpResponse.json(STATS);
    }),
    http.get("/api/service-graph", () => HttpResponse.json(GRAPH)),
  );
});

describe("metrics quick filter", () => {
  it("narrows the list without asking the server again", async () => {
    renderApp(<MetricsView />);
    // The list renders each metric as a button; the chart heading repeats the
    // selected name, so scope to buttons to keep the query unambiguous.
    const entry = (name: string) => screen.queryAllByRole("button", { name: new RegExp(name) });
    await waitFor(() => expect(entry("go.memory.used")).toHaveLength(1));

    apiCalls = [];
    await userEvent.type(screen.getByLabelText("Filter metrics"), "http");

    await waitFor(() => expect(entry("go.memory.used")).toHaveLength(0));
    expect(entry("http.server.duration")).toHaveLength(1);
    expect(entry("http.server.requests")).toHaveLength(1);
    // Purely client-side: the list is already loaded.
    expect(apiCalls.filter((p) => p === "/api/metrics")).toEqual([]);
  });

  it("says so when nothing matches, and clears back", async () => {
    renderApp(<MetricsView />);
    await screen.findByRole("button", { name: /go.memory.used/ });

    const box = screen.getByLabelText("Filter metrics");
    await userEvent.type(box, "zzzz");
    expect(await screen.findByText(/no metric matches/)).toBeInTheDocument();

    await userEvent.click(screen.getByLabelText("Clear filter metrics"));
    expect(
      await screen.findByRole("button", { name: /go.memory.used/ }),
    ).toBeInTheDocument();
  });
});

describe("services quick filter", () => {
  it("narrows the table and the map together", async () => {
    const { container } = renderApp(<ServicesView />);
    // Every service name appears twice — once in the table, once as a map
    // label — so each is checked in its own place.
    const rows = async () => within(await screen.findByRole("table")).queryAllByRole("row");
    const mapLabels = () =>
      [...container.querySelectorAll("svg text")].map((t) => t.textContent);

    await waitFor(async () => expect(await rows()).toHaveLength(3)); // header + 2
    expect(mapLabels()).toEqual(expect.arrayContaining(["gateway", "payments"]));

    apiCalls = [];
    await userEvent.type(screen.getByLabelText("Filter services"), "pay");

    await waitFor(async () => expect(await rows()).toHaveLength(2)); // header + 1
    expect(within(await screen.findByRole("table")).getByText("payments")).toBeInTheDocument();
    expect(mapLabels()).not.toContain("gateway");
    expect(apiCalls.filter((p) => p === "/api/services/stats")).toEqual([]);
  });

  it("reports when the filter matches no service", async () => {
    renderApp(<ServicesView />);
    await screen.findByRole("table");

    await userEvent.type(screen.getByLabelText("Filter services"), "zzzz");

    expect(await screen.findByText(/no service matches/)).toBeInTheDocument();
  });
});

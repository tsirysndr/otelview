import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import {
  logFixtures,
  seriesFixtures,
  spanFixtures,
  traceSummaryFixtures,
} from "./fixtures";

export const handlers = [
  http.get("/api/services", () => HttpResponse.json(["db", "frontend", "payments"])),
  http.get("/api/operations", () => HttpResponse.json(["GET /checkout", "SELECT orders"])),
  http.get("/api/traces", () => HttpResponse.json(traceSummaryFixtures)),
  http.get("/api/traces/:id", ({ params }) =>
    params.id === spanFixtures[0].trace_id
      ? HttpResponse.json(spanFixtures)
      : new HttpResponse("not found", { status: 404 }),
  ),
  http.get("/api/logs", () => HttpResponse.json(logFixtures)),
  http.get("/api/metrics", () =>
    HttpResponse.json([
      {
        name: "http.requests",
        description: "request count",
        unit: "1",
        metric_type: "sum",
        services: ["frontend"],
      },
    ]),
  ),
  http.get("/api/metrics/series", () => HttpResponse.json(seriesFixtures)),
  http.get("/api/stats", () =>
    HttpResponse.json({
      spans: 2,
      logs: 1,
      metric_points: 2,
      services: 3,
      backend: "memory",
    }),
  ),
];

export const server = setupServer(...handlers);

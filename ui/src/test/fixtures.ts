import type { LogRecord, MetricSeries, SpanRecord, TraceSummary } from "../lib/api";

const NOW = 1_700_000_000_000_000_000;

export const spanFixtures: SpanRecord[] = [
  {
    trace_id: "aaaa0000000000000000000000000001",
    span_id: "0000000000000001",
    parent_span_id: "",
    name: "GET /checkout",
    service_name: "frontend",
    kind: "server",
    start_time_unix_nano: NOW,
    end_time_unix_nano: NOW + 500_000_000,
    status_code: 0,
    status_message: "",
    attributes: { "http.method": "GET", "http.route": "/checkout" },
    resource_attributes: { "service.name": "frontend" },
    events: [],
    links: [],
    scope_name: "web",
    scope_version: "1.0",
  },
  {
    trace_id: "aaaa0000000000000000000000000001",
    span_id: "0000000000000002",
    parent_span_id: "0000000000000001",
    name: "SELECT orders",
    service_name: "db",
    kind: "client",
    start_time_unix_nano: NOW + 100_000_000,
    end_time_unix_nano: NOW + 350_000_000,
    status_code: 2,
    status_message: "deadlock",
    attributes: { "db.system": "postgres" },
    resource_attributes: { "service.name": "db" },
    events: [{ name: "retry", time_unix_nano: NOW + 200_000_000, attributes: {} }],
    links: [],
    scope_name: "sql",
    scope_version: "",
  },
];

export const traceSummaryFixtures: TraceSummary[] = [
  {
    trace_id: "aaaa0000000000000000000000000001",
    root_name: "GET /checkout",
    root_service: "frontend",
    start_time_unix_nano: NOW,
    duration_nanos: 500_000_000,
    span_count: 2,
    error_count: 1,
    services: ["db", "frontend"],
  },
];

export const logFixtures: LogRecord[] = [
  {
    time_unix_nano: NOW,
    observed_time_unix_nano: NOW,
    severity_number: 17,
    severity_text: "ERROR",
    body: "payment failed: card declined",
    attributes: { "order.id": "o-123" },
    resource_attributes: { "service.name": "payments" },
    service_name: "payments",
    trace_id: "aaaa0000000000000000000000000001",
    span_id: "0000000000000002",
    scope_name: "billing",
  },
];

export const seriesFixtures: MetricSeries[] = [
  {
    service_name: "frontend",
    attributes: { route: "/checkout" },
    points: [
      { time_unix_nano: NOW, value: 12 },
      { time_unix_nano: NOW + 60_000_000_000, value: 19 },
    ],
  },
];

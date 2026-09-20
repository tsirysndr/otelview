// REST client for the otelview query API.

export interface SpanRecord {
  trace_id: string;
  span_id: string;
  parent_span_id: string;
  name: string;
  service_name: string;
  kind: string;
  start_time_unix_nano: number;
  end_time_unix_nano: number;
  status_code: number;
  status_message: string;
  attributes: Record<string, unknown>;
  resource_attributes: Record<string, unknown>;
  events: { name: string; time_unix_nano: number; attributes: Record<string, unknown> }[];
  links: { trace_id: string; span_id: string; attributes: Record<string, unknown> }[];
  scope_name: string;
  scope_version: string;
}

export interface TraceSummary {
  trace_id: string;
  root_name: string;
  root_service: string;
  start_time_unix_nano: number;
  duration_nanos: number;
  span_count: number;
  error_count: number;
  services: string[];
}

export interface LogRecord {
  time_unix_nano: number;
  observed_time_unix_nano: number;
  severity_number: number;
  severity_text: string;
  body: unknown;
  attributes: Record<string, unknown>;
  resource_attributes: Record<string, unknown>;
  service_name: string;
  trace_id: string;
  span_id: string;
  scope_name: string;
}

export interface MetricInfo {
  name: string;
  description: string;
  unit: string;
  metric_type: string;
  services: string[];
}

export interface SeriesPoint {
  time_unix_nano: number;
  value: number;
}

export interface MetricSeries {
  service_name: string;
  attributes: Record<string, unknown>;
  points: SeriesPoint[];
}

export interface ServiceStats {
  service: string;
  span_count: number;
  request_count: number;
  error_count: number;
  error_rate: number;
  rate_per_sec: number;
  p50_ms: number;
  p95_ms: number;
  p99_ms: number;
}

export interface ServiceGraph {
  nodes: { service: string; span_count: number; error_count: number; avg_ms: number }[];
  edges: { source: string; target: string; calls: number; errors: number; avg_ms: number }[];
  sampled_traces: number;
}

export interface FieldInfo {
  name: string;
  count: number;
  top_values: [string, number][];
}

export interface LogBucket {
  time_unix_nano: number;
  trace: number;
  debug: number;
  info: number;
  warn: number;
  error: number;
  fatal: number;
}

export interface StorageStats {
  spans: number;
  logs: number;
  metric_points: number;
  services: number;
  backend: string;
}

export interface TraceSearchParams {
  service?: string;
  operation?: string;
  q?: string;
  traceql?: string;
  min_duration_ms?: number;
  errors_only?: boolean;
  lookback?: string;
  start_ms?: number;
  end_ms?: number;
  limit?: number;
}

export interface LogSearchParams {
  service?: string;
  min_severity?: number;
  search?: string;
  kql?: string;
  trace_id?: string;
  lookback?: string;
  start_ms?: number;
  end_ms?: number;
  limit?: number;
}

export interface SeriesSearchParams {
  name: string;
  service?: string;
  lookback?: string;
  start_ms?: number;
  end_ms?: number;
  max_points?: number;
  func?: string;
  agg?: string;
}

let apiConfig = { baseUrl: "", token: "" };

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function setApiConfig(cfg: { baseUrl: string; token: string }) {
  let baseUrl = cfg.baseUrl.replace(/\/+$/, "");
  // Desktop app with no configured server: talk to the embedded (or already
  // running) local instance.
  if (!baseUrl && isTauri()) baseUrl = "http://127.0.0.1:4319";
  apiConfig = { baseUrl, token: cfg.token };
}

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(path: string, params?: Record<string, unknown>): Promise<T> {
  const url = new URL(apiConfig.baseUrl + path, window.location.origin);
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      if (v !== undefined && v !== null && v !== "" && v !== false) {
        url.searchParams.set(k, String(v));
      }
    }
  }
  const headers: Record<string, string> = {};
  if (apiConfig.token) headers["authorization"] = `Bearer ${apiConfig.token}`;
  const resp = await fetch(url.toString(), { headers });
  if (!resp.ok) {
    throw new ApiError(resp.status, `${resp.status}: ${await resp.text()}`);
  }
  return resp.json() as Promise<T>;
}

export const api = {
  services: () => request<string[]>("/api/services"),
  operations: (service: string) =>
    request<string[]>("/api/operations", { service }),
  traces: (p: TraceSearchParams) => request<TraceSummary[]>("/api/traces", { ...p }),
  trace: (traceId: string) => request<SpanRecord[]>(`/api/traces/${traceId}`),
  logs: (p: LogSearchParams) => request<LogRecord[]>("/api/logs", { ...p }),
  metrics: () => request<MetricInfo[]>("/api/metrics"),
  metricSeries: (p: SeriesSearchParams) =>
    request<MetricSeries[]>("/api/metrics/series", { ...p }),
  stats: () => request<StorageStats>("/api/stats"),
  serviceStats: (p: Record<string, unknown>) =>
    request<ServiceStats[]>("/api/services/stats", p),
  serviceGraph: (p: Record<string, unknown>) =>
    request<ServiceGraph>("/api/service-graph", p),
  logHistogram: (p: Record<string, unknown>) =>
    request<LogBucket[]>("/api/logs/histogram", p),
  logFields: (p: Record<string, unknown>) =>
    request<FieldInfo[]>("/api/logs/fields", p),
  traceFields: (p: Record<string, unknown>) =>
    request<FieldInfo[]>("/api/traces/fields", p),
};

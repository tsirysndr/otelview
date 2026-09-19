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
  min_duration_ms?: number;
  errors_only?: boolean;
  lookback?: string;
  limit?: number;
}

export interface LogSearchParams {
  service?: string;
  min_severity?: number;
  search?: string;
  trace_id?: string;
  lookback?: string;
  limit?: number;
}

let apiConfig = { baseUrl: "", token: "" };

export function setApiConfig(cfg: { baseUrl: string; token: string }) {
  apiConfig = { baseUrl: cfg.baseUrl.replace(/\/+$/, ""), token: cfg.token };
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
  metricSeries: (name: string, service?: string, lookback?: string) =>
    request<MetricSeries[]>("/api/metrics/series", { name, service, lookback }),
  stats: () => request<StorageStats>("/api/stats"),
};

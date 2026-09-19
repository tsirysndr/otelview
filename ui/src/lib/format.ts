// Formatting helpers for times, durations and severities.

export function fmtDuration(nanos: number): string {
  if (nanos < 1_000) return `${nanos}ns`;
  if (nanos < 1_000_000) return `${(nanos / 1_000).toFixed(1)}µs`;
  if (nanos < 1_000_000_000) return `${(nanos / 1_000_000).toFixed(1)}ms`;
  if (nanos < 60_000_000_000) return `${(nanos / 1_000_000_000).toFixed(2)}s`;
  return `${(nanos / 60_000_000_000).toFixed(1)}m`;
}

export function fmtTime(nanos: number): string {
  const d = new Date(nanos / 1_000_000);
  return d.toLocaleTimeString([], { hour12: false }) +
    "." + String(Math.floor((nanos / 1_000_000) % 1000)).padStart(3, "0");
}

export function fmtDateTime(nanos: number): string {
  const d = new Date(nanos / 1_000_000);
  return d.toLocaleString([], { hour12: false });
}

export function fmtAgo(nanos: number): string {
  const ms = Date.now() - nanos / 1_000_000;
  if (ms < 0) return "now";
  const s = ms / 1000;
  if (s < 60) return `${Math.floor(s)}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

export function fmtCount(n: number): string {
  if (n < 1_000) return String(n);
  if (n < 1_000_000) return `${(n / 1_000).toFixed(1)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

export interface SeverityInfo {
  level: string;
  /** Text/badge color classes (dark + light handled via theme tokens). */
  color: string;
  dot: string;
}

export function severityInfo(num: number, text?: string): SeverityInfo {
  const label = text && text.length > 0 ? text.toUpperCase() : undefined;
  if (num >= 21) return { level: label ?? "FATAL", color: "text-danger", dot: "#FF3864" };
  if (num >= 17) return { level: label ?? "ERROR", color: "text-danger", dot: "#FF3864" };
  if (num >= 13) return { level: label ?? "WARN", color: "text-warning", dot: "#FFD319" };
  if (num >= 9) return { level: label ?? "INFO", color: "text-secondary", dot: "#05D9E8" };
  if (num >= 5) return { level: label ?? "DEBUG", color: "text-default-500", dot: "#8B87B3" };
  if (num >= 1) return { level: label ?? "TRACE", color: "text-default-400", dot: "#5B5890" };
  return { level: label ?? "—", color: "text-default-400", dot: "#5B5890" };
}

/** Render a log body (string or structured) as a single line. */
export function bodyPreview(body: unknown): string {
  if (body == null) return "";
  if (typeof body === "string") return body;
  return JSON.stringify(body);
}

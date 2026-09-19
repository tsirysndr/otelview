import { atom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import type { LogRecord } from "../lib/api";
import type { Theme } from "../theme";

export type View = "traces" | "logs" | "metrics" | "settings";

export const viewAtom = atom<View>("traces");
export const themeAtom = atomWithStorage<Theme>("otelview.theme", "dark");

/** Currently opened trace (waterfall); null = trace list. */
export const openTraceIdAtom = atom<string | null>(null);
/** Span selected in the waterfall → shown in the right inspector. */
export const selectedSpanIdAtom = atom<string | null>(null);
/** Log record selected → shown in the right inspector. */
export const selectedLogAtom = atom<LogRecord | null>(null);
/** Right inspector visibility (selecting a span/log re-opens it). */
export const inspectorOpenAtom = atom<boolean>(true);
/** Left icon rail visibility (⌘B). */
export const railVisibleAtom = atom<boolean>(true);
/** Raycast-style command palette ("/" or ⌘K). */
export const paletteOpenAtom = atom<boolean>(false);
/** Keyboard shortcuts help ("?"). */
export const helpOpenAtom = atom<boolean>(false);

/** API connection settings — used by the Tauri desktop build and remote
 * deployments; empty baseUrl = same origin. */
export interface ApiSettings {
  baseUrl: string;
  token: string;
}
export const apiSettingsAtom = atomWithStorage<ApiSettings>("otelview.api", {
  baseUrl: "",
  token: "",
});

/** Shared filters. */
export const lookbackAtom = atomWithStorage<string>("otelview.lookback", "1h");
export const liveAtom = atomWithStorage<boolean>("otelview.live", true);

/** Trace search filters (jotai-global so they survive view switches). */
export interface TraceFilters {
  service: string;
  operation: string;
  q: string;
  minDurationMs: string;
  errorsOnly: boolean;
  limit: number;
}
export const traceFiltersAtom = atom<TraceFilters>({
  service: "",
  operation: "",
  q: "",
  minDurationMs: "",
  errorsOnly: false,
  limit: 50,
});

/** Log search filters. */
export interface LogFilters {
  service: string;
  minSeverity: number;
  search: string;
  limit: number;
}
export const logFiltersAtom = atom<LogFilters>({
  service: "",
  minSeverity: 0,
  search: "",
  limit: 300,
});

/** Metrics explorer selection. */
export const selectedMetricAtom = atom<string | null>(null);
export const metricServiceAtom = atom<string>("");

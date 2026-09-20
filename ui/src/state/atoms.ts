import { atom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { atomFamily } from "jotai-family";
import type { LogRecord } from "../lib/api";
import {
  activeProfile,
  defaultSettings,
  normalizeSettings,
  type ApiSettings,
} from "../lib/profiles";
import { normalizeSaved, type SavedQuery } from "../lib/savedQueries";
import type { Theme } from "../theme";

export type { ApiSettings, ServerProfile } from "../lib/profiles";
export type { QueryKind, SavedQuery } from "../lib/savedQueries";

export type View = "traces" | "logs" | "metrics" | "services" | "settings";

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

/** API connection settings: the saved server profiles and which one is
 * live. Used by the Tauri desktop build and remote deployments; a profile
 * with an empty baseUrl means "same origin". */
const apiSettingsRawAtom = atomWithStorage<unknown>("otelview.api", defaultSettings());

/** Reads normalize whatever is in storage — including the old single-server
 * shape — so no consumer ever sees a half-valid value. */
export const apiSettingsAtom = atom(
  (get) => normalizeSettings(get(apiSettingsRawAtom)),
  (_get, set, next: ApiSettings) => set(apiSettingsRawAtom, next),
);

/** Named queries the user chose to keep. Reads normalize whatever is in
 * storage so a malformed entry can never break the filter bar. */
const savedQueriesRawAtom = atomWithStorage<unknown>("otelview.saved-queries", []);
export const savedQueriesAtom = atom(
  (get) => normalizeSaved(get(savedQueriesRawAtom)),
  (_get, set, next: SavedQuery[]) => set(savedQueriesRawAtom, next),
);

/** The server every API call currently goes to. */
export const activeProfileAtom = atom((get) => activeProfile(get(apiSettingsAtom)));

/** Shared filters. */
export const lookbackAtom = atomWithStorage<string>("otelview.lookback", "1h");
/** Absolute time range (unix millis); overrides the lookback when set. */
export const customRangeAtom = atom<{ from: number; to: number } | null>(null);
export const liveAtom = atomWithStorage<boolean>("otelview.live", true);

/** Trace search filters (jotai-global so they survive view switches). */
export type TraceQueryMode = "attributes" | "traceql";

export interface TraceFilters {
  service: string;
  operation: string;
  q: string;
  /** TraceQL source, used when `mode` is "traceql". */
  traceql: string;
  /** Which of `q` / `traceql` the filter bar is editing and sending. */
  mode: TraceQueryMode;
  minDurationMs: string;
  errorsOnly: boolean;
  limit: number;
}
export const traceFiltersAtom = atom<TraceFilters>({
  service: "",
  operation: "",
  q: "",
  traceql: "",
  mode: "attributes",
  minDurationMs: "",
  errorsOnly: false,
  limit: 50,
});

/** Log search filters. */
export interface LogFilters {
  service: string;
  minSeverity: number;
  search: string;
  /** Set when following a trace: narrows logs to that trace (and span). */
  traceId: string;
  spanId: string;
  limit: number;
}
export const logFiltersAtom = atom<LogFilters>({
  service: "",
  minSeverity: 0,
  search: "",
  traceId: "",
  spanId: "",
  limit: 300,
});

/** Recent-query history per search input, persisted in localStorage and
 * shared across the app. Keyed by the input's `historyKey` (e.g.
 * "traces.attributes", "logs.kql") — same storage key format the old
 * hand-rolled localStorage module used, so existing history keeps working. */
export const queryHistoryFamily = atomFamily((key: string) =>
  atomWithStorage<string[]>(`otelview.history.${key}`, []),
);

/** Metrics explorer selection. */
export const selectedMetricAtom = atom<string | null>(null);
export const metricServiceAtom = atom<string>("");
/** Metric query function (raw | rate | increase). */
export const metricFuncAtom = atom<string>("raw");
/** Cross-series aggregation (none | sum | avg | min | max). */
export const metricAggAtom = atom<string>("none");
/** Logs fields sidebar visibility. */
export const logFieldsOpenAtom = atom<boolean>(true);

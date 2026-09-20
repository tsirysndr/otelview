import { useSetAtom } from "jotai";
import {
  customRangeAtom,
  inspectorOpenAtom,
  logFiltersAtom,
  metricServiceAtom,
  openTraceIdAtom,
  selectedLogAtom,
  selectedMetricAtom,
  selectedSpanIdAtom,
  traceFiltersAtom,
  viewAtom,
} from "../state/atoms";

/** How much context to show around a correlated timestamp when the current
 * window would not contain it. Wide enough to catch neighbouring activity,
 * narrow enough that the destination is not a wall of noise. */
const CONTEXT_MS = 15 * 60 * 1000;

/** Jumping between signals.
 *
 * The join keys available are narrow: `trace_id` links traces and logs,
 * `service` links all three, and metrics have no exemplars, so metrics can
 * only ever be correlated by service plus a time window.
 *
 * Every jump also pins the time range around the thing being followed.
 * Without that, following a log from three days ago lands on a view still
 * showing the last 15 minutes — empty, and looking like a bug rather than a
 * window mismatch. */
export function useCorrelate() {
  const setView = useSetAtom(viewAtom);
  const setOpenTrace = useSetAtom(openTraceIdAtom);
  const setSelectedSpan = useSetAtom(selectedSpanIdAtom);
  const setSelectedLog = useSetAtom(selectedLogAtom);
  const setLogFilters = useSetAtom(logFiltersAtom);
  const setTraceFilters = useSetAtom(traceFiltersAtom);
  const setCustomRange = useSetAtom(customRangeAtom);
  const setSelectedMetric = useSetAtom(selectedMetricAtom);
  const setMetricService = useSetAtom(metricServiceAtom);
  const setInspectorOpen = useSetAtom(inspectorOpenAtom);

  /** Centre the window on `unixNano`, so the destination is never empty
   * purely because of the range that happened to be selected. */
  const focusTime = (unixNano?: number) => {
    if (!unixNano) return;
    const ms = unixNano / 1_000_000;
    setCustomRange({ from: ms - CONTEXT_MS, to: ms + CONTEXT_MS });
  };

  return {
    /** Logs belonging to one trace (optionally one span within it). */
    traceToLogs: (traceId: string, opts?: { spanId?: string; atUnixNano?: number }) => {
      focusTime(opts?.atUnixNano);
      setSelectedLog(null);
      setLogFilters((f) => ({
        ...f,
        traceId,
        spanId: opts?.spanId ?? "",
        // A trace is already a narrow slice; carrying a text query over from
        // the previous search would only hide rows.
        search: "",
        minSeverity: 0,
        service: "",
      }));
      setView("logs");
    },

    /** The trace a log belongs to, landing on its span when known. */
    logToTrace: (traceId: string, opts?: { spanId?: string; atUnixNano?: number }) => {
      focusTime(opts?.atUnixNano);
      setSelectedSpan(opts?.spanId ?? null);
      setOpenTrace(traceId);
      setView("traces");
      if (opts?.spanId) setInspectorOpen(true);
    },

    /** Traces for a service, optionally around a moment in time. */
    serviceToTraces: (service: string, opts?: { atUnixNano?: number }) => {
      focusTime(opts?.atUnixNano);
      setOpenTrace(null);
      setSelectedSpan(null);
      setTraceFilters((f) => ({ ...f, service, operation: "" }));
      setView("traces");
    },

    /** Logs for a service, optionally around a moment in time. */
    serviceToLogs: (service: string, opts?: { atUnixNano?: number }) => {
      focusTime(opts?.atUnixNano);
      setSelectedLog(null);
      setLogFilters((f) => ({
        ...f,
        service,
        traceId: "",
        spanId: "",
        search: "",
      }));
      setView("logs");
    },

    /** Metrics for a service. There are no exemplars, so this is as precise
     * as metric correlation gets: the service, in the surrounding window. */
    serviceToMetrics: (service: string, opts?: { atUnixNano?: number }) => {
      focusTime(opts?.atUnixNano);
      setMetricService(service);
      // Leave the chosen metric alone if there is one; MetricsView falls back
      // to the first available.
      setSelectedMetric((m) => m);
      setView("metrics");
    },
  };
}

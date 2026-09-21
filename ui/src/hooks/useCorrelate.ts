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
 * The join keys are narrow: `trace_id` links traces and logs, `service`
 * links a service row to either, and an exemplar — when a producer emits one
 * — links a specific metric back to the exact span that produced it. Absent
 * an exemplar, metrics are reachable from a service but never presented as
 * correlated with a span, because they would not be.
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
   * purely because of the range that happened to be selected.
   *
   * Nanoseconds do not divide cleanly into milliseconds, and the API takes
   * `start_ms`/`end_ms` as integers — an unrounded value is serialized as
   * "1758397850123.4568" and rejected outright. */
  const focusTime = (unixNano?: number) => {
    if (!unixNano) return;
    const ms = Math.floor(unixNano / 1_000_000);
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
        // the previous search would only hide rows. Both languages get
        // cleared, not just the active one — the mode survives the jump.
        search: "",
        lucene: "",
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
        lucene: "",
      }));
      setView("logs");
    },

    /** The specific metric an exemplar came from — a real metrics<->trace
     * link, so it selects that metric rather than just the service. */
    exemplarToMetric: (
      metricName: string,
      opts: { service?: string; atUnixNano?: number },
    ) => {
      focusTime(opts.atUnixNano);
      setSelectedMetric(metricName);
      if (opts.service) setMetricService(opts.service);
      setView("metrics");
    },

    /** Metrics for a service — navigation from the services table, not a
     * correlation. Picking a service genuinely means that service, so the
     * claim is honest here in a way it would not be from a single span. */
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

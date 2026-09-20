import { useQuery } from "@tanstack/react-query";
import { api } from "../lib/api";

/** A trace id that is absent or all zeros carries no trace. */
export function hasTraceId(traceId?: string): boolean {
  return !!traceId && !/^0*$/.test(traceId);
}

/** Which correlated signals actually exist for a span or log, so the UI can
 * offer only the jumps that lead somewhere.
 *
 * Both lookups are cached by react-query on keys coarser than the selection
 * — the trace's logs, and the global metric list — so clicking through spans
 * inside one trace costs a single request, not one per span. */
export function useSignalLinks({
  traceId,
  spanId,
  service,
}: {
  traceId?: string;
  spanId?: string;
  service?: string;
}) {
  const traced = hasTraceId(traceId);

  const { data: traceLogs, isLoading: logsLoading } = useQuery({
    queryKey: ["correlated-logs", traceId],
    // A trace's logs are few; fetching them is cheaper than the count
    // endpoint that does not exist, and the result answers the span check
    // too.
    queryFn: () => api.logs({ trace_id: traceId!, limit: 500, lookback: "all" }),
    enabled: traced,
    staleTime: 30_000,
  });

  const { data: metrics, isLoading: metricsLoading } = useQuery({
    queryKey: ["metrics"],
    queryFn: api.metrics,
    staleTime: 60_000,
  });

  return {
    /** The trace has logs somewhere in it. */
    hasTraceLogs: (traceLogs?.length ?? 0) > 0,
    /** This particular span emitted logs. */
    hasSpanLogs: !!spanId && !!traceLogs?.some((l) => l.span_id === spanId),
    /** Some metric reports this service. */
    hasServiceMetrics:
      !!service && !!metrics?.some((m) => m.services?.includes(service)),
    /** Don't flash a link in and then out while the answer is unknown. */
    loading: (traced && logsLoading) || metricsLoading,
  };
}

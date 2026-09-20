import { useQuery } from "@tanstack/react-query";
import { api } from "../lib/api";

/** A trace id that is absent or all zeros carries no trace. */
export function hasTraceId(traceId?: string): boolean {
  return !!traceId && !/^0*$/.test(traceId);
}

/** Which correlated signals actually exist for a span or log, so the UI can
 * offer only the jumps that lead somewhere.
 *
 * Metrics are only offered when an exemplar actually points at the span or
 * trace. Without one, a metric relates to a service over a window and not to
 * this span, so presenting it as correlated would claim a link the data does
 * not have. Service-level metrics stay reachable from the services table,
 * where that claim is honest.
 *
 * Both lookups are cached on the trace id rather than the selection, so
 * clicking through the spans of one trace costs one request each. */
export function useSignalLinks({
  traceId,
  spanId,
}: {
  traceId?: string;
  spanId?: string;
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

  const { data: exemplars, isLoading: exemplarsLoading } = useQuery({
    queryKey: ["metric-exemplars", traceId],
    queryFn: () => api.metricExemplars({ trace_id: traceId!, limit: 200 }),
    enabled: traced,
    staleTime: 30_000,
  });

  const spanExemplars = spanId
    ? (exemplars ?? []).filter((e) => e.exemplar.span_id === spanId)
    : [];

  return {
    /** Metrics whose exemplars point at this span, or at the trace. */
    spanExemplars,
    traceExemplars: exemplars ?? [],
    /** The trace has logs somewhere in it. */
    hasTraceLogs: (traceLogs?.length ?? 0) > 0,
    /** This particular span emitted logs. */
    hasSpanLogs: !!spanId && !!traceLogs?.some((l) => l.span_id === spanId),
    /** Don't flash a link in and then out while the answer is unknown. */
    loading: traced && (logsLoading || exemplarsLoading),
  };
}

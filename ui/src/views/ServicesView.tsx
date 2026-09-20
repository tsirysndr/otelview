import { useQuery } from "@tanstack/react-query";
import { IconAlignLeft, IconChartLine, IconTopologyStar3 } from "@tabler/icons-react";
import { useAtomValue, useSetAtom } from "jotai";
import { liveAtom, openTraceIdAtom, traceFiltersAtom, viewAtom } from "../state/atoms";
import { useTimeParams } from "../hooks/useTimeParams";
import { useCorrelate } from "../hooks/useCorrelate";
import { api } from "../lib/api";
import { serviceNeon } from "../lib/colors";
import { fmtCount, fmtDuration } from "../lib/format";
import { EmptyState } from "../components/EmptyState";
import { ServiceMap } from "../components/ServiceMap";
import { Skeleton, SkeletonTable } from "../components/Skeleton";

/** APM-style overview: dependency map + per-service RED metrics
 * (rate, errors, duration percentiles) derived from recent traces. */
export function ServicesView() {
  const timeParams = useTimeParams();
  const live = useAtomValue(liveAtom);
  const setView = useSetAtom(viewAtom);
  const setOpenTrace = useSetAtom(openTraceIdAtom);
  const setTraceFilters = useSetAtom(traceFiltersAtom);
  const correlate = useCorrelate();

  const { data: graph, isLoading: graphLoading } = useQuery({
    queryKey: ["service-graph", timeParams],
    queryFn: () => api.serviceGraph({ ...timeParams }),
    refetchInterval: live ? 5_000 : false,
  });
  const { data: stats = [], isLoading: statsLoading } = useQuery({
    queryKey: ["service-stats", timeParams],
    queryFn: () => api.serviceStats({ ...timeParams }),
    refetchInterval: live ? 5_000 : false,
  });

  const loading = graphLoading || statsLoading;
  if (loading) {
    return (
      <div className="flex h-full min-h-0 flex-col overflow-y-auto">
        <div className="shrink-0 border-b border-divider p-3">
          <Skeleton className="mb-2 h-3 w-24" />
          <Skeleton className="h-40 w-full rounded-md" />
        </div>
        <div className="p-3">
          <Skeleton className="mb-2 h-3 w-24" />
          <SkeletonTable rows={8} cols={5} label="loading service metrics" />
        </div>
      </div>
    );
  }
  if (stats.length === 0) {
    return (
      <EmptyState
        icon={<IconTopologyStar3 size={44} stroke={1.2} />}
        title="no services yet"
        hint="Service stats and the dependency map are derived from received traces."
      />
    );
  }

  const openService = (service: string) => {
    setOpenTrace(null);
    setTraceFilters((f) => ({ ...f, service, operation: "" }));
    setView("traces");
  };

  const maxP95 = Math.max(...stats.map((s) => s.p95_ms), 1);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto">
      {graph && graph.nodes.length > 0 && (
        <div className="shrink-0 border-b border-divider">
          <div className="flex items-baseline justify-between px-3 pt-2">
            <h2 className="text-[11px] uppercase tracking-wider text-default-500">
              service map
            </h2>
            <span className="text-[10px] text-default-400">
              from {graph.sampled_traces} sampled traces
            </span>
          </div>
          <ServiceMap graph={graph} />
        </div>
      )}

      <div className="p-3">
        <h2 className="mb-2 text-[11px] uppercase tracking-wider text-default-500">
          RED metrics
        </h2>
        <table className="w-full border-collapse text-xs">
          <thead>
            <tr className="border-b border-divider text-left text-[10px] uppercase tracking-wide text-default-500">
              <th className="py-1.5 pr-2 font-medium">service</th>
              <th className="py-1.5 pr-2 text-right font-medium">req/s</th>
              <th className="py-1.5 pr-2 text-right font-medium">errors</th>
              <th className="py-1.5 pr-2 text-right font-medium">p50</th>
              <th className="py-1.5 pr-2 text-right font-medium">p95</th>
              <th className="py-1.5 pr-2 text-right font-medium">p99</th>
              <th className="py-1.5 pr-2 text-right font-medium">spans</th>
              <th className="py-1.5 pr-2 font-medium">latency</th>
              <th className="py-1.5 font-medium">signals</th>
            </tr>
          </thead>
          <tbody>
            {stats.map((s) => {
              const { color, glow } = serviceNeon(s.service);
              const errPct = s.error_rate * 100;
              return (
                <tr
                  key={s.service}
                  className="cursor-pointer border-b border-divider/50 transition-colors hover:bg-content2"
                  onClick={() => openService(s.service)}
                  title={`open ${s.service} traces`}
                >
                  <td className="py-1.5 pr-2">
                    <span className="flex items-center gap-2">
                      <span
                        className="neon-glow inline-block h-2.5 w-2.5 rounded-full"
                        style={{ background: color, "--glow": glow } as React.CSSProperties}
                      />
                      {s.service}
                    </span>
                  </td>
                  <td className="py-1.5 pr-2 text-right tabular-nums">
                    {s.rate_per_sec.toFixed(2)}
                  </td>
                  <td
                    className={`py-1.5 pr-2 text-right tabular-nums ${
                      errPct >= 5 ? "text-danger" : errPct > 0 ? "text-warning" : "text-default-500"
                    }`}
                  >
                    {errPct.toFixed(1)}%
                  </td>
                  <td className="py-1.5 pr-2 text-right tabular-nums text-default-600">
                    {fmtDuration(s.p50_ms * 1e6)}
                  </td>
                  <td className="py-1.5 pr-2 text-right tabular-nums text-neon-cyan">
                    {fmtDuration(s.p95_ms * 1e6)}
                  </td>
                  <td className="py-1.5 pr-2 text-right tabular-nums text-default-600">
                    {fmtDuration(s.p99_ms * 1e6)}
                  </td>
                  <td className="py-1.5 pr-2 text-right tabular-nums text-default-500">
                    {fmtCount(s.span_count)}
                  </td>
                  <td className="py-1.5 pr-2">
                    <div className="h-1.5 w-full max-w-40 overflow-hidden rounded bg-content2">
                      <div
                        className="h-full rounded"
                        style={{
                          width: `${Math.max((s.p95_ms / maxP95) * 100, 2)}%`,
                          background: errPct >= 5 ? "#FF3864" : color,
                        }}
                      />
                    </div>
                  </td>
                  {/* The row already opens traces; these reach the other two
                      signals for the same service without leaving the table. */}
                  <td className="py-1.5" onClick={(e) => e.stopPropagation()}>
                    <div className="flex items-center gap-1">
                      <button
                        onClick={() => correlate.serviceToLogs(s.service)}
                        aria-label={`Logs for ${s.service}`}
                        title={`logs for ${s.service}`}
                        className="rounded p-1 text-default-400 transition-colors hover:text-neon-cyan"
                      >
                        <IconAlignLeft size={14} />
                      </button>
                      <button
                        onClick={() => correlate.serviceToMetrics(s.service)}
                        aria-label={`Metrics for ${s.service}`}
                        title={`metrics for ${s.service}`}
                        className="rounded p-1 text-default-400 transition-colors hover:text-neon-cyan"
                      >
                        <IconChartLine size={14} />
                      </button>
                    </div>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <p className="mt-2 text-[10px] text-default-400">
          computed from up to 250 recent traces in the selected window · requests =
          server/root spans · click a row for its traces, or the icons for its logs and metrics
        </p>
      </div>
    </div>
  );
}

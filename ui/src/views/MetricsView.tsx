import { Chip } from "@heroui/react";
import { useQuery } from "@tanstack/react-query";
import { useAtom, useAtomValue } from "jotai";
import {
  liveAtom,
  metricServiceAtom,
  selectedMetricAtom,
} from "../state/atoms";
import { useTimeParams } from "../hooks/useTimeParams";
import { api } from "../lib/api";
import { LineChart, type ChartSeries } from "../components/LineChart";
import { Field } from "../components/Field";
import { FilterSelect } from "../components/FilterSelect";

const TYPE_COLORS: Record<string, "secondary" | "primary" | "warning" | "success" | "default"> = {
  gauge: "secondary",
  sum: "primary",
  histogram: "warning",
  exponential_histogram: "warning",
  summary: "success",
};

function seriesLabel(service: string, attrs: Record<string, unknown>): string {
  const parts = Object.entries(attrs ?? {}).map(([k, v]) => `${k}=${String(v)}`);
  return parts.length > 0 ? `${service} · ${parts.join(", ")}` : service;
}

export function MetricsView() {
  const [selected, setSelected] = useAtom(selectedMetricAtom);
  const [service, setService] = useAtom(metricServiceAtom);
  const timeParams = useTimeParams();
  const live = useAtomValue(liveAtom);

  const { data: metrics = [], isLoading } = useQuery({
    queryKey: ["metrics"],
    queryFn: api.metrics,
    refetchInterval: live ? 5_000 : false,
  });
  const { data: services = [] } = useQuery({ queryKey: ["services"], queryFn: api.services });

  const active = selected ?? metrics[0]?.name ?? null;
  const activeInfo = metrics.find((m) => m.name === active);

  const { data: series = [] } = useQuery({
    queryKey: ["series", active, service, timeParams],
    queryFn: () =>
      api.metricSeries({ name: active!, service: service || undefined, ...timeParams }),
    enabled: !!active,
    refetchInterval: live ? 5_000 : false,
  });

  // Fixed-order color assignment; >6 series fold into "Other" gray but stay
  // individually plotted and labeled.
  const chartSeries: ChartSeries[] = series.map((s) => ({
    label: seriesLabel(s.service_name, s.attributes),
    points: s.points.map((p) => ({ t: p.time_unix_nano, v: p.value })),
  }));

  return (
    <div className="flex h-full min-h-0">
      {/* metric list */}
      <div className="flex w-72 shrink-0 flex-col border-r border-divider bg-content1">
        <div className="flex h-9 shrink-0 items-center border-b border-divider px-3 text-[11px] uppercase tracking-wider text-default-500">
          metrics ({metrics.length})
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {isLoading && <p className="p-3 text-xs text-default-500">loading…</p>}
          {!isLoading && metrics.length === 0 && (
            <p className="p-3 text-xs text-default-500">
              no metrics — send OTLP to :4318/v1/metrics
            </p>
          )}
          {metrics.map((m) => (
            <button
              key={m.name}
              onClick={() => setSelected(m.name)}
              className={`block w-full border-b border-divider/40 px-3 py-2 text-left transition-colors hover:bg-content2 ${
                active === m.name ? "bg-content2 shadow-[inset_2px_0_0_#05D9E8]" : ""
              }`}
            >
              <div className="truncate text-xs font-medium">{m.name}</div>
              <div className="mt-0.5 flex items-center gap-1.5">
                <Chip
                  size="sm"
                  variant="flat"
                  color={TYPE_COLORS[m.metric_type] ?? "default"}
                  className="h-4 text-[9px]"
                >
                  {m.metric_type}
                </Chip>
                {m.unit && <span className="text-[10px] text-default-500">{m.unit}</span>}
              </div>
            </button>
          ))}
        </div>
      </div>

      {/* chart panel */}
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        {activeInfo ? (
          <>
            <div className="flex shrink-0 flex-wrap items-center gap-3 border-b border-divider p-3">
              <div className="min-w-0">
                <h2 className="truncate text-sm font-semibold">{activeInfo.name}</h2>
                {activeInfo.description && (
                  <p className="truncate text-xs text-default-500">{activeInfo.description}</p>
                )}
              </div>
              <Field label="service" className="ml-auto w-44">
                <FilterSelect
                  ariaLabel="Service"
                  value={service}
                  onChange={setService}
                  options={[
                    { value: "", label: "all services" },
                    ...services.map((s) => ({ value: s, label: s })),
                  ]}
                />
              </Field>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto p-3">
              {chartSeries.length === 0 ? (
                <p className="text-sm text-default-500">no data points in this window</p>
              ) : (
                <LineChart series={chartSeries} unit={activeInfo.unit} height={320} />
              )}
              {activeInfo.metric_type === "histogram" && (
                <p className="mt-2 text-[11px] text-default-500">
                  histogram — the line tracks each point's sum; count and buckets are in
                  the raw payload
                </p>
              )}
            </div>
          </>
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-default-500">
            select a metric
          </div>
        )}
      </div>
    </div>
  );
}

import { Button, Chip, Input, Select, SelectItem, Switch } from "@heroui/react";
import { IconArrowLeft, IconSearch } from "@tabler/icons-react";
import { useQuery } from "@tanstack/react-query";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  liveAtom,
  lookbackAtom,
  openTraceIdAtom,
  selectedSpanIdAtom,
  traceFiltersAtom,
} from "../state/atoms";
import { fieldProps, plainTextField } from "../lib/inputProps";
import { api } from "../lib/api";
import { fmtAgo, fmtDuration } from "../lib/format";
import { serviceNeon } from "../lib/colors";
import { Field } from "../components/Field";
import { ScatterPlot } from "../components/ScatterPlot";
import { ServiceChip } from "../components/ServiceChip";
import { Waterfall } from "../components/Waterfall";

function TraceList() {
  const [filters, setFilters] = useAtom(traceFiltersAtom);
  const lookback = useAtomValue(lookbackAtom);
  const live = useAtomValue(liveAtom);
  const setOpenTrace = useSetAtom(openTraceIdAtom);
  const setSelectedSpan = useSetAtom(selectedSpanIdAtom);

  const { data: services = [] } = useQuery({ queryKey: ["services"], queryFn: api.services });
  const { data: operations = [] } = useQuery({
    queryKey: ["operations", filters.service],
    queryFn: () => api.operations(filters.service),
  });

  const { data: traces = [], isLoading } = useQuery({
    queryKey: ["traces", filters, lookback],
    queryFn: () =>
      api.traces({
        service: filters.service || undefined,
        operation: filters.operation || undefined,
        q: filters.q || undefined,
        min_duration_ms: filters.minDurationMs ? Number(filters.minDurationMs) : undefined,
        errors_only: filters.errorsOnly || undefined,
        lookback,
        limit: filters.limit,
      }),
    refetchInterval: live ? 3_000 : false,
  });

  const maxDuration = Math.max(...traces.map((t) => t.duration_nanos), 1);

  const open = (traceId: string) => {
    setSelectedSpan(null);
    setOpenTrace(traceId);
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* filter bar */}
      <div className="flex shrink-0 flex-wrap items-end gap-2 border-b border-divider bg-content1 p-2">
        <Field label="service" className="w-44">
        <Select
          {...fieldProps}
          aria-label="Service"
          placeholder="all services"
          selectedKeys={filters.service ? [filters.service] : []}
          onSelectionChange={(keys) =>
            setFilters({ ...filters, service: (Array.from(keys)[0] as string) ?? "", operation: "" })
          }
          items={[{ key: "", label: "all services" }, ...services.map((s) => ({ key: s, label: s }))]}
        >
          {(item) => <SelectItem key={item.key}>{item.label}</SelectItem>}
        </Select>
        </Field>
        <Field label="operation" className="w-52">
        <Select
          {...fieldProps}
          aria-label="Operation"
          placeholder="all operations"
          selectedKeys={filters.operation ? [filters.operation] : []}
          onSelectionChange={(keys) =>
            setFilters({ ...filters, operation: (Array.from(keys)[0] as string) ?? "" })
          }
          items={[{ key: "", label: "all operations" }, ...operations.map((o) => ({ key: o, label: o }))]}
        >
          {(item) => <SelectItem key={item.key}>{item.label}</SelectItem>}
        </Select>
        </Field>
        <Field label="attributes" className="w-56">
        <Input
          {...plainTextField}
          {...fieldProps}
          aria-label="Attribute filter"
          placeholder="key=value or text"
          value={filters.q}
          onValueChange={(q) => setFilters({ ...filters, q })}
          startContent={<IconSearch size={14} className="text-default-400" />}
        />
        </Field>
        <Field label="min duration" className="w-32">
        <Input
          {...plainTextField}
          {...fieldProps}
          aria-label="Minimum duration in milliseconds"
          placeholder="ms"
          value={filters.minDurationMs}
          onValueChange={(minDurationMs) => setFilters({ ...filters, minDurationMs })}
        />
        </Field>
        <div className="flex items-center gap-1.5 pb-1.5">
          <Switch
            size="sm"
            isSelected={filters.errorsOnly}
            onValueChange={(errorsOnly) => setFilters({ ...filters, errorsOnly })}
            aria-label="Errors only"
          />
          <span className="text-xs text-default-500">errors only</span>
        </div>
      </div>

      {/* duration scatter (Jaeger-style) */}
      {traces.length > 1 && (
        <div className="shrink-0 border-b border-divider px-2 pt-1">
          <ScatterPlot traces={traces} onOpen={open} />
        </div>
      )}

      {/* results */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {isLoading && <p className="p-4 text-sm text-default-500">searching…</p>}
        {!isLoading && traces.length === 0 && (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-default-500">
            <p className="text-sm">no traces found</p>
            <p className="text-xs">
              send OTLP to <span className="text-neon-cyan">grpc :4317</span> or{" "}
              <span className="text-neon-cyan">http :4318/v1/traces</span>
            </p>
          </div>
        )}
        {traces.map((t) => (
          <button
            key={t.trace_id}
            onClick={() => open(t.trace_id)}
            className="block w-full border-b border-divider/60 px-3 py-2 text-left transition-colors hover:bg-content2"
          >
            <div className="flex items-baseline gap-2">
              <span
                className="h-2.5 w-1 shrink-0 self-center rounded-sm"
                style={{
                  background: serviceNeon(t.root_service).color,
                  boxShadow: serviceNeon(t.root_service).glow,
                }}
              />
              <span className="truncate text-sm font-medium">{t.root_name}</span>
              <span className="shrink-0 text-xs text-neon-cyan">
                {fmtDuration(t.duration_nanos)}
              </span>
              {t.error_count > 0 && (
                <Chip size="sm" color="danger" variant="flat" className="h-5 text-[10px]">
                  {t.error_count} err
                </Chip>
              )}
              <span className="ml-auto shrink-0 text-[11px] text-default-500">
                {fmtAgo(t.start_time_unix_nano)}
              </span>
            </div>
            <div className="mt-1 flex items-center gap-2">
              {/* relative duration bar (Datadog-style latency read) */}
              <div className="h-1 w-40 shrink-0 overflow-hidden rounded bg-content2">
                <div
                  className="h-full rounded"
                  style={{
                    width: `${Math.max((t.duration_nanos / maxDuration) * 100, 2)}%`,
                    background: t.error_count > 0 ? "#FF3864" : serviceNeon(t.root_service).color,
                    boxShadow:
                      t.error_count > 0
                        ? "0 0 6px rgba(255,56,100,0.6)"
                        : serviceNeon(t.root_service).glow,
                  }}
                />
              </div>
              <span className="text-[11px] text-default-500">{t.span_count} spans</span>
              <span className="flex flex-wrap gap-1">
                {t.services.slice(0, 5).map((s) => (
                  <ServiceChip key={s} service={s} small />
                ))}
                {t.services.length > 5 && (
                  <span className="text-[10px] text-default-500">
                    +{t.services.length - 5}
                  </span>
                )}
              </span>
              <span className="ml-auto text-[10px] text-default-400">
                {t.trace_id.slice(0, 16)}…
              </span>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}

function TraceDetail({ traceId }: { traceId: string }) {
  const setOpenTrace = useSetAtom(openTraceIdAtom);
  const setSelectedSpan = useSetAtom(selectedSpanIdAtom);
  const { data: spans, isLoading, isError } = useQuery({
    queryKey: ["trace", traceId],
    queryFn: () => api.trace(traceId),
  });

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-divider bg-content1 px-2">
        <Button
          size="sm"
          variant="light"
          startContent={<IconArrowLeft size={15} />}
          onPress={() => {
            setOpenTrace(null);
            setSelectedSpan(null);
          }}
        >
          traces
        </Button>
        <span className="truncate text-xs text-default-500">{traceId}</span>
        {spans && spans.length > 0 && (
          <span className="ml-auto truncate text-sm font-medium">
            {spans.find((s) => !s.parent_span_id)?.name ?? spans[0].name}
          </span>
        )}
      </div>
      {isLoading && <p className="p-4 text-sm text-default-500">loading trace…</p>}
      {isError && <p className="p-4 text-sm text-danger">trace not found</p>}
      {spans && spans.length > 0 && <Waterfall spans={spans} />}
    </div>
  );
}

export function TracesView() {
  const openTraceId = useAtomValue(openTraceIdAtom);
  return openTraceId ? <TraceDetail traceId={openTraceId} /> : <TraceList />;
}

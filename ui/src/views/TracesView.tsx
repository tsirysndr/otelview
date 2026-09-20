import { Button, Chip, Input, Switch } from "@heroui/react";
import { IconArrowLeft, IconRoute } from "@tabler/icons-react";
import { useQuery } from "@tanstack/react-query";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  liveAtom,
  openTraceIdAtom,
  selectedSpanIdAtom,
  traceFiltersAtom,
} from "../state/atoms";
import { fieldProps, plainTextField, switchClassNames } from "../lib/inputProps";
import { useTimeParams } from "../hooks/useTimeParams";
import { api } from "../lib/api";
import { fmtAgo, fmtDuration } from "../lib/format";
import { serviceNeon } from "../lib/colors";
import { AttrInput } from "../components/AttrInput";
import { EmptyState } from "../components/EmptyState";
import { Field } from "../components/Field";
import { FilterSelect } from "../components/FilterSelect";
import { ScatterPlot } from "../components/ScatterPlot";
import { ServiceChip } from "../components/ServiceChip";
import { SkeletonRows, SkeletonWaterfall } from "../components/Skeleton";
import { Waterfall } from "../components/Waterfall";

function TraceList() {
  const [filters, setFilters] = useAtom(traceFiltersAtom);
  const timeParams = useTimeParams();
  const live = useAtomValue(liveAtom);
  const setOpenTrace = useSetAtom(openTraceIdAtom);
  const setSelectedSpan = useSetAtom(selectedSpanIdAtom);

  const { data: services = [] } = useQuery({ queryKey: ["services"], queryFn: api.services });
  const { data: operations = [] } = useQuery({
    queryKey: ["operations", filters.service],
    queryFn: () => api.operations(filters.service),
    // With no service picked there is nothing to ask for — the operations
    // dropdown only has entries per service.
    enabled: filters.service !== "",
  });
  const { data: attrFields = [] } = useQuery({
    queryKey: ["trace-fields", filters.service, timeParams],
    queryFn: () =>
      api.traceFields({ service: filters.service || undefined, ...timeParams }),
    refetchInterval: live ? 10_000 : false,
  });

  const { data: traces = [], isLoading } = useQuery({
    queryKey: ["traces", filters, timeParams],
    queryFn: () =>
      api.traces({
        service: filters.service || undefined,
        operation: filters.operation || undefined,
        q: filters.q || undefined,
        min_duration_ms: filters.minDurationMs ? Number(filters.minDurationMs) : undefined,
        errors_only: filters.errorsOnly || undefined,
        ...timeParams,
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
          <FilterSelect
            ariaLabel="Service"
            value={filters.service}
            onChange={(service) => setFilters({ ...filters, service, operation: "" })}
            options={[
              { value: "", label: "all services" },
              ...services.map((s) => ({ value: s, label: s })),
            ]}
          />
        </Field>
        <Field label="operation" className="w-52">
          <FilterSelect
            ariaLabel="Operation"
            value={filters.operation}
            onChange={(operation) => setFilters({ ...filters, operation })}
            options={[
              { value: "", label: "all operations" },
              ...operations.map((o) => ({ value: o, label: o })),
            ]}
          />
        </Field>
        <Field label="attributes" className="min-w-64 flex-1">
          <AttrInput
            value={filters.q}
            onChange={(q) => setFilters({ ...filters, q })}
            fields={attrFields}
            placeholder="http.method=GET or any text"
          />
        </Field>
        <Field label="min duration (ms)" className="w-32">
        <Input
          {...plainTextField}
          {...fieldProps}
          aria-label="Minimum duration in milliseconds"
          placeholder="e.g. 300"
          title="Only traces with a span slower than this many milliseconds"
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
            classNames={switchClassNames}
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
        {isLoading && <SkeletonRows rows={12} label="searching traces" />}
        {!isLoading && traces.length === 0 && (
          <EmptyState
            icon={<IconRoute size={44} stroke={1.2} />}
            title="no traces yet"
            hint="Nothing matches the current filters and time range — or no spans have been received."
          />
        )}
        {traces.map((t) => (
          <button
            key={t.trace_id}
            onClick={() => open(t.trace_id)}
            className="block w-full border-b border-divider/60 px-3 py-2 text-left transition-colors hover:bg-content2"
          >
            <div className="flex items-baseline gap-2">
              <span
                className="neon-glow h-2.5 w-1 shrink-0 self-center rounded-sm"
                style={
                  {
                    background: serviceNeon(t.root_service).color,
                    "--glow": serviceNeon(t.root_service).glow,
                  } as React.CSSProperties
                }
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
                  className="neon-glow h-full rounded"
                  style={
                    {
                      width: `${Math.max((t.duration_nanos / maxDuration) * 100, 2)}%`,
                      background:
                        t.error_count > 0 ? "#FF3864" : serviceNeon(t.root_service).color,
                      "--glow":
                        t.error_count > 0
                          ? "0 0 6px rgba(255,56,100,0.6)"
                          : serviceNeon(t.root_service).glow,
                    } as React.CSSProperties
                  }
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
      {isLoading && <SkeletonWaterfall />}
      {isError && <p className="p-4 text-sm text-danger">trace not found</p>}
      {spans && spans.length > 0 && <Waterfall spans={spans} />}
    </div>
  );
}

export function TracesView() {
  const openTraceId = useAtomValue(openTraceIdAtom);
  return openTraceId ? <TraceDetail traceId={openTraceId} /> : <TraceList />;
}

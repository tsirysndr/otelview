import { Input } from "@heroui/react";
import { IconAlignLeft, IconSearch } from "@tabler/icons-react";
import { useQuery } from "@tanstack/react-query";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  inspectorOpenAtom,
  liveAtom,
  logFiltersAtom,
  selectedLogAtom,
} from "../state/atoms";
import { useTimeParams } from "../hooks/useTimeParams";
import { fieldProps, plainTextField } from "../lib/inputProps";
import { api, type LogRecord } from "../lib/api";
import { bodyPreview, fmtTime, severityInfo } from "../lib/format";
import { EmptyState } from "../components/EmptyState";
import { Field } from "../components/Field";
import { FilterSelect } from "../components/FilterSelect";
import { ServiceChip } from "../components/ServiceChip";

const SEVERITIES = [
  { key: "0", label: "all levels" },
  { key: "5", label: "debug +" },
  { key: "9", label: "info +" },
  { key: "13", label: "warn +" },
  { key: "17", label: "error +" },
];

function logKey(l: LogRecord, i: number) {
  return `${l.time_unix_nano}-${l.span_id}-${i}`;
}

export function LogsView() {
  const [filters, setFilters] = useAtom(logFiltersAtom);
  const timeParams = useTimeParams();
  const live = useAtomValue(liveAtom);
  const [selectedLog, setSelectedLog] = useAtom(selectedLogAtom);
  const setInspectorOpen = useSetAtom(inspectorOpenAtom);

  const { data: services = [] } = useQuery({ queryKey: ["services"], queryFn: api.services });
  const { data: logs = [], isLoading } = useQuery({
    queryKey: ["logs", filters, timeParams],
    queryFn: () =>
      api.logs({
        service: filters.service || undefined,
        min_severity: filters.minSeverity || undefined,
        search: filters.search || undefined,
        ...timeParams,
        limit: filters.limit,
      }),
    refetchInterval: live ? 2_000 : false,
  });

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-end gap-2 border-b border-divider bg-content1 p-2">
        <Field label="service" className="w-44">
          <FilterSelect
            ariaLabel="Service"
            value={filters.service}
            onChange={(service) => setFilters({ ...filters, service })}
            options={[
              { value: "", label: "all services" },
              ...services.map((s) => ({ value: s, label: s })),
            ]}
          />
        </Field>
        <Field label="severity" className="w-36">
          <FilterSelect
            ariaLabel="Minimum severity"
            value={String(filters.minSeverity)}
            onChange={(v) => setFilters({ ...filters, minSeverity: Number(v) })}
            options={SEVERITIES.map((s) => ({ value: s.key, label: s.label }))}
          />
        </Field>
        <Field label="search" className="w-72">
        <Input
          {...plainTextField}
          {...fieldProps}
          aria-label="Search logs"
          placeholder="body, attributes…"
          value={filters.search}
          onValueChange={(search) => setFilters({ ...filters, search })}
          startContent={<IconSearch size={14} className="mr-1 shrink-0 text-default-400" />}
        />
        </Field>
        <span className="pb-2 text-[11px] text-default-500">
          {logs.length} records{live ? " · tailing" : ""}
        </span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto font-mono">
        {isLoading && <p className="p-4 text-sm text-default-500">loading…</p>}
        {!isLoading && logs.length === 0 && (
          <EmptyState
            icon={<IconAlignLeft size={44} stroke={1.2} />}
            title="no log records yet"
            hint="Nothing matches the current filters and time range — or no logs have been received."
          />
        )}
        {logs.map((l, i) => {
          const sev = severityInfo(l.severity_number, l.severity_text);
          const selected =
            selectedLog != null &&
            selectedLog.time_unix_nano === l.time_unix_nano &&
            selectedLog.span_id === l.span_id &&
            selectedLog.body === l.body;
          return (
            <button
              key={logKey(l, i)}
              onClick={() => {
                setSelectedLog(l);
                setInspectorOpen(true);
              }}
              className={`flex w-full items-center gap-2 border-b border-divider/40 px-3 py-1 text-left transition-colors hover:bg-content2 ${
                selected ? "bg-content2 shadow-[inset_2px_0_0_#FF2A6D]" : ""
              }`}
            >
              <span className="shrink-0 text-[11px] tabular-nums text-default-500">
                {fmtTime(l.time_unix_nano)}
              </span>
              <span
                className={`w-12 shrink-0 text-[10px] font-semibold ${sev.color}`}
                title={String(l.severity_number)}
              >
                {sev.level}
              </span>
              <ServiceChip service={l.service_name} small />
              <span className="truncate text-xs text-foreground/90">
                {bodyPreview(l.body)}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

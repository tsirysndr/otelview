import { Input, Select, SelectItem } from "@heroui/react";
import { IconSearch } from "@tabler/icons-react";
import { useQuery } from "@tanstack/react-query";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  inspectorOpenAtom,
  liveAtom,
  logFiltersAtom,
  lookbackAtom,
  selectedLogAtom,
} from "../state/atoms";
import { fieldProps, plainTextField } from "../lib/inputProps";
import { api, type LogRecord } from "../lib/api";
import { bodyPreview, fmtTime, severityInfo } from "../lib/format";
import { Field } from "../components/Field";
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
  const lookback = useAtomValue(lookbackAtom);
  const live = useAtomValue(liveAtom);
  const [selectedLog, setSelectedLog] = useAtom(selectedLogAtom);
  const setInspectorOpen = useSetAtom(inspectorOpenAtom);

  const { data: services = [] } = useQuery({ queryKey: ["services"], queryFn: api.services });
  const { data: logs = [], isLoading } = useQuery({
    queryKey: ["logs", filters, lookback],
    queryFn: () =>
      api.logs({
        service: filters.service || undefined,
        min_severity: filters.minSeverity || undefined,
        search: filters.search || undefined,
        lookback,
        limit: filters.limit,
      }),
    refetchInterval: live ? 2_000 : false,
  });

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-end gap-2 border-b border-divider bg-content1 p-2">
        <Field label="service" className="w-44">
        <Select
          {...fieldProps}
          aria-label="Service"
          placeholder="all services"
          selectedKeys={filters.service ? [filters.service] : []}
          onSelectionChange={(keys) =>
            setFilters({ ...filters, service: (Array.from(keys)[0] as string) ?? "" })
          }
          items={[{ key: "", label: "all services" }, ...services.map((s) => ({ key: s, label: s }))]}
        >
          {(item) => <SelectItem key={item.key}>{item.label}</SelectItem>}
        </Select>
        </Field>
        <Field label="severity" className="w-36">
        <Select
          {...fieldProps}
          aria-label="Minimum severity"
          selectedKeys={[String(filters.minSeverity)]}
          onSelectionChange={(keys) =>
            setFilters({ ...filters, minSeverity: Number(Array.from(keys)[0] ?? 0) })
          }
          items={SEVERITIES}
        >
          {(item) => <SelectItem key={item.key}>{item.label}</SelectItem>}
        </Select>
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
          <div className="flex h-full flex-col items-center justify-center gap-2 text-default-500">
            <p className="text-sm">no log records</p>
            <p className="text-xs">
              send OTLP to <span className="text-neon-cyan">http :4318/v1/logs</span>
            </p>
          </div>
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

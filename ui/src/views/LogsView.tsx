import { IconAlignLeft, IconLayoutSidebarLeftCollapse, IconLayoutSidebarLeftExpand } from "@tabler/icons-react";
import { useQuery } from "@tanstack/react-query";
import { useAtom, useAtomValue, useSetAtom } from "jotai";
import {
  inspectorOpenAtom,
  liveAtom,
  logFieldsOpenAtom,
  logFiltersAtom,
  selectedLogAtom,
} from "../state/atoms";
import { useTimeParams } from "../hooks/useTimeParams";
import { api, ApiError, type LogRecord } from "../lib/api";
import { bodyPreview, fmtTime, severityInfo } from "../lib/format";
import { EmptyState } from "../components/EmptyState";
import { Field } from "../components/Field";
import { KqlInput } from "../components/KqlInput";
import { LogHistogram } from "../components/LogHistogram";
import { SkeletonHistogram, SkeletonRows } from "../components/Skeleton";
import { FilterSelect } from "../components/FilterSelect";
import { ServiceChip } from "../components/ServiceChip";

/// How many more log lines "load older" asks for each time.
const LOG_PAGE = 300;

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
  const [fieldsOpen, setFieldsOpen] = useAtom(logFieldsOpenAtom);

  const { data: histogram = [], isLoading: histogramLoading } = useQuery({
    queryKey: ["log-histogram", filters, timeParams],
    queryFn: () =>
      api.logHistogram({
        service: filters.service || undefined,
        min_severity: filters.minSeverity || undefined,
        kql: filters.search || undefined,
        ...timeParams,
        buckets: 60,
      }),
    refetchInterval: live ? 5_000 : false,
    retry: false,
  });

  const { data: logs = [], isLoading, error } = useQuery({
    queryKey: ["logs", filters, timeParams],
    queryFn: () =>
      api.logs({
        service: filters.service || undefined,
        min_severity: filters.minSeverity || undefined,
        kql: filters.search || undefined,
        ...timeParams,
        limit: filters.limit,
      }),
    refetchInterval: live ? 2_000 : false,
    retry: false,
  });
  const kqlError =
    error instanceof ApiError && error.status === 400 ? error.message : null;

  // The server returns at most `limit`, so a full page means there is more in
  // the window than is on screen. Worth saying: without it a capped list is
  // indistinguishable from a window that simply holds nothing older.
  const truncated = !isLoading && !kqlError && logs.length >= filters.limit;

  const { data: fields = [] } = useQuery({
    queryKey: ["log-fields", filters.service, filters.minSeverity, timeParams],
    queryFn: () =>
      api.logFields({
        service: filters.service || undefined,
        min_severity: filters.minSeverity || undefined,
        ...timeParams,
      }),
    refetchInterval: live ? 10_000 : false,
  });

  const addToQuery = (clause: string) => {
    const q = filters.search.trim();
    setFilters({ ...filters, search: q ? `${q} and ${clause}` : clause });
  };

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
        <Field label="query (KQL)" className="min-w-72 flex-1">
          <KqlInput
            historyKey="logs.kql"
            savedKind="logs.kql"
            value={filters.search}
            onChange={(search) => setFilters({ ...filters, search })}
            fields={fields}
            invalid={!!kqlError}
            placeholder='http.method:POST and status_code:>=500 · body:"card declined"'
          />
        </Field>
        <button
          onClick={() => setFieldsOpen(!fieldsOpen)}
          aria-label="Toggle fields sidebar"
          title="Toggle fields sidebar"
          className="pb-1.5 text-default-500 transition-colors hover:text-foreground"
        >
          {fieldsOpen ? (
            <IconLayoutSidebarLeftCollapse size={17} />
          ) : (
            <IconLayoutSidebarLeftExpand size={17} />
          )}
        </button>
        <span className="pb-2 text-[11px] text-default-500">
          {logs.length} records{live ? " · tailing" : ""}
        </span>
      </div>

      {kqlError && (
        <div className="shrink-0 border-b border-divider bg-danger/10 px-3 py-1.5 text-xs text-danger">
          {kqlError}
        </div>
      )}
      {histogramLoading && !kqlError && (
        <div className="shrink-0 border-b border-divider px-2 pt-1">
          <SkeletonHistogram />
        </div>
      )}
      {!histogramLoading && histogram.length > 0 && !kqlError && (
        <div className="shrink-0 border-b border-divider px-2 pt-1">
          <LogHistogram buckets={histogram} />
        </div>
      )}

      <div className="flex min-h-0 flex-1">
      {fieldsOpen && (
        <aside className="w-60 shrink-0 overflow-y-auto border-r border-divider bg-content1 p-2">
          <h3 className="mb-1 px-1 text-[10px] uppercase tracking-wider text-default-500">
            fields
          </h3>
          {fields.length === 0 && (
            <p className="px-1 text-[11px] text-default-400">no fields yet</p>
          )}
          {fields.map((f) => (
            <details key={f.name} className="group mb-0.5">
              <summary className="flex cursor-pointer items-baseline justify-between gap-2 rounded px-1 py-0.5 text-[11px] hover:bg-content2">
                <span className="truncate text-default-600">{f.name}</span>
                <span className="shrink-0 text-[10px] text-default-400">{f.count}</span>
              </summary>
              <div className="mb-1 ml-2 flex flex-col">
                {f.top_values.map(([value, count]) => (
                  <button
                    key={value}
                    onClick={() =>
                      addToQuery(
                        /[\s:"()]/.test(value)
                          ? `${f.name}:"${value.replaceAll('"', '\\"')}"`
                          : `${f.name}:${value}`,
                      )
                    }
                    title={`add ${f.name}:${value} to the query`}
                    className="flex items-baseline justify-between gap-2 rounded px-1 py-0.5 text-left text-[11px] text-default-500 hover:bg-content2 hover:text-neon-cyan"
                  >
                    <span className="truncate">{value || "∅"}</span>
                    <span className="shrink-0 text-[10px] text-default-400">{count}</span>
                  </button>
                ))}
              </div>
            </details>
          ))}
        </aside>
      )}
      <div className="min-h-0 min-w-0 flex-1 overflow-y-auto font-mono">
        {isLoading && <SkeletonRows rows={14} label="loading logs" />}
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
        {truncated && (
          <div className="flex items-center justify-center gap-3 border-t border-divider px-3 py-3 text-[11px] text-default-500">
            {/* The list is the newest N, not the whole window. Said plainly,
                because a silently capped list reads as missing data — the
                histogram above always covers the full range. */}
            <span>
              showing the newest {logs.length.toLocaleString()} — the window holds more
            </span>
            <button
              onClick={() => setFilters({ ...filters, limit: filters.limit + LOG_PAGE })}
              className="rounded border border-divider px-2 py-1 text-[11px] text-default-600 transition-colors hover:border-neon-cyan hover:text-neon-cyan"
            >
              load {LOG_PAGE.toLocaleString()} older
            </button>
          </div>
        )}
      </div>
      </div>
    </div>
  );
}

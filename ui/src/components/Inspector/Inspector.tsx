import { Button, Chip, Tab, Tabs } from "@heroui/react";
import { IconAlignLeft, IconChartLine, IconExternalLink, IconRoute, IconX } from "@tabler/icons-react";
import { useQuery } from "@tanstack/react-query";
import { useAtom, useAtomValue } from "jotai";
import {
  inspectorOpenAtom,
  openTraceIdAtom,
  selectedLogAtom,
  selectedSpanIdAtom,
  viewAtom,
} from "../../state/atoms";
import { api, type SpanRecord } from "../../lib/api";
import { fmtDateTime, fmtDuration, fmtTime, severityInfo } from "../../lib/format";
import { useCorrelate } from "../../hooks/useCorrelate";
import { KeyValueTable } from "../KeyValueTable";
import { ServiceChip } from "../ServiceChip";

function StatusChip({ span }: { span: SpanRecord }) {
  if (span.status_code === 2) {
    return (
      <Chip size="sm" color="danger" variant="flat">
        ERROR{span.status_message ? `: ${span.status_message}` : ""}
      </Chip>
    );
  }
  if (span.status_code === 1) {
    return (
      <Chip size="sm" color="success" variant="flat">
        OK
      </Chip>
    );
  }
  return null;
}

function SpanDetails({ span }: { span: SpanRecord }) {
  const correlate = useCorrelate();
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-3">
      <div>
        <h3 className="break-all text-sm font-semibold">{span.name}</h3>
        <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
          <ServiceChip service={span.service_name} small />
          <Chip size="sm" variant="flat" className="text-[10px]">
            {span.kind}
          </Chip>
          <StatusChip span={span} />
        </div>
      </div>

      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
        <dt className="text-default-500">duration</dt>
        <dd className="text-neon-cyan">
          {fmtDuration(span.end_time_unix_nano - span.start_time_unix_nano)}
        </dd>
        <dt className="text-default-500">start</dt>
        <dd>{fmtDateTime(span.start_time_unix_nano)}</dd>
        <dt className="text-default-500">span id</dt>
        <dd className="break-all">{span.span_id}</dd>
        <dt className="text-default-500">parent</dt>
        <dd className="break-all">{span.parent_span_id || "— (root)"}</dd>
        {span.scope_name && (
          <>
            <dt className="text-default-500">scope</dt>
            <dd className="break-all">
              {span.scope_name}
              {span.scope_version ? ` @ ${span.scope_version}` : ""}
            </dd>
          </>
        )}
      </dl>

      {/* The three signals meet at this span: its own logs, and the wider
          behaviour of the service it ran in. */}
      <div className="flex flex-wrap gap-1.5">
        <Button
          size="sm"
          variant="flat"
          startContent={<IconAlignLeft size={14} />}
          onPress={() =>
            correlate.traceToLogs(span.trace_id, {
              spanId: span.span_id,
              atUnixNano: span.start_time_unix_nano,
            })
          }
        >
          logs for this span
        </Button>
        <Button
          size="sm"
          variant="flat"
          startContent={<IconAlignLeft size={14} />}
          onPress={() =>
            correlate.traceToLogs(span.trace_id, {
              atUnixNano: span.start_time_unix_nano,
            })
          }
        >
          whole trace
        </Button>
        <Button
          size="sm"
          variant="flat"
          startContent={<IconChartLine size={14} />}
          onPress={() =>
            correlate.serviceToMetrics(span.service_name, {
              atUnixNano: span.start_time_unix_nano,
            })
          }
        >
          {span.service_name} metrics
        </Button>
      </div>

      <Tabs size="sm" variant="underlined" aria-label="Span detail sections">
        <Tab key="attrs" title={`Attributes (${Object.keys(span.attributes ?? {}).length})`}>
          <KeyValueTable data={span.attributes} />
        </Tab>
        <Tab key="resource" title="Resource">
          <KeyValueTable data={span.resource_attributes} />
        </Tab>
        <Tab key="events" title={`Events (${span.events?.length ?? 0})`}>
          {(span.events ?? []).length === 0 ? (
            <p className="px-1 py-2 text-xs text-default-400">none</p>
          ) : (
            <div className="flex flex-col gap-2">
              {span.events.map((e, i) => (
                <div key={i} className="rounded-md bg-content2 p-2">
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="text-xs font-medium">{e.name}</span>
                    <span className="text-[10px] text-default-500">
                      {fmtTime(e.time_unix_nano)}
                    </span>
                  </div>
                  <KeyValueTable data={e.attributes} />
                </div>
              ))}
            </div>
          )}
        </Tab>
        <Tab key="links" title={`Links (${span.links?.length ?? 0})`}>
          {(span.links ?? []).length === 0 ? (
            <p className="px-1 py-2 text-xs text-default-400">none</p>
          ) : (
            <div className="flex flex-col gap-1 text-xs">
              {span.links.map((l, i) => (
                <button
                  key={i}
                  onClick={() =>
                    correlate.logToTrace(l.trace_id, { spanId: l.span_id || undefined })
                  }
                  title="open the linked trace"
                  className="flex items-center gap-1.5 break-all rounded-md bg-content2 p-2 text-left transition-colors hover:text-neon-cyan"
                >
                  <IconRoute size={13} className="shrink-0" />
                  <span>
                    trace {l.trace_id} · span {l.span_id}
                  </span>
                </button>
              ))}
            </div>
          )}
        </Tab>
      </Tabs>
    </div>
  );
}

function LogDetails() {
  const log = useAtomValue(selectedLogAtom);
  const correlate = useCorrelate();
  if (!log) return null;
  const sev = severityInfo(log.severity_number, log.severity_text);
  const hasTrace = log.trace_id && !/^0*$/.test(log.trace_id);
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-3">
      <div className="flex flex-wrap items-center gap-1.5">
        <span
          className="inline-block h-2.5 w-2.5 rounded-full"
          style={{ background: sev.dot }}
        />
        <span className={`text-xs font-semibold ${sev.color}`}>{sev.level}</span>
        <ServiceChip service={log.service_name} small />
        <span className="text-[10px] text-default-500">{fmtDateTime(log.time_unix_nano)}</span>
      </div>

      <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-md bg-content2 p-2 text-xs">
        {typeof log.body === "string" ? log.body : JSON.stringify(log.body, null, 2)}
      </pre>

      <div className="flex flex-wrap gap-1.5">
        {hasTrace && (
          <Button
            size="sm"
            variant="flat"
            color="secondary"
            startContent={<IconExternalLink size={14} />}
            onPress={() =>
              correlate.logToTrace(log.trace_id, {
                spanId: log.span_id || undefined,
                atUnixNano: log.time_unix_nano,
              })
            }
          >
            open trace {log.trace_id.slice(0, 12)}…
          </Button>
        )}
        <Button
          size="sm"
          variant="flat"
          startContent={<IconChartLine size={14} />}
          onPress={() =>
            correlate.serviceToMetrics(log.service_name, {
              atUnixNano: log.time_unix_nano,
            })
          }
        >
          {log.service_name} metrics
        </Button>
      </div>

      <Tabs size="sm" variant="underlined" aria-label="Log detail sections">
        <Tab key="attrs" title={`Attributes (${Object.keys(log.attributes ?? {}).length})`}>
          <KeyValueTable data={log.attributes} />
        </Tab>
        <Tab key="resource" title="Resource">
          <KeyValueTable data={log.resource_attributes} />
        </Tab>
      </Tabs>
    </div>
  );
}

/** Right-hand detail panel (VS Code secondary sidebar). */
export function Inspector() {
  const view = useAtomValue(viewAtom);
  const inspectorOpen = useAtomValue(inspectorOpenAtom);
  const openTraceId = useAtomValue(openTraceIdAtom);
  const [selectedSpanId, setSelectedSpanId] = useAtom(selectedSpanIdAtom);
  const [selectedLog, setSelectedLog] = useAtom(selectedLogAtom);

  const { data: spans } = useQuery({
    queryKey: ["trace", openTraceId],
    queryFn: () => api.trace(openTraceId!),
    enabled: view === "traces" && !!openTraceId,
  });

  const span =
    view === "traces" && selectedSpanId
      ? spans?.find((s) => s.span_id === selectedSpanId)
      : undefined;
  const showLog = view === "logs" && selectedLog;

  if (!inspectorOpen || (!span && !showLog)) return null;

  return (
    <aside
      className="fixed inset-0 z-40 flex flex-col bg-content1
        lg:static lg:inset-auto lg:z-auto lg:w-[360px] lg:shrink-0 lg:border-l lg:border-divider"
    >
      <div className="flex h-10 shrink-0 items-center justify-between border-b border-divider px-2 lg:h-8">
        <span className="text-[11px] uppercase tracking-wider text-default-500">
          {span ? "span" : "log record"}
        </span>
        <Button
          isIconOnly
          size="sm"
          variant="light"
          aria-label="Close inspector"
          onPress={() => {
            setSelectedSpanId(null);
            setSelectedLog(null);
          }}
        >
          <IconX size={14} />
        </Button>
      </div>
      {span ? <SpanDetails span={span} /> : <LogDetails />}
    </aside>
  );
}

import { useMemo, useState } from "react";
import { IconChevronDown, IconChevronRight, IconAlertTriangle } from "@tabler/icons-react";
import { useAtom, useSetAtom } from "jotai";
import { inspectorOpenAtom, selectedSpanIdAtom } from "../state/atoms";
import type { SpanRecord } from "../lib/api";
import { serviceColor } from "../lib/colors";
import { fmtDuration } from "../lib/format";

interface Node {
  span: SpanRecord;
  children: Node[];
  depth: number;
}

function buildTree(spans: SpanRecord[]): Node[] {
  const byId = new Map<string, Node>();
  for (const s of spans) byId.set(s.span_id, { span: s, children: [], depth: 0 });
  const roots: Node[] = [];
  for (const node of byId.values()) {
    const parent = node.span.parent_span_id
      ? byId.get(node.span.parent_span_id)
      : undefined;
    if (parent && parent !== node) {
      parent.children.push(node);
    } else {
      roots.push(node); // true root or orphan (parent not received)
    }
  }
  const sortRec = (nodes: Node[], depth: number) => {
    nodes.sort((a, b) => a.span.start_time_unix_nano - b.span.start_time_unix_nano);
    for (const n of nodes) {
      n.depth = depth;
      sortRec(n.children, depth + 1);
    }
  };
  sortRec(roots, 0);
  return roots;
}

export function Waterfall({ spans }: { spans: SpanRecord[] }) {
  const [selectedSpanId, setSelectedSpanId] = useAtom(selectedSpanIdAtom);
  const setInspectorOpen = useSetAtom(inspectorOpenAtom);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  const roots = useMemo(() => buildTree(spans), [spans]);
  const t0 = Math.min(...spans.map((s) => s.start_time_unix_nano));
  const t1 = Math.max(...spans.map((s) => s.end_time_unix_nano));
  const total = Math.max(t1 - t0, 1);

  const rows: Node[] = [];
  const walk = (nodes: Node[]) => {
    for (const n of nodes) {
      rows.push(n);
      if (!collapsed.has(n.span.span_id)) walk(n.children);
    }
  };
  walk(roots);

  const toggle = (id: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const ticks = [0, 0.25, 0.5, 0.75, 1];
  const NAME_W = 320;

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-auto">
      {/* time scale header */}
      <div
        className="sticky top-0 z-10 flex h-7 shrink-0 border-b border-divider bg-content1 text-[10px] text-default-500"
        style={{ minWidth: NAME_W + 400 }}
      >
        <div
          className="shrink-0 border-r border-divider px-2 leading-7"
          style={{ width: NAME_W }}
        >
          {spans.length} spans · {fmtDuration(total)}
        </div>
        <div className="relative flex-1">
          {ticks.map((f) => (
            <span
              key={f}
              className="absolute top-0 border-l border-divider pl-1 leading-7"
              style={{ left: `${f * 100}%` }}
            >
              {f > 0 && f < 1 ? fmtDuration(total * f) : f === 1 ? fmtDuration(total) : "0"}
            </span>
          ))}
        </div>
      </div>

      <div style={{ minWidth: NAME_W + 400 }}>
        {rows.map((n) => {
          const s = n.span;
          const left = ((s.start_time_unix_nano - t0) / total) * 100;
          const w = Math.max(((s.end_time_unix_nano - s.start_time_unix_nano) / total) * 100, 0.2);
          const isErr = s.status_code === 2;
          const color = serviceColor(s.service_name);
          const hasKids = n.children.length > 0;
          return (
            <div
              key={s.span_id}
              data-selected={selectedSpanId === s.span_id}
              className="span-row flex h-7 cursor-pointer items-center border-b border-divider/40"
              onClick={() => {
                setSelectedSpanId(s.span_id);
                setInspectorOpen(true);
              }}
            >
              <div
                className="flex h-full shrink-0 items-center gap-1 overflow-hidden border-r border-divider pr-1"
                style={{ width: NAME_W, paddingLeft: 6 + n.depth * 14 }}
              >
                {hasKids ? (
                  <button
                    className="shrink-0 text-default-500 hover:text-foreground"
                    onClick={(e) => {
                      e.stopPropagation();
                      toggle(s.span_id);
                    }}
                    aria-label={collapsed.has(s.span_id) ? "expand" : "collapse"}
                  >
                    {collapsed.has(s.span_id) ? (
                      <IconChevronRight size={13} />
                    ) : (
                      <IconChevronDown size={13} />
                    )}
                  </button>
                ) : (
                  <span className="w-[13px] shrink-0" />
                )}
                <span
                  className="h-2.5 w-1 shrink-0 rounded-sm"
                  style={{ background: color }}
                />
                <span className="truncate text-xs">
                  {isErr && (
                    <IconAlertTriangle
                      size={12}
                      className="mr-0.5 inline-block text-danger"
                    />
                  )}
                  {s.name}
                </span>
                <span className="ml-auto shrink-0 truncate pl-1 text-[10px] text-default-500">
                  {s.service_name}
                </span>
              </div>

              <div className="relative h-full flex-1">
                {/* faint tick grid */}
                {ticks.slice(1, -1).map((f) => (
                  <span
                    key={f}
                    className="absolute inset-y-0 border-l border-divider/40"
                    style={{ left: `${f * 100}%` }}
                  />
                ))}
                <div
                  className="absolute top-1/2 h-[9px] -translate-y-1/2 rounded-[3px]"
                  style={{
                    left: `${left}%`,
                    width: `${w}%`,
                    background: isErr ? "#FF3864" : color,
                    boxShadow: isErr ? "0 0 6px rgba(255,56,100,0.6)" : undefined,
                    minWidth: 2,
                  }}
                />
                <span
                  className="absolute top-1/2 -translate-y-1/2 whitespace-nowrap pl-1.5 text-[10px] text-default-500"
                  style={{
                    left: left + w > 88 ? undefined : `calc(${left + w}% )`,
                    right: left + w > 88 ? 4 : undefined,
                  }}
                >
                  {fmtDuration(s.end_time_unix_nano - s.start_time_unix_nano)}
                </span>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

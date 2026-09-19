import { useMemo, useRef, useState } from "react";
import type { TraceSummary } from "../lib/api";
import { fmtDuration, fmtTime } from "../lib/format";
import { STATUS } from "../lib/colors";

/** Jaeger-style duration-vs-time scatter of trace results. Click opens the
 * trace; errors are drawn in the reserved status red (with a ring, so state
 * is not color-alone). */
export function ScatterPlot({
  traces,
  onOpen,
  height = 130,
}: {
  traces: TraceSummary[];
  onOpen: (traceId: string) => void;
  height?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(800);
  const [hover, setHover] = useState<TraceSummary | null>(null);

  useMemo(() => {
    const obs = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width;
      if (w) setWidth(w);
    });
    const t = setTimeout(() => ref.current && obs.observe(ref.current), 0);
    return () => {
      clearTimeout(t);
      obs.disconnect();
    };
  }, []);

  if (traces.length === 0) return null;

  const pad = { l: 56, r: 14, t: 10, b: 20 };
  const iw = Math.max(50, width - pad.l - pad.r);
  const ih = height - pad.t - pad.b;
  const tMin = Math.min(...traces.map((t) => t.start_time_unix_nano));
  const tMax = Math.max(...traces.map((t) => t.start_time_unix_nano));
  const dMax = Math.max(...traces.map((t) => t.duration_nanos), 1);

  const x = (t: number) =>
    pad.l + (tMax === tMin ? iw / 2 : ((t - tMin) / (tMax - tMin)) * iw);
  const y = (d: number) => pad.t + ih - (d / dMax) * ih;

  return (
    <div ref={ref} className="relative w-full">
      <svg width={width} height={height} className="block">
        {[0.5, 1].map((f) => (
          <g key={f}>
            <line
              x1={pad.l}
              x2={pad.l + iw}
              y1={y(dMax * f)}
              y2={y(dMax * f)}
              stroke="hsl(var(--heroui-divider))"
              strokeWidth={1}
            />
            <text
              x={pad.l - 6}
              y={y(dMax * f) + 3}
              textAnchor="end"
              fontSize={10}
              fill="hsl(var(--heroui-default-500))"
            >
              {fmtDuration(dMax * f)}
            </text>
          </g>
        ))}
        {traces.map((t) => {
          const isErr = t.error_count > 0;
          return (
            <circle
              key={t.trace_id}
              cx={x(t.start_time_unix_nano)}
              cy={y(t.duration_nanos)}
              r={hover?.trace_id === t.trace_id ? 9 : 6}
              fill={isErr ? STATUS.error : "#05D9E8"}
              fillOpacity={0.9}
              stroke={isErr ? "#FFFFFF" : "hsl(var(--heroui-background))"}
              strokeWidth={isErr ? 1.5 : 1}
              className="neon-drop cursor-pointer transition-[r]"
              style={
                {
                  "--drop":
                    hover?.trace_id === t.trace_id
                      ? isErr
                        ? "drop-shadow(0 0 10px rgba(255,56,100,1)) drop-shadow(0 0 18px rgba(255,56,100,0.6))"
                        : "drop-shadow(0 0 10px rgba(5,217,232,1)) drop-shadow(0 0 18px rgba(5,217,232,0.6))"
                      : isErr
                        ? "drop-shadow(0 0 4px rgba(255,56,100,0.8))"
                        : "drop-shadow(0 0 3px rgba(5,217,232,0.6))",
                } as React.CSSProperties
              }
              onMouseEnter={() => setHover(t)}
              onMouseLeave={() => setHover(null)}
              onClick={() => onOpen(t.trace_id)}
            />
          );
        })}
      </svg>
      {hover && (
        <div
          className="pointer-events-none absolute z-10 rounded-md border border-divider bg-content1 p-2 text-xs shadow-lg"
          style={{
            left: Math.min(x(hover.start_time_unix_nano) + 10, width - 190),
            top: 4,
            width: 180,
          }}
        >
          <div className="truncate font-medium">{hover.root_name}</div>
          <div className="text-default-500">
            {fmtTime(hover.start_time_unix_nano)} · {fmtDuration(hover.duration_nanos)}
          </div>
          <div className="text-default-500">
            {hover.span_count} spans
            {hover.error_count > 0 && (
              <span className="text-danger"> · {hover.error_count} errors</span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

import { useMemo, useRef, useState } from "react";
import type { LogBucket } from "../../lib/api";
import { fmtTime } from "../../lib/format";

const LEVELS: { key: keyof Omit<LogBucket, "time_unix_nano">; color: string; label: string }[] = [
  { key: "trace", color: "#5B5890", label: "trace" },
  { key: "debug", color: "#8B87B3", label: "debug" },
  { key: "info", color: "#05B4C6", label: "info" },
  { key: "warn", color: "#FFD319", label: "warn" },
  { key: "error", color: "#FF3864", label: "error" },
  { key: "fatal", color: "#99183F", label: "fatal" },
];

/** Kibana-style log volume histogram: stacked severity bars over time. */
export function LogHistogram({ buckets, height = 84 }: { buckets: LogBucket[]; height?: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(800);
  const [hover, setHover] = useState<number | null>(null);

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

  if (buckets.length === 0) return null;
  const totals = buckets.map((b) =>
    LEVELS.reduce((sum, l) => sum + (b[l.key] as number), 0),
  );
  const max = Math.max(...totals, 1);
  const pad = { l: 30, r: 6, t: 6, b: 14 };
  const iw = Math.max(50, width - pad.l - pad.r);
  const ih = height - pad.t - pad.b;
  const bw = Math.max(iw / buckets.length - 2, 1.5);

  return (
    <div ref={ref} className="relative w-full">
      <svg width={width} height={height} className="block">
        <text x={pad.l - 4} y={pad.t + 8} textAnchor="end" fontSize={9} fill="hsl(var(--heroui-default-500))">
          {max}
        </text>
        <line
          x1={pad.l}
          x2={pad.l + iw}
          y1={pad.t + ih}
          y2={pad.t + ih}
          stroke="hsl(var(--heroui-divider))"
        />
        {buckets.map((b, i) => {
          const x = pad.l + (i / buckets.length) * iw;
          let y = pad.t + ih;
          return (
            <g
              key={b.time_unix_nano}
              onMouseEnter={() => setHover(i)}
              onMouseLeave={() => setHover(null)}
              opacity={hover === null || hover === i ? 1 : 0.55}
            >
              {/* hit area */}
              <rect x={x} y={pad.t} width={bw + 2} height={ih} fill="transparent" />
              {LEVELS.map((l) => {
                const v = b[l.key] as number;
                if (v === 0) return null;
                const h = (v / max) * ih;
                y -= h;
                return (
                  <rect
                    key={l.key}
                    x={x}
                    y={y}
                    width={bw}
                    height={Math.max(h - 0.5, 0.5)}
                    fill={l.color}
                    rx={1}
                  />
                );
              })}
            </g>
          );
        })}
        {[0, 0.5, 1].map((f) => {
          const idx = Math.min(Math.round(f * (buckets.length - 1)), buckets.length - 1);
          return (
            <text
              key={f}
              x={pad.l + f * iw}
              y={height - 3}
              textAnchor={f === 0 ? "start" : f === 1 ? "end" : "middle"}
              fontSize={9}
              fill="hsl(var(--heroui-default-500))"
            >
              {fmtTime(buckets[idx].time_unix_nano).slice(0, 8)}
            </text>
          );
        })}
      </svg>
      {hover !== null && (
        <div
          className="pointer-events-none absolute top-1 z-10 rounded-md border border-divider bg-content1 px-2 py-1 text-[10px]"
          style={{
            left: Math.min(pad.l + (hover / buckets.length) * iw + 8, width - 130),
          }}
        >
          <div className="text-default-500">{fmtTime(buckets[hover].time_unix_nano)}</div>
          {LEVELS.filter((l) => (buckets[hover][l.key] as number) > 0).map((l) => (
            <div key={l.key} className="flex items-center justify-between gap-3">
              <span className="flex items-center gap-1">
                <span className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: l.color }} />
                <span className="text-default-600">{l.label}</span>
              </span>
              <span>{buckets[hover][l.key] as number}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

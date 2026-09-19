import { useMemo, useRef, useState } from "react";
import { seriesColor } from "../lib/colors";
import { fmtTime } from "../lib/format";

export interface ChartSeries {
  label: string;
  points: { t: number; v: number }[]; // t = unix nanos
}

interface Hover {
  x: number;
  t: number;
  values: { label: string; v: number | null; color: string }[];
}

function niceTicks(min: number, max: number, count: number): number[] {
  if (!isFinite(min) || !isFinite(max) || min === max) {
    return [min];
  }
  const span = max - min;
  const step = Math.pow(10, Math.floor(Math.log10(span / count)));
  const err = (span / count) / step;
  const mult = err >= 7.5 ? 10 : err >= 3.5 ? 5 : err >= 1.5 ? 2 : 1;
  const s = step * mult;
  const ticks: number[] = [];
  for (let v = Math.ceil(min / s) * s; v <= max + 1e-12; v += s) ticks.push(v);
  return ticks;
}

function fmtValue(v: number): string {
  if (Math.abs(v) >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (Math.abs(v) >= 1_000) return `${(v / 1_000).toFixed(1)}k`;
  if (Number.isInteger(v)) return String(v);
  return v.toPrecision(3);
}

/** Multi-series SVG line chart with crosshair + tooltip and a legend. */
export function LineChart({
  series,
  height = 260,
  unit,
}: {
  series: ChartSeries[];
  height?: number;
  unit?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<Hover | null>(null);
  const [width, setWidth] = useState(800);

  // Track container width.
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

  const pad = { l: 48, r: 12, t: 10, b: 22 };
  const iw = Math.max(50, width - pad.l - pad.r);
  const ih = height - pad.t - pad.b;

  const all = series.flatMap((s) => s.points);
  const tMin = Math.min(...all.map((p) => p.t));
  const tMax = Math.max(...all.map((p) => p.t));
  const vMinRaw = Math.min(0, ...all.map((p) => p.v));
  const vMaxRaw = Math.max(...all.map((p) => p.v));
  const vTicks = niceTicks(vMinRaw, vMaxRaw === vMinRaw ? vMinRaw + 1 : vMaxRaw, 4);
  const vMin = Math.min(vMinRaw, vTicks[0] ?? 0);
  const vMax = Math.max(vMaxRaw, vTicks[vTicks.length - 1] ?? 1);

  const x = (t: number) =>
    pad.l + (tMax === tMin ? iw / 2 : ((t - tMin) / (tMax - tMin)) * iw);
  const y = (v: number) =>
    pad.t + ih - (vMax === vMin ? ih / 2 : ((v - vMin) / (vMax - vMin)) * ih);

  if (all.length === 0) return null;

  const onMove = (e: React.MouseEvent<SVGSVGElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const t = tMin + ((px - pad.l) / iw) * (tMax - tMin);
    const values = series.map((s, i) => {
      let best: { t: number; v: number } | null = null;
      for (const p of s.points) {
        if (!best || Math.abs(p.t - t) < Math.abs(best.t - t)) best = p;
      }
      return { label: s.label, v: best?.v ?? null, color: seriesColor(i) };
    });
    const snapT =
      series
        .flatMap((s) => s.points)
        .reduce<{ t: number } | null>(
          (acc, p) => (!acc || Math.abs(p.t - t) < Math.abs(acc.t - t) ? p : acc),
          null,
        )?.t ?? t;
    setHover({ x: x(snapT), t: snapT, values });
  };

  const tTickCount = Math.max(2, Math.floor(iw / 140));
  const tTicks = niceTicks(tMin, tMax, tTickCount);

  return (
    <div ref={ref} className="relative w-full">
      <svg
        width={width}
        height={height}
        onMouseMove={onMove}
        onMouseLeave={() => setHover(null)}
        className="block"
      >
        {/* recessive horizontal grid */}
        {vTicks.map((v) => (
          <g key={v}>
            <line
              x1={pad.l}
              x2={pad.l + iw}
              y1={y(v)}
              y2={y(v)}
              stroke="hsl(var(--heroui-divider))"
              strokeWidth={1}
            />
            <text
              x={pad.l - 6}
              y={y(v) + 3}
              textAnchor="end"
              fontSize={10}
              fill="hsl(var(--heroui-default-500))"
            >
              {fmtValue(v)}
            </text>
          </g>
        ))}
        {/* time axis labels */}
        {tTicks.map((t) => (
          <text
            key={t}
            x={x(t)}
            y={height - 6}
            textAnchor="middle"
            fontSize={10}
            fill="hsl(var(--heroui-default-500))"
          >
            {fmtTime(t).slice(0, 8)}
          </text>
        ))}
        {/* series lines */}
        {series.map((s, i) => {
          const sorted = [...s.points].sort((a, b) => a.t - b.t);
          const d = sorted
            .map((p, j) => `${j === 0 ? "M" : "L"}${x(p.t).toFixed(1)},${y(p.v).toFixed(1)}`)
            .join(" ");
          return (
            <g key={s.label}>
              <path d={d} fill="none" stroke={seriesColor(i)} strokeWidth={2} />
              {sorted.length === 1 && (
                <circle cx={x(sorted[0].t)} cy={y(sorted[0].v)} r={3} fill={seriesColor(i)} />
              )}
            </g>
          );
        })}
        {/* crosshair */}
        {hover && (
          <line
            x1={hover.x}
            x2={hover.x}
            y1={pad.t}
            y2={pad.t + ih}
            stroke="hsl(var(--heroui-default-400))"
            strokeDasharray="3,3"
            strokeWidth={1}
          />
        )}
      </svg>

      {hover && (
        <div
          className="pointer-events-none absolute top-2 z-10 rounded-md border border-divider bg-content1 p-2 text-xs shadow-lg"
          style={{
            left: hover.x + 160 > width ? hover.x - 168 : hover.x + 8,
            minWidth: 150,
          }}
        >
          <div className="mb-1 text-[10px] text-default-500">{fmtTime(hover.t)}</div>
          {hover.values.map((v) => (
            <div key={v.label} className="flex items-center justify-between gap-3">
              <span className="flex min-w-0 items-center gap-1.5">
                <span
                  className="inline-block h-2 w-2 shrink-0 rounded-full"
                  style={{ background: v.color }}
                />
                <span className="truncate text-default-600">{v.label}</span>
              </span>
              <span className="text-foreground">
                {v.v == null ? "—" : fmtValue(v.v)}
                {unit ? ` ${unit}` : ""}
              </span>
            </div>
          ))}
        </div>
      )}

      {/* legend (identity never by color alone: labels always shown) */}
      {series.length >= 2 && (
        <div className="mt-1 flex flex-wrap gap-x-4 gap-y-1 px-2">
          {series.map((s, i) => (
            <span key={s.label} className="flex items-center gap-1.5 text-[11px] text-default-600">
              <span
                className="inline-block h-2 w-2 rounded-full"
                style={{ background: seriesColor(i) }}
              />
              {s.label}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

import { useMemo, useRef, useState } from "react";
import type { ServiceGraph } from "../../lib/api";
import { serviceNeon } from "../../lib/colors";
import { fmtDuration } from "../../lib/format";

/** Dependency graph derived from spans: services on a circle, calls as
 * curved edges (width ∝ volume, red when errors flow across). */
export function ServiceMap({ graph, height = 300 }: { graph: ServiceGraph; height?: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(800);
  const [hover, setHover] = useState<string | null>(null);

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

  const cx = width / 2;
  const cy = height / 2;
  const r = Math.max(Math.min(cx, cy) - 56, 60);
  const pos = new Map<string, { x: number; y: number }>();
  graph.nodes.forEach((n, i) => {
    const angle = (i / Math.max(graph.nodes.length, 1)) * Math.PI * 2 - Math.PI / 2;
    pos.set(n.service, { x: cx + r * Math.cos(angle), y: cy + r * Math.sin(angle) });
  });

  const maxCalls = Math.max(...graph.edges.map((e) => e.calls), 1);

  return (
    <div ref={ref} className="relative w-full">
      <svg width={width} height={height} className="block">
        <defs>
          <marker id="arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M0,0 L8,4 L0,8 z" fill="hsl(var(--heroui-default-400))" />
          </marker>
          <marker id="arrow-err" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M0,0 L8,4 L0,8 z" fill="#FF3864" />
          </marker>
        </defs>
        {graph.edges.map((e) => {
          const a = pos.get(e.source);
          const b = pos.get(e.target);
          if (!a || !b) return null;
          const hasErr = e.errors > 0;
          const active = hover === null || hover === e.source || hover === e.target;
          // Trim ends so arrows stop at the node circle, curve through a
          // point pulled toward the center.
          const dx = b.x - a.x;
          const dy = b.y - a.y;
          const len = Math.hypot(dx, dy) || 1;
          const pad = 24;
          const ax = a.x + (dx / len) * pad;
          const ay = a.y + (dy / len) * pad;
          const bx = b.x - (dx / len) * pad;
          const by = b.y - (dy / len) * pad;
          const mx = (ax + bx) / 2 + (cx - (ax + bx) / 2) * 0.25;
          const my = (ay + by) / 2 + (cy - (ay + by) / 2) * 0.25;
          return (
            <g key={`${e.source}→${e.target}`} opacity={active ? 1 : 0.15}>
              <path
                d={`M${ax},${ay} Q${mx},${my} ${bx},${by}`}
                fill="none"
                stroke={hasErr ? "#FF3864" : "hsl(var(--heroui-default-400))"}
                strokeWidth={1 + (e.calls / maxCalls) * 3.5}
                strokeOpacity={0.7}
                markerEnd={hasErr ? "url(#arrow-err)" : "url(#arrow)"}
              >
                <title>
                  {e.source} → {e.target}: {e.calls} calls
                  {e.errors > 0 ? `, ${e.errors} errors` : ""}, avg{" "}
                  {fmtDuration(e.avg_ms * 1e6)}
                </title>
              </path>
            </g>
          );
        })}
        {graph.nodes.map((n) => {
          const p = pos.get(n.service)!;
          const { color, glow } = serviceNeon(n.service);
          const active = hover === null || hover === n.service;
          return (
            <g
              key={n.service}
              opacity={active ? 1 : 0.3}
              className="cursor-pointer"
              onMouseEnter={() => setHover(n.service)}
              onMouseLeave={() => setHover(null)}
            >
              <circle
                cx={p.x}
                cy={p.y}
                r={18}
                fill="hsl(var(--heroui-content2))"
                stroke={n.error_count > 0 ? "#FF3864" : color}
                strokeWidth={2}
                className="neon-drop"
                style={{ "--drop": `drop-shadow(0 0 6px ${glow.includes("0 0") ? color : color}66)` } as React.CSSProperties}
              >
                <title>
                  {n.service}: {n.span_count} spans
                  {n.error_count > 0 ? `, ${n.error_count} errors` : ""}, avg{" "}
                  {fmtDuration(n.avg_ms * 1e6)}
                </title>
              </circle>
              <circle cx={p.x} cy={p.y} r={5} fill={color} />
              <text
                x={p.x}
                y={p.y + 32}
                textAnchor="middle"
                fontSize={11}
                fill="hsl(var(--heroui-foreground))"
              >
                {n.service}
              </text>
              <text
                x={p.x}
                y={p.y + 44}
                textAnchor="middle"
                fontSize={9}
                fill="hsl(var(--heroui-default-500))"
              >
                {fmtDuration(n.avg_ms * 1e6)} avg
              </text>
            </g>
          );
        })}
      </svg>
    </div>
  );
}

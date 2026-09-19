import { useQuery } from "@tanstack/react-query";
import { api } from "../lib/api";
import { fmtCount } from "../lib/format";

export function StatusLine() {
  const { data, isError } = useQuery({
    queryKey: ["stats"],
    queryFn: api.stats,
    refetchInterval: 5_000,
  });

  return (
    <footer className="flex h-6 shrink-0 items-center gap-4 border-t border-divider bg-[#171530] px-3 text-[11px] text-default-500">
      <span className="flex items-center gap-1.5">
        <span
          className={`inline-block h-2 w-2 rounded-full ${
            isError
              ? "bg-danger shadow-[0_0_5px_#FF3864]"
              : "bg-neon-green shadow-[0_0_5px_#05FFA1]"
          }`}
        />
        {isError ? "disconnected" : "connected"}
      </span>
      {data && (
        <>
          <span>
            storage <span className="text-default-600">{data.backend}</span>
          </span>
          <span>
            spans <span className="text-neon-cyan">{fmtCount(data.spans)}</span>
          </span>
          <span>
            logs <span className="text-neon-cyan">{fmtCount(data.logs)}</span>
          </span>
          <span>
            metric points <span className="text-neon-cyan">{fmtCount(data.metric_points)}</span>
          </span>
          <span>
            services <span className="text-neon-cyan">{fmtCount(data.services)}</span>
          </span>
        </>
      )}
      <div className="flex-1" />
      <span>OTLP gRPC :4317 · HTTP :4318</span>
    </footer>
  );
}

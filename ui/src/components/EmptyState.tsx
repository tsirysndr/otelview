import type { ReactNode } from "react";

/** Full-panel placeholder shown while a view has no data yet. */
export function EmptyState({
  icon,
  title,
  hint,
  showIngestHelp = true,
}: {
  icon: ReactNode;
  title: string;
  hint?: string;
  showIngestHelp?: boolean;
}) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
      <div className="text-default-300">{icon}</div>
      <p className="text-sm font-medium text-default-600">{title}</p>
      {hint && <p className="max-w-md text-xs text-default-500">{hint}</p>}
      {showIngestHelp && (
        <div className="mt-2 rounded-lg border border-divider bg-content1 p-3 text-left text-xs text-default-500">
          <p>
            point your apps at{" "}
            <span className="text-neon-cyan">grpc://host:4317</span> or{" "}
            <span className="text-neon-cyan">http://host:4318</span>
          </p>
          <pre className="mt-2 overflow-x-auto rounded bg-content2 p-2 text-[11px]">
            export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
          </pre>
        </div>
      )}
    </div>
  );
}

import type { ReactNode } from "react";
import { GettingStarted } from "./GettingStarted";

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
      {showIngestHelp && <GettingStarted />}
    </div>
  );
}

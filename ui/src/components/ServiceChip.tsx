import { serviceColor } from "../lib/colors";

export function ServiceChip({ service, small }: { service: string; small?: boolean }) {
  const color = serviceColor(service);
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-md bg-content2 ${
        small ? "px-1.5 py-0 text-[10px]" : "px-2 py-0.5 text-xs"
      } text-default-600`}
    >
      <span
        className="inline-block h-2 w-2 shrink-0 rounded-full"
        style={{ background: color }}
      />
      {service}
    </span>
  );
}

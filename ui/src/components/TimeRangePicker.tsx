import { useEffect, useRef, useState } from "react";
import { DateRangePicker } from "@heroui/react";
import { IconCalendar, IconX } from "@tabler/icons-react";
import { useAtom } from "jotai";
import { parseAbsoluteToLocal, type ZonedDateTime } from "@internationalized/date";
import { customRangeAtom, lookbackAtom } from "../state/atoms";

const LOOKBACKS = ["5m", "15m", "1h", "6h", "24h", "7d", "all"];

function toZoned(ms: number): ZonedDateTime {
  return parseAbsoluteToLocal(new Date(ms).toISOString());
}

function fmtRange(from: number, to: number): string {
  const opts: Intl.DateTimeFormatOptions = {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  };
  return `${new Date(from).toLocaleString([], opts)} → ${new Date(to).toLocaleString([], opts)}`;
}

/** Lookback pills + a themed HeroUI date-time range picker (to the minute). */
export function TimeRangePicker() {
  const [lookback, setLookback] = useAtom(lookbackAtom);
  const [custom, setCustom] = useAtom(customRangeAtom);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  return (
    <div ref={ref} className="relative flex items-center gap-1">
      {custom && !open ? (
        <button
          onClick={() => setOpen(true)}
          className="flex items-center gap-1.5 rounded-lg bg-content2 px-2 py-1 text-xs text-neon-cyan"
          title="Custom time range — click to edit"
        >
          <IconCalendar size={13} />
          {fmtRange(custom.from, custom.to)}
          <span
            role="button"
            aria-label="Clear custom range"
            className="ml-0.5 text-default-500 hover:text-foreground"
            onClick={(e) => {
              e.stopPropagation();
              setCustom(null);
            }}
          >
            <IconX size={12} />
          </span>
        </button>
      ) : !open ? (
        <div className="flex items-center gap-1 rounded-lg bg-content2 p-0.5">
          {LOOKBACKS.map((lb) => (
            <button
              key={lb}
              onClick={() => {
                setCustom(null);
                setLookback(lb);
              }}
              className={`rounded-md px-2 py-0.5 text-xs transition-colors ${
                lookback === lb
                  ? "bg-content4 text-foreground"
                  : "text-default-500 hover:text-foreground"
              }`}
            >
              {lb}
            </button>
          ))}
          <button
            onClick={() => setOpen(true)}
            aria-label="Custom time range"
            title="Custom time range"
            className="rounded-md px-1.5 py-0.5 text-default-500 transition-colors hover:text-foreground"
          >
            <IconCalendar size={14} />
          </button>
        </div>
      ) : (
        <div className="flex items-center gap-1">
          <DateRangePicker
            aria-label="Custom time range"
            variant="bordered"
            radius="sm"
            size="sm"
            granularity="minute"
            hideTimeZone
            hourCycle={24}
            visibleMonths={1}
            className="w-[350px]"
            classNames={{
              inputWrapper: "border-default-300 data-[hover=true]:border-default-400",
              selectorIcon: "text-neon-cyan",
            }}
            popoverProps={{
              classNames: {
                content:
                  "rounded-large border border-content3 bg-content1 shadow-none",
              },
            }}
            calendarProps={{
              classNames: {
                base: "bg-content1",
                headerWrapper: "bg-content1",
                gridHeader: "bg-content1 shadow-none",
                title: "text-default-600 text-xs uppercase tracking-wider",
                gridHeaderCell: "text-default-500",
                cellButton:
                  "data-[today=true]:text-neon-cyan data-[selected=true]:data-[range-selection=true]:bg-primary/20 data-[selection-start=true]:bg-primary data-[selection-end=true]:bg-primary",
              },
            }}
            value={
              custom
                ? { start: toZoned(custom.from), end: toZoned(custom.to) }
                : null
            }
            onChange={(v) => {
              if (!v?.start || !v?.end) return;
              const from = v.start.toDate().getTime();
              const to = v.end.toDate().getTime();
              if (from < to) setCustom({ from, to });
            }}
          />
          <button
            onClick={() => setOpen(false)}
            aria-label="Close range picker"
            className="rounded-md p-1 text-default-500 transition-colors hover:text-foreground"
          >
            <IconX size={14} />
          </button>
        </div>
      )}
    </div>
  );
}

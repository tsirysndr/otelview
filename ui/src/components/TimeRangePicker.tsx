import { useEffect, useRef, useState } from "react";
import { Button } from "@heroui/react";
import { IconCalendar, IconX } from "@tabler/icons-react";
import { useAtom } from "jotai";
import { customRangeAtom, lookbackAtom } from "../state/atoms";
import { plainTextField } from "../lib/inputProps";

const LOOKBACKS = ["5m", "15m", "1h", "6h", "24h", "7d", "all"];

function toLocalInput(ms: number): string {
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(
    d.getHours(),
  )}:${pad(d.getMinutes())}`;
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

const INPUT =
  "h-8 rounded-small border-2 border-default-300 bg-transparent px-2 text-xs text-foreground outline-none transition-colors hover:border-default-400 focus:border-default-500 [color-scheme:dark]";

/** Lookback pills + a custom absolute date-time range popover. */
export function TimeRangePicker() {
  const [lookback, setLookback] = useAtom(lookbackAtom);
  const [custom, setCustom] = useAtom(customRangeAtom);
  const [open, setOpen] = useState(false);
  const [from, setFrom] = useState(() => toLocalInput(Date.now() - 3_600_000));
  const [to, setTo] = useState(() => toLocalInput(Date.now()));
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const apply = () => {
    const fromMs = new Date(from).getTime();
    const toMs = new Date(to).getTime();
    if (Number.isNaN(fromMs) || Number.isNaN(toMs) || fromMs >= toMs) return;
    setCustom({ from: fromMs, to: toMs });
    setOpen(false);
  };

  return (
    <div ref={ref} className="relative flex items-center gap-1">
      {custom ? (
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
      ) : (
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
            onClick={() => setOpen((v) => !v)}
            aria-label="Custom time range"
            title="Custom time range"
            className="rounded-md px-1.5 py-0.5 text-default-500 transition-colors hover:text-foreground"
          >
            <IconCalendar size={14} />
          </button>
        </div>
      )}

      {open && (
        <div className="absolute right-0 top-9 z-40 flex w-64 flex-col gap-2 rounded-large border border-content3 bg-content1 p-3">
          <span className="text-[10px] uppercase tracking-wide text-default-500">
            custom range
          </span>
          <label className="flex flex-col gap-0.5">
            <span className="text-[10px] text-default-500">from</span>
            <input
              {...plainTextField}
              type="datetime-local"
              value={from}
              onChange={(e) => setFrom(e.target.value)}
              className={INPUT}
              aria-label="Range start"
            />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="text-[10px] text-default-500">to</span>
            <input
              {...plainTextField}
              type="datetime-local"
              value={to}
              onChange={(e) => setTo(e.target.value)}
              className={INPUT}
              aria-label="Range end"
            />
          </label>
          <div className="mt-1 flex justify-end gap-2">
            <Button size="sm" variant="light" radius="sm" onPress={() => setOpen(false)}>
              cancel
            </Button>
            <Button size="sm" color="secondary" variant="flat" radius="sm" onPress={apply}>
              apply
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

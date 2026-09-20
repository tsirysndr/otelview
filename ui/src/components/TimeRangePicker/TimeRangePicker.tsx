import { useEffect, useRef, useState } from "react";
import { Button } from "@heroui/react";
import { IconCalendar, IconX } from "@tabler/icons-react";
import { useAtom } from "jotai";
import { DayPicker, type DateRange } from "react-day-picker";
import "react-day-picker/style.css";
import { customRangeAtom, lookbackAtom } from "../../state/atoms";
import { plainTextField } from "../../lib/inputProps";

const LOOKBACKS = ["5m", "15m", "1h", "6h", "24h", "7d", "all"];

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

function parseTime(s: string): { h: number; m: number } | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(s.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 23 || min > 59) return null;
  return { h, m: min };
}

function fmtTimeOf(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

const TIME_INPUT =
  "h-7 w-16 rounded-small border-2 border-default-300 bg-transparent px-1.5 text-center text-xs " +
  "text-foreground outline-none transition-colors hover:border-default-400 focus:border-default-500";

/** Lookback pills + a custom calendar range popover (react-day-picker,
 * fully themed) with from/to time-of-day inputs. */
export function TimeRangePicker({ align = "right" }: { align?: "left" | "right" }) {
  const [lookback, setLookback] = useAtom(lookbackAtom);
  const [custom, setCustom] = useAtom(customRangeAtom);
  const [open, setOpen] = useState(false);
  const [range, setRange] = useState<DateRange | undefined>();
  const [fromTime, setFromTime] = useState("00:00");
  const [toTime, setToTime] = useState("23:59");
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  const openPanel = () => {
    if (custom) {
      setRange({ from: new Date(custom.from), to: new Date(custom.to) });
      setFromTime(fmtTimeOf(custom.from));
      setToTime(fmtTimeOf(custom.to));
    } else {
      const now = new Date();
      setRange({ from: now, to: now });
      setFromTime("00:00");
      setToTime("23:59");
    }
    setError(null);
    setOpen(true);
  };

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
    const fromDay = range?.from;
    const toDay = range?.to ?? range?.from;
    if (!fromDay || !toDay) return setError("pick a day range");
    const ft = parseTime(fromTime);
    const tt = parseTime(toTime);
    if (!ft || !tt) return setError("times must be HH:MM");
    const from = new Date(fromDay);
    from.setHours(ft.h, ft.m, 0, 0);
    const to = new Date(toDay);
    to.setHours(tt.h, tt.m, 59, 999);
    if (from.getTime() >= to.getTime()) return setError("start must be before end");
    setCustom({ from: from.getTime(), to: to.getTime() });
    setOpen(false);
  };

  return (
    <div ref={ref} className="relative flex items-center gap-1">
      {custom ? (
        <button
          onClick={openPanel}
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
            onClick={openPanel}
            aria-label="Custom time range"
            title="Custom time range"
            className="rounded-md px-1.5 py-0.5 text-default-500 transition-colors hover:text-foreground"
          >
            <IconCalendar size={14} />
          </button>
        </div>
      )}

      {open && (
        <div
          className={`otelview-rdp absolute top-9 z-40 flex flex-col gap-2 rounded-large border border-content3 bg-content1 p-3 ${
            align === "right" ? "right-0" : "left-0"
          }`}
        >
          <DayPicker
            mode="range"
            numberOfMonths={1}
            selected={range}
            onSelect={setRange}
            defaultMonth={range?.from}
            showOutsideDays
            weekStartsOn={1}
          />
          <div className="flex items-center justify-between gap-2 border-t border-divider pt-2">
            <label className="flex items-center gap-1.5 text-[10px] uppercase tracking-wide text-default-500">
              from
              <input
                {...plainTextField}
                value={fromTime}
                onChange={(e) => setFromTime(e.target.value)}
                placeholder="00:00"
                aria-label="Start time"
                className={TIME_INPUT}
              />
            </label>
            <label className="flex items-center gap-1.5 text-[10px] uppercase tracking-wide text-default-500">
              to
              <input
                {...plainTextField}
                value={toTime}
                onChange={(e) => setToTime(e.target.value)}
                placeholder="23:59"
                aria-label="End time"
                className={TIME_INPUT}
              />
            </label>
          </div>
          {error && <p className="text-[11px] text-danger">{error}</p>}
          <div className="flex justify-end gap-2">
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

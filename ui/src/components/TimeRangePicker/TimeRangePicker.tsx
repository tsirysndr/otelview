import { useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { Button } from "@heroui/react";
import { IconCalendar, IconX } from "@tabler/icons-react";
import { useAtom } from "jotai";
import dayjs from "dayjs";
import { DayPicker, type DateRange } from "react-day-picker";
import "react-day-picker/style.css";
import { customRangeAtom, lookbackAtom } from "../../state/atoms";
import { timeRangeSchema, type TimeRangeForm } from "../../lib/schemas";
import { plainTextField } from "../../lib/inputProps";

const LOOKBACKS = ["5m", "15m", "1h", "6h", "24h", "7d", "all"];

const STAMP = "MMM D, HH:mm";

function fmtRange(from: number, to: number): string {
  return `${dayjs(from).format(STAMP)} → ${dayjs(to).format(STAMP)}`;
}

function fmtTimeOf(ms: number): string {
  return dayjs(ms).format("HH:mm");
}

/** Combine a day from the calendar with an HH:MM typed into a time field.
 * The schema has already checked the shape, so the split is safe here. */
function at(day: Date, hhmm: string, endOfMinute: boolean): dayjs.Dayjs {
  const [h, m] = hhmm.trim().split(":").map(Number);
  return dayjs(day)
    .hour(h)
    .minute(m)
    .second(endOfMinute ? 59 : 0)
    .millisecond(endOfMinute ? 999 : 0);
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
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const {
    register,
    handleSubmit,
    reset,
    formState: { errors },
  } = useForm<TimeRangeForm>({
    resolver: zodResolver(timeRangeSchema),
    defaultValues: { fromTime: "00:00", toTime: "23:59" },
    // Validate as you type. With onBlur the error only cleared when the
    // field lost focus — which is the same event as reaching for Save, so
    // the message vanished, the layout shifted, and the click was swallowed.
    mode: "onChange",
  });

  const openPanel = () => {
    if (custom) {
      setRange({ from: new Date(custom.from), to: new Date(custom.to) });
      reset({ fromTime: fmtTimeOf(custom.from), toTime: fmtTimeOf(custom.to) });
    } else {
      const now = new Date();
      setRange({ from: now, to: now });
      reset({ fromTime: "00:00", toTime: "23:59" });
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

  // The calendar is not a form field, so the day range is still checked here;
  // the time fields are validated by the schema before this runs.
  const apply = handleSubmit(({ fromTime, toTime }) => {
    const fromDay = range?.from;
    const toDay = range?.to ?? range?.from;
    if (!fromDay || !toDay) return setError("pick a day range");
    const from = at(fromDay, fromTime, false);
    const to = at(toDay, toTime, true);
    if (!from.isBefore(to)) {
      return setError("that range is empty — pick a later end day");
    }
    setError(null);
    setCustom({ from: from.valueOf(), to: to.valueOf() });
    setOpen(false);
  });

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
          className={`otelview-rdp fixed inset-0 z-40 flex flex-col gap-2 overflow-y-auto bg-content1 p-3
            lg:absolute lg:inset-auto lg:top-9 lg:rounded-large lg:border lg:border-content3 ${
            align === "right" ? "lg:right-0" : "lg:left-0"
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
                placeholder="00:00"
                aria-label="Start time"
                aria-invalid={!!errors.fromTime}
                className={`${TIME_INPUT} ${errors.fromTime ? "border-danger" : ""}`}
                {...register("fromTime")}
              />
            </label>
            <label className="flex items-center gap-1.5 text-[10px] uppercase tracking-wide text-default-500">
              to
              <input
                {...plainTextField}
                placeholder="23:59"
                aria-label="End time"
                aria-invalid={!!errors.toTime}
                className={`${TIME_INPUT} ${errors.toTime ? "border-danger" : ""}`}
                {...register("toTime")}
              />
            </label>
          </div>
          {(errors.fromTime || errors.toTime || error) && (
            <p className="text-[11px] text-danger">
              {errors.fromTime?.message ?? errors.toTime?.message ?? error}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="light" radius="sm" onPress={() => setOpen(false)}>
              cancel
            </Button>
            <Button
              size="sm"
              color="secondary"
              variant="flat"
              radius="sm"
              onPress={() => void apply()}
            >
              apply
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

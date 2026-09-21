import { lazy, Suspense, useState } from "react";
import { IconCalendar, IconX } from "@tabler/icons-react";
import { useAtom } from "jotai";
import { customRangeAtom, lookbackAtom } from "../../state/atoms";
import { lookbackToRange } from "../../lib/lookback";
import type { Range } from "./CustomRangePicker";

// HeroUI's date stack is large and most sessions never leave the quick
// lookbacks, so it is fetched only when the custom picker is opened.
const CustomRangePicker = lazy(() => import("./CustomRangePicker"));

const LOOKBACKS = ["5m", "15m", "1h", "6h", "24h", "7d", "all"];

/** Quick lookback pills, with HeroUI's DateRangePicker behind them for an
 * absolute window.
 *
 * That picker covers the calendar, both ends of the range and the time of
 * day in one control, so there is no separate HH:MM field to parse or
 * order-check: `granularity="minute"` can only produce a well-formed
 * instant, the component enforces start <= end itself, and `maxValue` keeps
 * the range out of the future. */
export function TimeRangePicker() {
  const [lookback, setLookback] = useAtom(lookbackAtom);
  const [custom, setCustom] = useAtom(customRangeAtom);
  // Sticky while editing, so clearing the range does not yank the control
  // out from under the user mid-edit.
  const [picking, setPicking] = useState(false);
  // The calendar opens with the picker: one click on the icon, not two.
  const [calendarOpen, setCalendarOpen] = useState(false);
  // What the picker shows before anything is committed. Opening the picker
  // must not touch customRangeAtom: that atom feeds every time-scoped query
  // key, so writing to it refetches the whole view just to show a calendar.
  // The draft carries the window already on screen — identical to what is
  // displayed, and complete enough that picking days does not also demand
  // typing both times — without asking the server for any of it again.
  const [draft, setDraft] = useState<Range | null>(null);

  const startPicking = () => {
    setDraft(lookbackToRange(lookback, Date.now()));
    setPicking(true);
    setCalendarOpen(true);
  };

  // Only a real edit commits, and that refetch is the one the user asked
  // for by changing the range.
  const commit = (r: Range | null) => {
    setDraft(r);
    setCustom(r);
  };

  if (custom !== null || picking) {
    return (
      <div className="flex shrink-0 items-center gap-1">
        <Suspense
          fallback={<div className="h-7 w-72 animate-pulse rounded-medium bg-content2" />}
        >
          <CustomRangePicker
            value={custom ?? draft}
            onChange={commit}
            isOpen={calendarOpen}
            onOpenChange={setCalendarOpen}
          />
        </Suspense>
        <button
          onClick={() => {
            setCustom(null);
            setDraft(null);
            setPicking(false);
            setCalendarOpen(false);
          }}
          aria-label="Clear custom range"
          title="Back to quick ranges"
          className="shrink-0 rounded p-0.5 text-default-500 transition-colors hover:text-foreground"
        >
          <IconX size={14} />
        </button>
      </div>
    );
  }

  return (
    <div className="flex shrink-0 items-center gap-1 rounded-lg bg-content2 p-0.5">
      {LOOKBACKS.map((lb) => (
        <button
          key={lb}
          onClick={() => {
            setCustom(null);
            setDraft(null);
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
        onClick={startPicking}
        aria-label="Pick a custom time range"
        title="Custom time range"
        className="rounded-md px-1.5 py-0.5 text-default-500 transition-colors hover:text-foreground"
      >
        <IconCalendar size={14} />
      </button>
    </div>
  );
}

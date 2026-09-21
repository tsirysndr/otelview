import { lazy, Suspense, useEffect, useState } from "react";
import { IconCalendar, IconX } from "@tabler/icons-react";
import { useAtom } from "jotai";
import dayjs from "dayjs";
import { customRangeAtom, lookbackAtom } from "../../state/atoms";
import { lookbackToRange } from "../../lib/lookback";
import type { Range } from "./CustomRangePicker";

// HeroUI's date stack is large and most sessions never leave the quick
// lookbacks, so it is fetched only when the custom picker is opened.
const CustomRangePicker = lazy(() => import("./CustomRangePicker"));

/** Fetch the chunk before it is needed. Hovering the trigger is a good
 * enough signal and costs nothing for anyone who never opens the picker;
 * opening awaits the same promise, so a click that beats the hover still
 * mounts the real thing rather than a placeholder. */
const importPicker = () => import("./CustomRangePicker");

const LOOKBACKS = ["5m", "15m", "1h", "6h", "24h", "7d", "all"];

const STAMP = "MMM D, HH:mm";

function fmtRange(from: number, to: number): string {
  return `${dayjs(from).format(STAMP)} → ${dayjs(to).format(STAMP)}`;
}

/** Quick lookback pills, with HeroUI's DateRangePicker behind them for an
 * absolute window.
 *
 * That picker covers the calendar, both ends of the range and the time of
 * day in one control, so there is no separate HH:MM field to parse or
 * order-check: `granularity="minute"` can only produce a well-formed
 * instant, the component enforces start <= end itself, and `maxValue` keeps
 * the range out of the future.
 *
 * Once a range is settled the editable segments give way to a compact
 * summary chip: the bar spends most of its life displaying a range rather
 * than editing one, and a row of date segments is a lot of chrome to carry
 * for that. Clicking the chip brings the picker back. */
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
  // The chunk is fetched before the picker is rendered, so the trigger needs
  // to show that the click registered. Dimming it keeps the bar's layout
  // still — a spinner or a placeholder row is what made this flicker.
  const [loading, setLoading] = useState(false);

  const openPicker = async (seed: Range | null) => {
    setDraft(seed);
    // Swap in only once the chunk is in memory. Rendering first showed a
    // placeholder until the 180 kB chunk arrived — on a cold load that is
    // most of a second of pulsing grey, which is the flicker. Already
    // resolved after a hover, so this is usually instant.
    setLoading(true);
    await importPicker();
    setLoading(false);
    setPicking(true);
  };

  // Open on the frame *after* the picker mounts. Opening in the same render
  // positions the popover against a trigger that has not been laid out yet,
  // so it appears offset and undersized and then jumps into place — the
  // flicker. One frame later the field has its real geometry.
  useEffect(() => {
    if (!picking) return;
    const id = requestAnimationFrame(() => setCalendarOpen(true));
    return () => cancelAnimationFrame(id);
  }, [picking]);

  const reset = () => {
    setCustom(null);
    setDraft(null);
    setPicking(false);
    setCalendarOpen(false);
  };

  // Only a real edit commits, and that refetch is the one the user asked
  // for by changing the range.
  const commit = (r: Range | null) => {
    setDraft(r);
    setCustom(r);
  };

  if (picking) {
    return (
      <div className="flex shrink-0 items-center gap-1">
        <Suspense
          // Reached only if the chunk is somehow not cached by now; sized
          // like the field so the bar does not jump.
          fallback={<div className="h-8 w-[21rem]" />}
        >
          <CustomRangePicker
            value={custom ?? draft}
            onChange={commit}
            isOpen={calendarOpen}
            onOpenChange={(open) => {
              setCalendarOpen(open);
              // Dismissing the calendar settles the range: fall back to the
              // summary chip, or to the pills if nothing was chosen.
              if (!open) setPicking(false);
            }}
          />
        </Suspense>
        <button
          onClick={reset}
          aria-label="Clear custom range"
          title="Back to quick ranges"
          className="shrink-0 rounded p-0.5 text-default-500 transition-colors hover:text-foreground"
        >
          <IconX size={14} />
        </button>
      </div>
    );
  }

  if (custom !== null) {
    return (
      <button
        onClick={() => openPicker(custom)}
        onMouseEnter={importPicker}
        onFocus={importPicker}
        aria-label={`Edit custom range: ${fmtRange(custom.from, custom.to)}`}
        className="flex shrink-0 items-center gap-1.5 rounded-lg bg-content2 px-2 py-1 text-xs text-neon-cyan"
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
            reset();
          }}
        >
          <IconX size={12} />
        </span>
      </button>
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
        onClick={() => openPicker(lookbackToRange(lookback, Date.now()))}
        onMouseEnter={importPicker}
        onFocus={importPicker}
        aria-label="Pick a custom time range"
        title="Custom time range"
        className={`rounded-md px-1.5 py-0.5 transition-colors ${
          loading ? "text-default-300" : "text-default-500 hover:text-foreground"
        }`}
      >
        <IconCalendar size={14} />
      </button>
    </div>
  );
}

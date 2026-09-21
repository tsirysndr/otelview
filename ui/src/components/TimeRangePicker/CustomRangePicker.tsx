import { DateRangePicker } from "@heroui/react";
// @internationalized/date is pinned to an exact version in package.json,
// matching what HeroUI itself depends on. A caret range floats to a newer
// patch, npm/bun then give HeroUI its own nested copy, and the two
// `ZonedDateTime` classes become nominally distinct types — so the value
// built here no longer type-checks against the picker that receives it.
import {
  fromDate,
  getLocalTimeZone,
  now,
  type ZonedDateTime,
} from "@internationalized/date";

const zone = () => getLocalTimeZone();
const toZoned = (ms: number): ZonedDateTime => fromDate(new Date(ms), zone());

export interface Range {
  from: number;
  to: number;
}

/** The absolute-range control, split into its own module so the date stack
 * it pulls in — calendar, date-input, the @internationalized/date machinery
 * — is code-split and only fetched when somebody actually opens it. Most
 * sessions never leave the quick lookbacks. */
export default function CustomRangePicker({
  value,
  onChange,
  isOpen,
  onOpenChange,
}: {
  value: Range | null;
  onChange: (r: Range | null) => void;
  /** Controlled so the calendar can be shown the moment the bar switches
   * into custom mode, rather than needing a second click on the field. */
  isOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  return (
    <DateRangePicker
      aria-label="Custom time range"
      isOpen={isOpen}
      onOpenChange={onOpenChange}
      size="sm"
      variant="flat"
      granularity="minute"
      // Two months: a range usually spans a boundary, and paging back and
      // forth to place the two ends is the main friction with one.
      visibleMonths={2}
      hideTimeZone
      maxValue={now(zone())}
      // The trigger has done its job once the calendar is up; leaving it
      // there just offers a second way to toggle what is already open.
      // Inline rather than a `hidden` class: the component supplies its own
      // display utility, which wins the cascade against Tailwind's.
      selectorButtonProps={{ style: isOpen ? { display: "none" } : undefined }}
      // No scale-and-fade entrance: a popover this size grows visibly into
      // place, which reads as a flicker rather than as motion.
      popoverProps={{ disableAnimation: true }}
      value={value ? { start: toZoned(value.from), end: toZoned(value.to) } : null}
      onChange={(v) => {
        if (!v?.start || !v?.end) return onChange(null);
        onChange({ from: v.start.toDate().getTime(), to: v.end.toDate().getTime() });
      }}
      classNames={{
        base: "w-auto",
        inputWrapper: "h-7 min-h-7 bg-content2",
        input: "text-xs",
      }}
    />
  );
}

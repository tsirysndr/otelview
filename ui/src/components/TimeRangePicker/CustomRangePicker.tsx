import { DateRangePicker } from "@heroui/react";
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
}: {
  value: Range | null;
  onChange: (r: Range | null) => void;
}) {
  return (
    <DateRangePicker
      aria-label="Custom time range"
      size="sm"
      variant="flat"
      granularity="minute"
      hideTimeZone
      maxValue={now(zone())}
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

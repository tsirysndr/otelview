import { useAtomValue } from "jotai";
import { customRangeAtom, lookbackAtom } from "../state/atoms";

export interface TimeParams {
  lookback?: string;
  start_ms?: number;
  end_ms?: number;
}

/** Time filter for API queries: the absolute custom range when one is set,
 * otherwise the relative lookback window.
 *
 * The bounds are floored here rather than trusted from the caller: the API
 * parses them as integers, and a fractional millisecond — easy to produce by
 * dividing nanoseconds — is rejected as a malformed query string rather than
 * being truncated. Doing it at this one chokepoint covers every producer of a
 * custom range. */
export function useTimeParams(): TimeParams {
  const lookback = useAtomValue(lookbackAtom);
  const custom = useAtomValue(customRangeAtom);
  return custom
    ? { start_ms: Math.floor(custom.from), end_ms: Math.floor(custom.to) }
    : { lookback };
}

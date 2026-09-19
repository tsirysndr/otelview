import { useAtomValue } from "jotai";
import { customRangeAtom, lookbackAtom } from "../state/atoms";

export interface TimeParams {
  lookback?: string;
  start_ms?: number;
  end_ms?: number;
}

/** Time filter for API queries: the absolute custom range when one is set,
 * otherwise the relative lookback window. */
export function useTimeParams(): TimeParams {
  const lookback = useAtomValue(lookbackAtom);
  const custom = useAtomValue(customRangeAtom);
  return custom ? { start_ms: custom.from, end_ms: custom.to } : { lookback };
}

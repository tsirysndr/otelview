import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { createStore, Provider } from "jotai";
import { createElement, type ReactNode } from "react";
import { useTimeParams } from "./useTimeParams";
import { customRangeAtom } from "../state/atoms";

function wrapperFor(store: ReturnType<typeof createStore>) {
  return ({ children }: { children: ReactNode }) =>
    createElement(Provider, { store }, children);
}

describe("useTimeParams", () => {
  it("floors fractional bounds so the API never sees a decimal", () => {
    // Regression guard: correlation jumps derive the window from a
    // nanosecond timestamp, and nanos do not divide cleanly into millis.
    // The unrounded value serialized as "1758397850123.4568" and the server
    // rejected the whole request with "invalid digit found in string".
    const store = createStore();
    store.set(customRangeAtom, { from: 1758397850123.4568, to: 1758399650123.9 });

    const { result } = renderHook(() => useTimeParams(), {
      wrapper: wrapperFor(store),
    });

    expect(result.current.start_ms).toBe(1758397850123);
    expect(result.current.end_ms).toBe(1758399650123);
    expect(Number.isInteger(result.current.start_ms)).toBe(true);
    expect(Number.isInteger(result.current.end_ms)).toBe(true);
    expect(String(result.current.start_ms)).not.toContain(".");
  });

  it("passes integer bounds through untouched", () => {
    const store = createStore();
    store.set(customRangeAtom, { from: 1000, to: 2000 });
    const { result } = renderHook(() => useTimeParams(), {
      wrapper: wrapperFor(store),
    });
    expect(result.current).toEqual({ start_ms: 1000, end_ms: 2000 });
  });

  it("falls back to the lookback when no custom range is set", () => {
    const store = createStore();
    const { result } = renderHook(() => useTimeParams(), {
      wrapper: wrapperFor(store),
    });
    expect(result.current.lookback).toBeTruthy();
    expect(result.current.start_ms).toBeUndefined();
  });
});

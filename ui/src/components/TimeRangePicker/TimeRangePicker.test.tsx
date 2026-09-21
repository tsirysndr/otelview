import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { TimeRangePicker } from "./TimeRangePicker";
import { customRangeAtom, lookbackAtom } from "../../state/atoms";
import { renderApp } from "../../test/utils";

describe("time range picker", () => {
  // Regression guard. customRangeAtom feeds every time-scoped query key, so
  // writing to it refetches traces, logs, metrics and the service graph at
  // once. Opening the picker used to seed it with the current window — the
  // range was identical, but the whole view reloaded just to show a
  // calendar. The seeded window now lives in local draft state until the
  // user actually edits it.
  it("opening the picker does not touch the shared range", async () => {
    const { store } = renderApp(<TimeRangePicker />);
    expect(store.get(customRangeAtom)).toBeNull();

    await userEvent.click(
      screen.getByRole("button", { name: "Pick a custom time range" }),
    );

    // The picker is showing...
    expect(await screen.findByLabelText("Custom time range")).toBeInTheDocument();
    // ...but nothing that drives a query key has changed.
    expect(store.get(customRangeAtom)).toBeNull();
    expect(store.get(lookbackAtom)).toBe("1h");
  });

  // Once a range is settled the editable segments give way to the compact
  // coloured summary. These drive that chip directly rather than through
  // HeroUI's popover, whose focus and blur behaviour does not survive jsdom
  // faithfully — the popover itself is covered in a real browser.
  it("shows the settled range as a summary chip", async () => {
    const { store } = renderApp(<TimeRangePicker />);
    store.set(customRangeAtom, { from: Date.parse("2026-09-08T17:35:00Z"), to: Date.parse("2026-09-15T18:35:00Z") });

    const chip = await screen.findByRole("button", { name: /Edit custom range:/ });
    expect(chip).toHaveTextContent("→");
    // The old colouring, not the default input styling.
    expect(chip.className).toMatch(/text-neon-cyan/);
    // The quick lookbacks step aside while a range is set.
    expect(screen.queryByRole("button", { name: "1h" })).not.toBeInTheDocument();
  });

  it("clearing from the chip returns to the quick lookbacks", async () => {
    const { store } = renderApp(<TimeRangePicker />);
    store.set(customRangeAtom, { from: 1_000, to: 2_000 });
    await screen.findByRole("button", { name: /Edit custom range:/ });

    await userEvent.click(screen.getByRole("button", { name: "Clear custom range" }));

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "1h" })).toBeInTheDocument(),
    );
    expect(store.get(customRangeAtom)).toBeNull();
  });

  it("choosing a lookback replaces the custom range", async () => {
    const { store } = renderApp(<TimeRangePicker />);
    store.set(customRangeAtom, { from: 1_000, to: 2_000 });
    await screen.findByRole("button", { name: /Edit custom range:/ });

    await userEvent.click(screen.getByRole("button", { name: "Clear custom range" }));
    await userEvent.click(await screen.findByRole("button", { name: "6h" }));

    expect(store.get(lookbackAtom)).toBe("6h");
    expect(store.get(customRangeAtom)).toBeNull();
  });
});

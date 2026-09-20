import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { TopBar } from "./TopBar";
import { renderApp } from "../../test/utils";

describe("top bar overflow", () => {
  // Regression guard. The bar scrolls horizontally on narrow screens, but a
  // scroll container clips absolutely-positioned descendants — and the
  // time-range popover lives inside this bar. When that clipping was left on
  // at desktop widths the calendar silently stopped rendering: still in the
  // DOM, still "visible" to a query, just painted nowhere. jsdom cannot see
  // clipping, so the invariant is asserted on the classes instead.
  it("does not establish a scroll container at desktop widths", () => {
    renderApp(<TopBar />);
    const header = screen.getByRole("banner");
    if (/(^|\s|:)overflow-(x-)?(auto|scroll|hidden)/.test(header.className)) {
      expect(header.className).toMatch(/lg:overflow-(visible|x-visible)/);
    }
  });

  it("keeps the time-range popover inside the bar reachable", async () => {
    renderApp(<TopBar />);
    await userEvent.click(screen.getByRole("button", { name: "Custom time range" }));
    // The popover renders as a sibling within the bar's subtree; if it ever
    // moves to a portal this assertion should be revisited, not deleted.
    expect(await screen.findByLabelText("Start time")).toBeInTheDocument();
  });
});

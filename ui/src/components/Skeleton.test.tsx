import { screen, waitForElementToBeRemoved } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { LogsView } from "../views/LogsView";
import { renderApp } from "../test/utils";

describe("loading skeletons", () => {
  it("stands in for the log list until the first data arrives", async () => {
    renderApp(<LogsView />);

    // Present before the request resolves…
    const skeleton = screen.getByLabelText("loading logs");
    expect(skeleton).toBeInTheDocument();
    expect(skeleton).toHaveAttribute("aria-busy", "true");

    // …and gone once there is something real to show.
    await waitForElementToBeRemoved(() => screen.queryByLabelText("loading logs"));
    expect(
      await screen.findByText("payment failed: card declined"),
    ).toBeInTheDocument();
  });

  it("takes its colours from theme tokens rather than fixed values", () => {
    renderApp(<LogsView />);

    const skeleton = screen.getByLabelText("loading logs");

    // The placeholder has to follow the active theme, so it may only carry
    // semantic classes — a literal colour would be wrong in one of the two
    // themes and could not be caught by eye in the other.
    const bars = skeleton.querySelectorAll('[class*="bg-default-"]');
    expect(bars.length).toBeGreaterThan(0);

    const everyClass = Array.from(skeleton.querySelectorAll("*"))
      .map((el) => el.className)
      .join(" ");
    expect(everyClass).not.toMatch(/#[0-9a-fA-F]{3,6}\b/);
    expect(everyClass).not.toMatch(/\b(rgb|hsl)a?\(/);

    // The sweep is what makes it read as loading rather than as empty rows.
    expect(skeleton.querySelectorAll(".animate-shimmer").length).toBeGreaterThan(0);
  });
});

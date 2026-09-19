import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { LogsView } from "./LogsView";
import { renderApp } from "../test/utils";

describe("LogsView", () => {
  it("renders log records with severity levels", async () => {
    renderApp(<LogsView />);
    expect(
      await screen.findByText("payment failed: card declined"),
    ).toBeInTheDocument();
    expect(screen.getByText("ERROR")).toBeInTheDocument();
    // "payments" also appears as a service filter option
    expect(screen.getAllByText("payments").length).toBeGreaterThan(0);
  });
});

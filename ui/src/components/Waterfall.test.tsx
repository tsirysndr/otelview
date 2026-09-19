import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { Waterfall } from "./Waterfall";
import { spanFixtures } from "../test/fixtures";
import { renderApp } from "../test/utils";

describe("Waterfall", () => {
  it("renders one row per span with durations", () => {
    renderApp(<Waterfall spans={spanFixtures} />);
    expect(screen.getByText("GET /checkout")).toBeInTheDocument();
    expect(screen.getByText("SELECT orders")).toBeInTheDocument();
    expect(screen.getByText("2 spans · 500.0ms")).toBeInTheDocument();
  });

  it("collapses children", async () => {
    renderApp(<Waterfall spans={spanFixtures} />);
    expect(screen.getByText("SELECT orders")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "collapse" }));
    expect(screen.queryByText("SELECT orders")).not.toBeInTheDocument();
  });
});

import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { TracesView } from "./TracesView";
import { renderApp } from "../test/utils";

describe("TracesView", () => {
  it("lists traces from the API (via msw) and opens the waterfall", async () => {
    renderApp(<TracesView />);
    // summary row ("GET /checkout" also appears as an operation filter
    // option, so anchor on row-only content)
    expect(await screen.findByText("2 spans")).toBeInTheDocument();
    expect(screen.getByText("1 err")).toBeInTheDocument();
    const rows = await screen.findAllByText("GET /checkout");
    expect(rows.length).toBeGreaterThan(0);

    // open trace → waterfall shows both spans
    await userEvent.click(screen.getByText("2 spans").closest("button")!);
    expect(await screen.findByText("SELECT orders")).toBeInTheDocument();
  });
});

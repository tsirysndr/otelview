import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import App from "../../App";
import { renderApp } from "../../test/utils";

describe("keyboard shortcuts help", () => {
  it("pressing ? opens the shortcuts dialog, Escape closes it", async () => {
    renderApp(<App />);
    await userEvent.keyboard("?");
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(/keyboard shortcuts/i);
    expect(dialog).toHaveTextContent("Toggle inspector panel");
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { FilterAutocomplete } from "./FilterAutocomplete";
import { renderApp } from "../../test/utils";

const OPTIONS = [
  { value: "", label: "all services" },
  { value: "gateway", label: "gateway" },
  { value: "payments", label: "payments" },
];

function setup(value = "") {
  const onChange = vi.fn();
  renderApp(
    <FilterAutocomplete
      value={value}
      onChange={onChange}
      options={OPTIONS}
      ariaLabel="Service"
    />,
  );
  return { onChange, box: screen.getByRole("combobox", { name: "Service" }) };
}

describe("FilterAutocomplete", () => {
  it("shows the catch-all as the placeholder, not as a row", async () => {
    const { box } = setup();
    expect(box).toHaveAttribute("placeholder", "all services");

    await userEvent.click(box);
    const names = (await screen.findAllByRole("option")).map((o) => o.textContent);
    expect(names).toEqual(["gateway", "payments"]);
  });

  it("filters as you type and reports the chosen value", async () => {
    const { onChange, box } = setup();
    await userEvent.click(box);
    await userEvent.type(box, "pay");

    await waitFor(async () =>
      expect(await screen.findAllByRole("option")).toHaveLength(1),
    );
    await userEvent.click(screen.getByRole("option", { name: "payments" }));
    expect(onChange).toHaveBeenCalledWith("payments");
  });

  // Clearing empties the text without reporting a selection change, so the
  // filter would stay applied while the box looked reset.
  it("clearing reports the catch-all", async () => {
    const { onChange } = setup("payments");
    const clear = screen
      .getAllByRole("button")
      .find((b) => b.getAttribute("aria-label") !== "Show suggestions");
    expect(clear, "a clear button should be offered once a value is set").toBeDefined();

    await userEvent.click(clear!);
    expect(onChange).toHaveBeenCalledWith("");
  });
});

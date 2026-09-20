import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { SettingsView } from "./SettingsView";
import { renderApp } from "../test/utils";

function stored() {
  return JSON.parse(localStorage.getItem("otelview.api") ?? "null");
}

describe("server profile validation", () => {
  beforeEach(() => localStorage.clear());

  it("refuses to save a malformed base URL", async () => {
    renderApp(<SettingsView />);
    const url = screen.getByLabelText(/API base URL/);

    await userEvent.type(url, "htp://oops:4319");
    await userEvent.click(screen.getByRole("button", { name: "save" }));

    expect(await screen.findByText(/must be an http\(s\) URL/)).toBeInTheDocument();
    // Nothing reached storage, so the client is not pointed at a dead URL.
    expect(stored()).toBeNull();
  });

  it("refuses to save a nameless profile", async () => {
    renderApp(<SettingsView />);
    const name = screen.getByLabelText(/Profile name/);

    await userEvent.clear(name);
    await userEvent.type(name, "   ");
    await userEvent.click(screen.getByRole("button", { name: "save" }));

    expect(await screen.findByText(/give this server a name/)).toBeInTheDocument();
    expect(stored()).toBeNull();
  });

  it("clears the error as you fix it, and saves on the first click", async () => {
    // Regression guard. With validation on blur, the error only cleared when
    // the field lost focus — the same event as reaching for Save. The message
    // disappeared, the layout shifted under the pointer, and the click was
    // swallowed, so a corrected URL silently failed to save until clicked
    // twice.
    renderApp(<SettingsView />);
    const url = screen.getByLabelText(/API base URL/);
    const save = screen.getByRole("button", { name: "save" });

    await userEvent.type(url, "htp://oops");
    await userEvent.click(save);
    expect(await screen.findByText(/must be an http\(s\) URL/)).toBeInTheDocument();

    await userEvent.clear(url);
    await userEvent.type(url, "http://127.0.0.1:4319");
    // Gone before the pointer ever reaches the button.
    await waitFor(() =>
      expect(screen.queryByText(/must be an http\(s\) URL/)).not.toBeInTheDocument(),
    );

    await userEvent.click(save);
    await waitFor(() => expect(stored()).not.toBeNull());
    expect(stored().profiles[0].baseUrl).toBe("http://127.0.0.1:4319");
  });

  it("saves a valid profile, and accepts an empty URL as same-origin", async () => {
    renderApp(<SettingsView />);
    await userEvent.type(screen.getByLabelText(/Profile name/), " prod");
    await userEvent.click(screen.getByRole("button", { name: "save" }));

    await waitFor(() => expect(stored()).not.toBeNull());
    const s = stored();
    expect(s.profiles[0].name).toBe("this server prod");
    expect(s.profiles[0].baseUrl).toBe("");
  });

  it("accepts a well-formed absolute URL", async () => {
    renderApp(<SettingsView />);
    await userEvent.type(
      screen.getByLabelText(/API base URL/),
      "https://otel.example.com:4319",
    );
    await userEvent.click(screen.getByRole("button", { name: "save" }));

    await waitFor(() => expect(stored()).not.toBeNull());
    expect(stored().profiles[0].baseUrl).toBe("https://otel.example.com:4319");
  });
});

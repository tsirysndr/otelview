import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { http, HttpResponse } from "msw";
import { LogsView } from "./LogsView";
import { TracesView } from "./TracesView";
import { server } from "../test/server";
import { logFixtures, traceSummaryFixtures } from "../test/fixtures";
import { renderApp } from "../test/utils";

// Capture the query params of every /api request the views fire.
let logCalls: URLSearchParams[] = [];
let traceCalls: URLSearchParams[] = [];

beforeEach(() => {
  logCalls = [];
  traceCalls = [];
  server.use(
    http.get("/api/logs", ({ request }) => {
      logCalls.push(new URL(request.url).searchParams);
      return HttpResponse.json(logFixtures);
    }),
    http.get("/api/traces", ({ request }) => {
      traceCalls.push(new URL(request.url).searchParams);
      return HttpResponse.json(traceSummaryFixtures);
    }),
  );
});

describe("filter wiring", () => {
  it("severity select sends min_severity to the API", async () => {
    renderApp(<LogsView />);
    await waitFor(() => expect(logCalls.length).toBeGreaterThan(0));
    expect(logCalls.at(-1)!.get("min_severity")).toBeNull();

    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Minimum severity" }),
      "17",
    );
    await waitFor(() =>
      expect(logCalls.at(-1)!.get("min_severity")).toBe("17"),
    );
  });

  it("log service select sends service to the API", async () => {
    renderApp(<LogsView />);
    await waitFor(() => expect(logCalls.length).toBeGreaterThan(0));
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Service" }),
      "payments",
    );
    await waitFor(() => expect(logCalls.at(-1)!.get("service")).toBe("payments"));
  });

  it("trace service + errors-only filters reach the API", async () => {
    renderApp(<TracesView />);
    await waitFor(() => expect(traceCalls.length).toBeGreaterThan(0));

    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Service" }),
      "frontend",
    );
    await waitFor(() => expect(traceCalls.at(-1)!.get("service")).toBe("frontend"));

    await userEvent.click(screen.getByRole("switch", { name: "Errors only" }));
    await waitFor(() =>
      expect(traceCalls.at(-1)!.get("errors_only")).toBe("true"),
    );
  });
});

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

  it("min duration input sends min_duration_ms to the API", async () => {
    renderApp(<TracesView />);
    await waitFor(() => expect(traceCalls.length).toBeGreaterThan(0));
    await userEvent.type(
      screen.getByRole("textbox", { name: "Minimum duration in milliseconds" }),
      "300",
    );
    await waitFor(() =>
      expect(traceCalls.at(-1)!.get("min_duration_ms")).toBe("300"),
    );
  });

  it("the traceql mode sends traceql and drops the attribute query", async () => {
    renderApp(<TracesView />);
    await waitFor(() => expect(traceCalls.length).toBeGreaterThan(0));

    // Attribute mode is the default and uses `q`.
    await userEvent.type(
      screen.getByRole("textbox", { name: "Attribute filter" }),
      "http.method=GET",
    );
    await waitFor(() => expect(traceCalls.at(-1)!.get("q")).toBe("http.method=GET"));

    await userEvent.click(screen.getByRole("button", { name: "traceql" }));
    await userEvent.type(
      screen.getByRole("textbox", { name: "TraceQL query" }),
      "{{ status = error }",
    );
    await waitFor(() =>
      expect(traceCalls.at(-1)!.get("traceql")).toBe("{ status = error }"),
    );
    // The attribute query must not tag along once TraceQL is driving.
    expect(traceCalls.at(-1)!.get("q")).toBeNull();

    // Switching back restores the attribute query and drops traceql.
    await userEvent.click(screen.getByRole("button", { name: "attributes" }));
    await waitFor(() => expect(traceCalls.at(-1)!.get("q")).toBe("http.method=GET"));
    expect(traceCalls.at(-1)!.get("traceql")).toBeNull();
  });

  it("surfaces a TraceQL parse error from the API", async () => {
    server.use(
      http.get("/api/traces", ({ request }) => {
        const url = new URL(request.url);
        traceCalls.push(url.searchParams);
        if (url.searchParams.get("traceql")) {
          return new HttpResponse("invalid TraceQL query: expected '}'", {
            status: 400,
          });
        }
        return HttpResponse.json(traceSummaryFixtures);
      }),
    );
    renderApp(<TracesView />);
    await waitFor(() => expect(traceCalls.length).toBeGreaterThan(0));

    await userEvent.click(screen.getByRole("button", { name: "traceql" }));
    await userEvent.type(
      screen.getByRole("textbox", { name: "TraceQL query" }),
      "{{ status = ",
    );
    expect(await screen.findByText(/invalid TraceQL query/)).toBeInTheDocument();
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

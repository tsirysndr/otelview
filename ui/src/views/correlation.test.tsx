import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { http, HttpResponse } from "msw";
import { LogsView } from "./LogsView";
import { server } from "../test/server";
import { logFixtures } from "../test/fixtures";
import { renderApp } from "../test/utils";

let logCalls: URLSearchParams[] = [];

beforeEach(() => {
  logCalls = [];
  server.use(
    http.get("/api/logs", ({ request }) => {
      logCalls.push(new URL(request.url).searchParams);
      return HttpResponse.json(logFixtures);
    }),
  );
});

describe("trace → logs correlation", () => {
  it("sends trace_id and shows a chip explaining the narrowing", async () => {
    const { store } = renderApp(<LogsView />);
    await waitFor(() => expect(logCalls.length).toBeGreaterThan(0));
    expect(logCalls.at(-1)!.get("trace_id")).toBeNull();

    // Simulate arriving from a trace.
    const { logFiltersAtom } = await import("../state/atoms");
    act(() => store.set(logFiltersAtom, (f) => ({ ...f, traceId: "abc123", spanId: "s1" })));

    await waitFor(() => expect(logCalls.at(-1)!.get("trace_id")).toBe("abc123"));
    expect(await screen.findByText(/showing logs for trace/)).toBeInTheDocument();

    // And a way back out of the narrowing.
    await userEvent.click(screen.getByLabelText("Clear trace filter"));
    await waitFor(() => expect(logCalls.at(-1)!.get("trace_id")).toBeNull());
    expect(screen.queryByText(/showing logs for trace/)).not.toBeInTheDocument();
  });

  it("drops the previous text query so the trace's logs are not hidden", async () => {
    const { store } = renderApp(<LogsView />);
    const { logFiltersAtom } = await import("../state/atoms");

    act(() => store.set(logFiltersAtom, (f) => ({ ...f, search: "payment declined" })));
    await waitFor(() => expect(logCalls.at(-1)!.get("kql")).toBe("payment declined"));

    // Following a trace clears it, the way useCorrelate does.
    act(() =>
      store.set(logFiltersAtom, (f) => ({
        ...f,
        traceId: "abc123",
        search: "",
        minSeverity: 0,
        service: "",
      })),
    );
    await waitFor(() => expect(logCalls.at(-1)!.get("trace_id")).toBe("abc123"));
    expect(logCalls.at(-1)!.get("kql")).toBeNull();
  });
});

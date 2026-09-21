import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { createStore, Provider } from "jotai";
import { HighlightedInput } from "../HighlightedInput";
import { withAppliedQuery } from "../../lib/history";

// Each render gets its own jotai store so tests don't leak history into one
// another through the shared atomFamily.
function Harness({ historyKey }: { historyKey?: string }) {
  const [value, setValue] = useState("");
  return (
    <Provider store={createStore()}>
      <HighlightedInput
        value={value}
        onChange={setValue}
        renderTokens={(v) => [{ text: v, className: "" }]}
        suggest={() => ({ from: 0, items: [] })}
        ariaLabel="test query"
        historyKey={historyKey}
      />
    </Provider>
  );
}

/** Same harness, but with a grammar that answers the empty input — the case
 * TraceQL and Lucene use for their starter queries. */
function TemplateHarness() {
  const [value, setValue] = useState("");
  return (
    <Provider store={createStore()}>
      <HighlightedInput
        value={value}
        onChange={setValue}
        renderTokens={(v) => [{ text: v, className: "" }]}
        suggest={() => ({
          from: 0,
          items: [{ label: "{ status = error }", insert: "{ status = error }" }],
        })}
        ariaLabel="test query"
        historyKey="tpl"
      />
    </Provider>
  );
}

describe("empty-input suggestions", () => {
  beforeEach(() => localStorage.clear());

  // The grammar's starter queries are the only place a language's syntax is
  // ever shown, so an empty box must reach the grammar, not just history.
  it("offers the grammar's starters when there is nothing to recall", () => {
    render(<TemplateHarness />);
    fireEvent.focus(screen.getByLabelText("test query"));
    expect(screen.getByText("{ status = error }")).toBeInTheDocument();
  });

  it("puts recalled queries ahead of the starters", () => {
    localStorage.setItem("otelview.history.tpl", JSON.stringify(["{ kind = server }"]));
    render(<TemplateHarness />);
    fireEvent.focus(screen.getByLabelText("test query"));
    const recalled = screen.getByText("{ kind = server }");
    const starter = screen.getByText("{ status = error }");
    // Node.DOCUMENT_POSITION_FOLLOWING — the starter comes after the recall.
    expect(recalled.compareDocumentPosition(starter) & 4).toBeTruthy();
  });
});

describe("query history merge", () => {
  it("dedupes to the front and caps", () => {
    let history: string[] = [];
    for (let i = 0; i < 20; i++) history = withAppliedQuery(history, `query ${i}`);
    history = withAppliedQuery(history, "query 10");
    expect(history[0]).toBe("query 10");
    expect(history.length).toBe(15);
    expect(history.filter((q) => q === "query 10").length).toBe(1);
  });

  it("ignores blank queries", () => {
    expect(withAppliedQuery(["a"], "   ")).toEqual(["a"]);
  });
});

describe("query history (jotai + localStorage)", () => {
  beforeEach(() => localStorage.clear());

  it("survives malformed storage", () => {
    localStorage.setItem("otelview.history.k", "not json at all");
    render(<Harness historyKey="k" />);
    const input = screen.getByLabelText("test query");
    fireEvent.focus(input);
    expect(screen.queryByText("recent")).not.toBeInTheDocument();
  });

  it("records an applied query, persists it, and offers it back on a fresh mount", () => {
    const { unmount } = render(<Harness historyKey="t" />);
    const input = screen.getByLabelText("test query");

    fireEvent.change(input, { target: { value: "status_code:>=500" } });
    fireEvent.keyDown(input, { key: "Enter" });
    unmount();

    // A fresh mount (new store, same key) reads back what localStorage has.
    render(<Harness historyKey="t" />);
    const freshInput = screen.getByLabelText("test query");
    fireEvent.focus(freshInput);
    expect(screen.getByText("status_code:>=500")).toBeInTheDocument();
    expect(screen.getByText("recent")).toBeInTheDocument();

    // Clicking it applies it wholesale.
    fireEvent.mouseDown(screen.getByText("status_code:>=500"));
    expect(freshInput).toHaveValue("status_code:>=500");
  });

  it("suggests matching history entries while typing, not just when empty", () => {
    localStorage.setItem(
      "otelview.history.m",
      JSON.stringify(["http.method=GET", "service.name=checkout"]),
    );
    render(<Harness historyKey="m" />);
    const input = screen.getByLabelText("test query");
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "http" } });
    expect(screen.getByText("http.method=GET")).toBeInTheDocument();
    expect(screen.queryByText("service.name=checkout")).not.toBeInTheDocument();
  });

  it("clears history from the dropdown at any time", () => {
    localStorage.setItem("otelview.history.clr", JSON.stringify(["old query"]));
    render(<Harness historyKey="clr" />);
    const input = screen.getByLabelText("test query");
    fireEvent.focus(input);
    expect(screen.getByText("old query")).toBeInTheDocument();

    fireEvent.mouseDown(screen.getByLabelText("Clear history"));
    expect(screen.queryByText("old query")).not.toBeInTheDocument();
    expect(localStorage.getItem("otelview.history.clr")).toBeNull();
  });
});

describe("clear button", () => {
  it("appears with text, clears on click, and refocuses", () => {
    render(<Harness />);
    const input = screen.getByLabelText("test query");
    expect(screen.queryByLabelText("Clear query")).not.toBeInTheDocument();

    fireEvent.change(input, { target: { value: "some query" } });
    const clear = screen.getByLabelText("Clear query");

    fireEvent.mouseDown(clear);
    expect(input).toHaveValue("");
    expect(input).toHaveFocus();
  });

  it("does not record cleared text as applied on blur", () => {
    localStorage.clear();
    render(<Harness historyKey="c" />);
    const input = screen.getByLabelText("test query");
    fireEvent.change(input, { target: { value: "typo" } });
    fireEvent.mouseDown(screen.getByLabelText("Clear query"));
    vi.useFakeTimers();
    fireEvent.blur(input);
    vi.runAllTimers();
    vi.useRealTimers();
    expect(localStorage.getItem("otelview.history.c")).toBeNull();
  });
});

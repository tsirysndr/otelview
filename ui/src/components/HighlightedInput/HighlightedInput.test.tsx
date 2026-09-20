import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { HighlightedInput } from "../HighlightedInput";
import { loadHistory, pushHistory } from "../../lib/history";

function Harness({ historyKey }: { historyKey?: string }) {
  const [value, setValue] = useState("");
  return (
    <HighlightedInput
      value={value}
      onChange={setValue}
      renderTokens={(v) => [{ text: v, className: "" }]}
      suggest={() => ({ from: 0, items: [] })}
      ariaLabel="test query"
      historyKey={historyKey}
    />
  );
}

describe("query history", () => {
  beforeEach(() => localStorage.clear());

  it("dedupes to the front and caps", () => {
    for (let i = 0; i < 20; i++) pushHistory("k", `query ${i}`);
    pushHistory("k", "query 10");
    const h = loadHistory("k");
    expect(h[0]).toBe("query 10");
    expect(h.length).toBe(15);
    expect(h.filter((q) => q === "query 10").length).toBe(1);
  });

  it("survives malformed storage", () => {
    localStorage.setItem("otelview.history.k", "not json at all");
    expect(loadHistory("k")).toEqual([]);
  });

  it("records an applied query and offers it back when focused empty", () => {
    render(<Harness historyKey="t" />);
    const input = screen.getByLabelText("test query");

    fireEvent.change(input, { target: { value: "status_code:>=500" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(loadHistory("t")).toEqual(["status_code:>=500"]);

    // Clear, refocus: the recent query is suggested.
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.focus(input);
    expect(screen.getByText("status_code:>=500")).toBeInTheDocument();
    expect(screen.getByText("recent")).toBeInTheDocument();

    // Clicking it applies it wholesale.
    fireEvent.mouseDown(screen.getByText("status_code:>=500"));
    expect(input).toHaveValue("status_code:>=500");
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
    expect(loadHistory("c")).toEqual([]);
  });
});

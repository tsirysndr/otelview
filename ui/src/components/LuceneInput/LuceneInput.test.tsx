import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LuceneInput } from "./LuceneInput";
import type { FieldInfo } from "../../lib/api";
import { renderApp } from "../../test/utils";

const FIELDS: FieldInfo[] = [
  {
    name: "http.method",
    count: 12,
    top_values: [
      ["GET", 9],
      ["POST", 3],
    ],
  },
  { name: "http.status_code", count: 12, top_values: [["502", 3]] },
];

function render(value: string, onChange = vi.fn()) {
  const r = renderApp(
    <LuceneInput value={value} onChange={onChange} fields={FIELDS} signal="logs" />,
  );
  return { ...r, onChange };
}

/** The highlight overlay is one span per token, so the classes on the span
 * holding a piece of text say how that piece was lexed. */
function classOf(container: HTMLElement, text: string): string {
  const span = [...container.querySelectorAll("pre span")].find(
    (s) => s.textContent === text,
  );
  if (!span) throw new Error(`no token rendered for ${JSON.stringify(text)}`);
  return span.className;
}

describe("LuceneInput highlighting", () => {
  it("colours fields, operators, strings and numbers apart", () => {
    const { container } = render(
      'http.method:POST AND body:"card declined" AND count:42',
    );
    // The colon belongs to the field name it qualifies.
    expect(classOf(container, "http.method:")).toContain("neon-cyan");
    expect(classOf(container, "AND")).toContain("neon-magenta");
    expect(classOf(container, '"card declined"')).toContain("neon-yellow");
    expect(classOf(container, "42")).toContain("neon-green");
  });

  it("treats range and modifier punctuation as syntax, not text", () => {
    const { container } = render("+http.status_code:[500 TO *] -level:DEBUG");
    expect(classOf(container, "+")).toContain("neon-purple");
    expect(classOf(container, "[")).toContain("neon-purple");
    expect(classOf(container, "TO")).toContain("neon-magenta");
    expect(classOf(container, "-")).toContain("neon-purple");
  });

  it("keeps a fuzzy or boost suffix attached to its number", () => {
    const { container } = render("timeout~2 declined^4");
    // `~2` is one token: splitting it would colour the 2 as a term.
    expect(classOf(container, "~2")).toBeTruthy();
    expect(classOf(container, "^4")).toBeTruthy();
  });

  it("does not mistake a hyphen inside a word for the prohibit operator", () => {
    // `o-42` is a single term; the lexer only negates at clause start.
    const { container } = render("order.id:o-42");
    expect(classOf(container, "order.id:")).toContain("neon-cyan");
  });
});

describe("LuceneInput completions", () => {
  it("offers starter queries when empty, and inserts one", async () => {
    const { onChange } = render("");
    await userEvent.click(screen.getByRole("textbox", { name: "Lucene query" }));

    expect(await screen.findByText("severity_number:[13 TO *]")).toBeInTheDocument();
    await userEvent.click(screen.getByText('"connection refused"'));
    expect(onChange).toHaveBeenCalledWith('"connection refused" ');
  });

  it("completes a field name, then its values", async () => {
    const onChange = vi.fn();
    const { rerender } = render("http.me", onChange);
    const box = screen.getByRole("textbox", { name: "Lucene query" });
    await userEvent.click(box);
    await userEvent.type(box, "t");

    await userEvent.click(await screen.findByText("http.method"));
    expect(onChange).toHaveBeenLastCalledWith("http.method:");

    // With the field in place, the same box offers that field's top values.
    rerender(
      <LuceneInput
        value="http.method:"
        onChange={onChange}
        fields={FIELDS}
        signal="logs"
      />,
    );
    await userEvent.click(screen.getByRole("textbox", { name: "Lucene query" }));
    await userEvent.click(await screen.findByText("POST"));
    expect(onChange).toHaveBeenLastCalledWith("http.method:POST ");
  });

  it("suggests TO inside an unfinished range", async () => {
    const onChange = vi.fn();
    render("http.status_code:[500 ", onChange);
    const box = screen.getByRole("textbox", { name: "Lucene query" });
    await userEvent.click(box);
    await userEvent.type(box, "T");

    await userEvent.click(await screen.findByText("TO"));
    expect(onChange).toHaveBeenLastCalledWith("http.status_code:[500 TO ");
  });
});

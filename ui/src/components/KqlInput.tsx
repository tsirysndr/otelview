import { useCallback } from "react";
import type { FieldInfo } from "../lib/api";
import {
  HighlightedInput,
  type HlToken,
  type SuggestResult,
  type Suggestion,
} from "./HighlightedInput";

/* ---------- tokenizer (mirrors the server-side KQL lexer) ---------- */

type TokenKind = "keyword" | "field" | "op" | "value" | "string" | "paren" | "space";
interface Token {
  kind: TokenKind;
  text: string;
}

const KEYWORDS = new Set(["and", "or", "not"]);

function tokenize(input: string): Token[] {
  const out: Token[] = [];
  let i = 0;
  while (i < input.length) {
    const c = input[i];
    if (c === " " || c === "\t") {
      let j = i;
      while (j < input.length && (input[j] === " " || input[j] === "\t")) j++;
      out.push({ kind: "space", text: input.slice(i, j) });
      i = j;
    } else if (c === "(" || c === ")") {
      out.push({ kind: "paren", text: c });
      i++;
    } else if (c === '"') {
      let j = i + 1;
      while (j < input.length && input[j] !== '"') j += input[j] === "\\" ? 2 : 1;
      out.push({ kind: "string", text: input.slice(i, Math.min(j + 1, input.length)) });
      i = Math.min(j + 1, input.length);
    } else {
      let j = i;
      while (j < input.length && !' \t()"'.includes(input[j])) j++;
      const word = input.slice(i, j);
      const colon = word.indexOf(":");
      if (colon > 0) {
        out.push({ kind: "field", text: word.slice(0, colon + 1) });
        const rest = word.slice(colon + 1);
        const m = /^(>=|<=|>|<)/.exec(rest);
        if (m) {
          out.push({ kind: "op", text: m[1] });
          if (rest.length > m[1].length)
            out.push({ kind: "value", text: rest.slice(m[1].length) });
        } else if (rest) {
          out.push({ kind: "value", text: rest });
        }
        // value may continue as a quoted string handled next loop turn
      } else if (KEYWORDS.has(word.toLowerCase())) {
        out.push({ kind: "keyword", text: word });
      } else {
        out.push({ kind: "value", text: word });
      }
      i = j;
    }
  }
  return out;
}

const TOKEN_CLASS: Record<TokenKind, string> = {
  keyword: "text-neon-magenta",
  field: "text-neon-cyan",
  op: "text-neon-purple",
  value: "text-foreground",
  string: "text-neon-yellow",
  paren: "text-default-400",
  space: "",
};

/* ---------- autocomplete ---------- */

function suggestionsFor(
  text: string,
  cursor: number,
  fields: FieldInfo[],
): SuggestResult {
  const before = text.slice(0, cursor);
  // value position: `field:partial`
  const valueMatch = /([A-Za-z0-9_.@/-]+):(>=|<=|>|<)?("?[^"\s()]*)$/.exec(before);
  if (valueMatch) {
    const [whole, fieldName, , partialRaw] = valueMatch;
    const partial = (partialRaw ?? "").replace(/^"/, "").toLowerCase();
    const field = fields.find((f) => f.name.toLowerCase() === fieldName.toLowerCase());
    const items: Suggestion[] = (field?.top_values ?? [])
      .filter(([v]) => v.toLowerCase().includes(partial))
      .slice(0, 8)
      .map(([v, count]) => ({
        label: v || "∅",
        detail: String(count),
        insert:
          `${fieldName}:` +
          (/[\s:"()]/.test(v) ? `"${v.replaceAll('"', '\\"')}"` : v) +
          " ",
      }));
    return { from: cursor - whole.length, items };
  }
  // field/keyword position: bare partial word
  const wordMatch = /([A-Za-z0-9_.@/-]*)$/.exec(before);
  const partial = (wordMatch?.[1] ?? "").toLowerCase();
  const from = cursor - (wordMatch?.[1].length ?? 0);
  const fieldItems: Suggestion[] = fields
    .filter((f) => f.name.toLowerCase().includes(partial))
    .slice(0, 8)
    .map((f) => ({
      label: f.name,
      detail: `${f.count} · field`,
      insert: `${f.name}:`,
      reopen: true,
    }));
  const kwItems: Suggestion[] = ["and", "or", "not"]
    .filter((k) => partial && k.startsWith(partial))
    .map((k) => ({ label: k, detail: "operator", insert: `${k} ` }));
  return { from, items: [...kwItems, ...fieldItems] };
}

/* ---------- component ---------- */

export function KqlInput({
  value,
  onChange,
  fields,
  invalid,
  placeholder,
}: {
  value: string;
  onChange: (v: string) => void;
  fields: FieldInfo[];
  invalid?: boolean;
  placeholder?: string;
}) {
  const renderTokens = useCallback(
    (v: string): HlToken[] =>
      tokenize(v).map((t) => ({ text: t.text, className: TOKEN_CLASS[t.kind] })),
    [],
  );
  const suggest = useCallback(
    (v: string, cursor: number): SuggestResult => suggestionsFor(v, cursor, fields),
    [fields],
  );
  return (
    <HighlightedInput
      value={value}
      onChange={onChange}
      renderTokens={renderTokens}
      suggest={suggest}
      invalid={invalid}
      placeholder={placeholder}
      ariaLabel="KQL query"
    />
  );
}

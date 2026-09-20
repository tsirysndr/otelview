import { useCallback } from "react";
import type { FieldInfo } from "../../lib/api";
import {
  HighlightedInput,
  type HlToken,
  type SuggestResult,
  type Suggestion,
} from "../HighlightedInput";

/* ---------- tokenizer (mirrors the server-side TraceQL lexer) ---------- */

type TokenKind =
  | "brace"
  | "field"
  | "intrinsic"
  | "op"
  | "logic"
  | "string"
  | "number"
  | "agg"
  | "plain"
  | "space";

interface Token {
  kind: TokenKind;
  text: string;
}

export const INTRINSICS = [
  "name",
  "duration",
  "status",
  "kind",
  "rootName",
  "rootServiceName",
  "traceDuration",
];

const AGGREGATES = ["count", "avg", "sum", "min", "max"];

/** Bare words that are values rather than fields. */
const ENUM_VALUES = new Set([
  "error",
  "ok",
  "unset",
  "server",
  "client",
  "internal",
  "producer",
  "consumer",
  "unspecified",
  "true",
  "false",
]);

const IDENT = /[A-Za-z0-9_.\-/@:]/;

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
    } else if (c === "{" || c === "}" || c === "(" || c === ")") {
      out.push({ kind: "brace", text: c });
      i++;
    } else if (c === "&" && input[i + 1] === "&") {
      out.push({ kind: "logic", text: "&&" });
      i += 2;
    } else if (c === "|" && input[i + 1] === "|") {
      out.push({ kind: "logic", text: "||" });
      i += 2;
    } else if (c === "|") {
      out.push({ kind: "logic", text: "|" });
      i++;
    } else if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < input.length && input[j] !== c) j += input[j] === "\\" ? 2 : 1;
      out.push({ kind: "string", text: input.slice(i, Math.min(j + 1, input.length)) });
      i = Math.min(j + 1, input.length);
    } else if (/[=!<>~]/.test(c)) {
      const two = input.slice(i, i + 2);
      if (["=~", "!~", ">=", "<=", "!=", "=="].includes(two)) {
        out.push({ kind: "op", text: two });
        i += 2;
      } else if (c === "!") {
        out.push({ kind: "logic", text: c });
        i++;
      } else {
        out.push({ kind: "op", text: c });
        i++;
      }
    } else if (/[0-9]/.test(c)) {
      let j = i;
      while (j < input.length && /[0-9.]/.test(input[j])) j++;
      while (j < input.length && /[a-zµ]/i.test(input[j])) j++;
      out.push({ kind: "number", text: input.slice(i, j) });
      i = j;
    } else if (IDENT.test(c)) {
      let j = i;
      while (j < input.length && IDENT.test(input[j])) j++;
      const word = input.slice(i, j);
      // A word followed by `(` is an aggregate call.
      const isCall = input[j] === "(";
      let kind: TokenKind = "field";
      if (isCall && AGGREGATES.includes(word)) kind = "agg";
      else if (INTRINSICS.includes(word)) kind = "intrinsic";
      else if (ENUM_VALUES.has(word)) kind = "plain";
      out.push({ kind, text: word });
      i = j;
    } else {
      out.push({ kind: "plain", text: c });
      i++;
    }
  }
  return out;
}

const TOKEN_CLASS: Record<TokenKind, string> = {
  brace: "text-default-400",
  field: "text-neon-cyan",
  intrinsic: "text-neon-cyan",
  op: "text-neon-purple",
  logic: "text-neon-magenta",
  string: "text-neon-yellow",
  number: "text-neon-green",
  agg: "text-neon-magenta",
  plain: "text-foreground",
  space: "",
};

/* ---------- autocomplete ---------- */

/** Starter queries offered when the box is empty — TraceQL's shape is not
 * guessable, so the first suggestion doubles as documentation. */
const TEMPLATES: Suggestion[] = [
  { label: "{ status = error }", detail: "failing spans", insert: "{ status = error }" },
  {
    label: "{ duration > 100ms }",
    detail: "slow spans",
    insert: "{ duration > 100ms }",
  },
  {
    label: '{ .http.status_code >= 500 }',
    detail: "server errors",
    insert: '{ .http.status_code >= 500 }',
  },
  {
    label: "{ status = error } && { duration > 1s }",
    detail: "both, same trace",
    insert: "{ status = error } && { duration > 1s }",
  },
  {
    label: "{} | count() > 10",
    detail: "large traces",
    insert: "{} | count() > 10",
  },
];

/** Values that only make sense for a particular intrinsic. */
const ENUM_SUGGESTIONS: Record<string, string[]> = {
  status: ["error", "ok", "unset"],
  kind: ["server", "client", "internal", "producer", "consumer", "unspecified"],
};

function quoteIfNeeded(v: string): string {
  return /^[0-9]+(\.[0-9]+)?$/.test(v) || /^[A-Za-z0-9_.\-/@]+$/.test(v)
    ? v
    : `"${v.replaceAll('"', '\\"')}"`;
}

function suggestionsFor(
  text: string,
  cursor: number,
  fields: FieldInfo[],
): SuggestResult {
  const before = text.slice(0, cursor);
  if (before.trim() === "") return { from: 0, items: TEMPLATES };

  // Value position: `field <op> partial`
  const valueMatch =
    /([A-Za-z0-9_.\-/@]+)\s*(=~|!~|>=|<=|!=|==|=|>|<)\s*("?[^"\s(){}|]*)$/.exec(before);
  if (valueMatch) {
    const [whole, rawField, , partialRaw] = valueMatch;
    const partial = (partialRaw ?? "").replace(/^"/, "").toLowerCase();
    const enums = ENUM_SUGGESTIONS[rawField];
    if (enums) {
      return {
        from: cursor - whole.length,
        items: enums
          .filter((v) => v.includes(partial))
          .map((v) => ({
            label: v,
            detail: rawField,
            insert: `${rawField} ${valueMatch[2]} ${v}`,
          })),
      };
    }
    if (rawField === "duration" || rawField === "traceDuration") {
      return {
        from: cursor - whole.length,
        items: ["10ms", "100ms", "500ms", "1s", "5s"]
          .filter((v) => v.startsWith(partial))
          .map((v) => ({
            label: v,
            detail: "duration",
            insert: `${rawField} ${valueMatch[2]} ${v}`,
          })),
      };
    }
    // Attribute values come from the discovered field list. The scope prefix
    // is not part of the attribute's real name, so strip it before lookup.
    const bare = rawField.replace(/^(span\.|resource\.|\.)/, "");
    const field = fields.find((f) => f.name.toLowerCase() === bare.toLowerCase());
    return {
      from: cursor - whole.length,
      items: (field?.top_values ?? [])
        .filter(([v]) => v.toLowerCase().includes(partial))
        .slice(0, 8)
        .map(([v, count]) => ({
          label: v || "∅",
          detail: String(count),
          insert: `${rawField} ${valueMatch[2]} ${quoteIfNeeded(v)}`,
        })),
    };
  }

  // Field position: a bare partial word.
  const wordMatch = /([A-Za-z0-9_.\-/@]*)$/.exec(before);
  const partial = (wordMatch?.[1] ?? "").toLowerCase();
  const from = cursor - (wordMatch?.[1].length ?? 0);

  // Honour an explicit scope prefix if the user typed one.
  const scope = /^(span\.|resource\.)/.exec(partial);
  if (scope) {
    const rest = partial.slice(scope[1].length);
    return {
      from,
      items: fields
        .filter((f) => f.name.toLowerCase().includes(rest))
        .slice(0, 8)
        .map((f) => ({
          label: `${scope[1]}${f.name}`,
          detail: `${f.count} · attribute`,
          insert: `${scope[1]}${f.name} `,
        })),
    };
  }

  const intrinsics: Suggestion[] = INTRINSICS.filter((k) =>
    k.toLowerCase().includes(partial),
  ).map((k) => ({ label: k, detail: "intrinsic", insert: `${k} ` }));

  const attrs: Suggestion[] = fields
    .filter((f) => f.name.toLowerCase().includes(partial.replace(/^\./, "")))
    .slice(0, 8)
    .map((f) => ({
      label: `.${f.name}`,
      detail: `${f.count} · attribute`,
      insert: `.${f.name} `,
    }));

  return { from, items: [...intrinsics, ...attrs] };
}

/* ---------- component ---------- */

/** TraceQL query editor — the trace-side counterpart to KqlInput. */
export function TraceQlInput({
  value,
  onChange,
  fields,
  invalid,
  placeholder,
  historyKey,
}: {
  value: string;
  onChange: (v: string) => void;
  fields: FieldInfo[];
  invalid?: boolean;
  placeholder?: string;
  historyKey?: string;
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
      ariaLabel="TraceQL query"
      historyKey={historyKey}
    />
  );
}

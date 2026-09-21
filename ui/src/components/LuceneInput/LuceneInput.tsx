import { useCallback } from "react";
import type { FieldInfo } from "../../lib/api";
import type { QueryKind } from "../../lib/savedQueries";
import {
  HighlightedInput,
  type HlToken,
  type SuggestResult,
  type Suggestion,
} from "../HighlightedInput";

/* ---------- tokenizer (mirrors the server-side Lucene lexer) ---------- */

type TokenKind =
  | "field"
  | "keyword"
  | "modifier"
  | "range"
  | "string"
  | "number"
  | "fuzzy"
  | "paren"
  | "term"
  | "space";

interface Token {
  kind: TokenKind;
  text: string;
}

/** Both spellings of each operator, as the Rust lexer accepts. */
const KEYWORDS = new Set(["AND", "OR", "NOT", "TO", "&&", "||"]);

/** Same character class the Rust `is_word_char` uses. */
const WORD = /[^\s()[\]{}:"^~+-]/;

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
    } else if ("[]{}".includes(c)) {
      out.push({ kind: "range", text: c });
      i++;
    } else if (c === '"') {
      let j = i + 1;
      while (j < input.length && input[j] !== '"') j += input[j] === "\\" ? 2 : 1;
      out.push({ kind: "string", text: input.slice(i, Math.min(j + 1, input.length)) });
      i = Math.min(j + 1, input.length);
    } else if (c === "~" || c === "^") {
      // The number that follows belongs to the modifier, so they colour as one.
      let j = i + 1;
      while (j < input.length && /[0-9.]/.test(input[j])) j++;
      out.push({ kind: "fuzzy", text: input.slice(i, j) });
      i = j;
    } else if (c === "+" || c === "-" || c === "!") {
      out.push({ kind: "modifier", text: c });
      i++;
    } else if (c === "&" && input[i + 1] === "&") {
      out.push({ kind: "keyword", text: "&&" });
      i += 2;
    } else if (c === "|" && input[i + 1] === "|") {
      out.push({ kind: "keyword", text: "||" });
      i += 2;
    } else if (c === ":") {
      // A colon with no word before it is not a field separator; the lexer
      // would reject it, so show it as a plain term rather than pretending.
      out.push({ kind: "term", text: c });
      i++;
    } else {
      let j = i;
      while (j < input.length && (WORD.test(input[j]) || input[j] === "\\")) {
        j += input[j] === "\\" ? 2 : 1;
      }
      const word = input.slice(i, Math.min(j, input.length));
      i = Math.min(j, input.length);
      if (KEYWORDS.has(word)) {
        out.push({ kind: "keyword", text: word });
      } else if (input[i] === ":") {
        // `field:` — the colon belongs with the name it qualifies.
        out.push({ kind: "field", text: `${word}:` });
        i++;
      } else if (/^[0-9]+(\.[0-9]+)?$/.test(word)) {
        out.push({ kind: "number", text: word });
      } else {
        out.push({ kind: "term", text: word });
      }
    }
  }
  return out;
}

const TOKEN_CLASS: Record<TokenKind, string> = {
  field: "text-neon-cyan",
  keyword: "text-neon-magenta",
  modifier: "text-neon-purple",
  range: "text-neon-purple",
  string: "text-neon-yellow",
  number: "text-neon-green",
  fuzzy: "text-default-400",
  paren: "text-default-400",
  term: "text-foreground",
  space: "",
};

/* ---------- autocomplete ---------- */

/** Offered when the box is empty. Lucene's punctuation — ranges, fuzzy,
 * require/exclude — is the part people do not remember, so the starters
 * exist to show it.
 *
 * They differ per signal because the fields do: a span has no `level` and a
 * log has no `duration`, and a starter naming a field the records do not
 * carry teaches the syntax by way of a query that always returns nothing. */
const STARTERS: Record<LuceneSignal, Suggestion[]> = {
  logs: [
    { label: "level:ERROR", detail: "failing records", insert: "level:ERROR " },
    {
      label: '"connection refused"',
      detail: "exact phrase",
      insert: '"connection refused" ',
    },
    {
      label: "severity_number:[13 TO *]",
      detail: "range, open ended",
      insert: "severity_number:[13 TO *] ",
    },
    { label: "timeout~2", detail: "fuzzy, within 2 edits", insert: "timeout~2 " },
    {
      label: "+service:payments -level:DEBUG",
      detail: "require / exclude",
      insert: "+service:payments -level:DEBUG ",
    },
  ],
  traces: [
    { label: "status:2", detail: "failing spans", insert: "status:2 " },
    {
      label: "http.status_code:[500 TO *]",
      detail: "range, open ended",
      insert: "http.status_code:[500 TO *] ",
    },
    { label: 'name:"GET /checkout"', detail: "exact phrase", insert: 'name:"GET /checkout" ' },
    { label: "kind:client", detail: "outbound spans", insert: "kind:client " },
    {
      label: "+service:payments -kind:internal",
      detail: "require / exclude",
      insert: "+service:payments -kind:internal ",
    },
  ],
};

/** Which record shape the box is querying — see [`STARTERS`]. */
export type LuceneSignal = "logs" | "traces";

function quoteIfNeeded(v: string): string {
  return /[\s:"()[\]{}^~+-]/.test(v) ? `"${v.replaceAll('"', '\\"')}"` : v;
}

function suggestionsFor(
  text: string,
  cursor: number,
  fields: FieldInfo[],
  signal: LuceneSignal,
): SuggestResult {
  const before = text.slice(0, cursor);
  if (before.trim() === "") return { from: 0, items: STARTERS[signal] };

  // Inside a range, the only useful word is the separator.
  const rangeMatch = /[[{]\s*[^\s[\]{}]+\s+([A-Za-z]*)$/.exec(before);
  if (rangeMatch) {
    const partial = rangeMatch[1];
    if ("TO".startsWith(partial.toUpperCase())) {
      return {
        from: cursor - partial.length,
        items: [{ label: "TO", detail: "range separator", insert: "TO " }],
      };
    }
  }

  // Value position: `field:partial`, with any leading quote already typed.
  const valueMatch = /([^\s()[\]{}:"^~+-]+):("?[^"\s()[\]{}]*)$/.exec(before);
  if (valueMatch) {
    const [whole, fieldName, partialRaw] = valueMatch;
    const partial = (partialRaw ?? "").replace(/^"/, "").toLowerCase();
    const field = fields.find((f) => f.name.toLowerCase() === fieldName.toLowerCase());
    return {
      from: cursor - whole.length,
      items: (field?.top_values ?? [])
        .filter(([v]) => v.toLowerCase().includes(partial))
        .slice(0, 8)
        .map(([v, count]) => ({
          label: v || "∅",
          detail: String(count),
          insert: `${fieldName}:${quoteIfNeeded(v)} `,
        })),
    };
  }

  // Field / operator position: a bare partial word.
  const wordMatch = /([^\s()[\]{}:"^~+-]*)$/.exec(before);
  const partial = wordMatch?.[1] ?? "";
  const from = cursor - partial.length;
  const lower = partial.toLowerCase();

  // Operators are upper case in Lucene, so only offer them once the user has
  // typed something — otherwise they crowd out the field list on every space.
  const ops: Suggestion[] = ["AND", "OR", "NOT"]
    .filter((k) => partial !== "" && k.startsWith(partial.toUpperCase()))
    .map((k) => ({ label: k, detail: "operator", insert: `${k} ` }));

  const fieldItems: Suggestion[] = fields
    .filter((f) => f.name.toLowerCase().includes(lower))
    .slice(0, 8)
    .map((f) => ({
      label: f.name,
      detail: `${f.count} · field`,
      insert: `${f.name}:`,
      reopen: true,
    }));

  return { from, items: [...ops, ...fieldItems] };
}

/* ---------- component ---------- */

/** Lucene query editor, offered alongside KQL on logs and TraceQL on traces.
 *
 * The three languages all parse to an AST and evaluate as a predicate in
 * Rust, so which one you write in is purely a matter of what you already
 * know — Lucene is here because it is what Kibana and Solr users type by
 * reflex. */
export function LuceneInput({
  value,
  onChange,
  fields,
  signal,
  invalid,
  placeholder,
  historyKey,
  savedKind,
}: {
  value: string;
  onChange: (v: string) => void;
  fields: FieldInfo[];
  /** Which record shape is being queried; picks the starter queries. */
  signal: LuceneSignal;
  invalid?: boolean;
  placeholder?: string;
  historyKey?: string;
  savedKind?: QueryKind;
}) {
  const renderTokens = useCallback(
    (v: string): HlToken[] =>
      tokenize(v).map((t) => ({ text: t.text, className: TOKEN_CLASS[t.kind] })),
    [],
  );
  const suggest = useCallback(
    (v: string, cursor: number): SuggestResult =>
      suggestionsFor(v, cursor, fields, signal),
    [fields, signal],
  );
  return (
    <HighlightedInput
      value={value}
      onChange={onChange}
      renderTokens={renderTokens}
      suggest={suggest}
      invalid={invalid}
      placeholder={placeholder}
      ariaLabel="Lucene query"
      historyKey={historyKey}
      savedKind={savedKind}
    />
  );
}

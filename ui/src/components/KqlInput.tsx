import { useEffect, useMemo, useRef, useState } from "react";
import { IconSearch } from "@tabler/icons-react";
import type { FieldInfo } from "../lib/api";
import { plainTextField } from "../lib/inputProps";

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

interface Suggestion {
  label: string;
  detail?: string;
  /** Replacement for the current partial token. */
  insert: string;
  /** Keep the caret typing (fields insert `name:` and stay open). */
  reopen?: boolean;
}

function suggestionsFor(
  text: string,
  cursor: number,
  fields: FieldInfo[],
): { from: number; items: Suggestion[] } {
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
  const inputRef = useRef<HTMLInputElement>(null);
  const preRef = useRef<HTMLPreElement>(null);
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(0);
  const [selected, setSelected] = useState(0);

  const tokens = useMemo(() => tokenize(value), [value]);
  const { from, items } = useMemo(
    () => (open ? suggestionsFor(value, cursor, fields) : { from: 0, items: [] }),
    [open, value, cursor, fields],
  );

  useEffect(() => setSelected(0), [items.length, from]);

  const syncScroll = () => {
    if (preRef.current && inputRef.current) {
      preRef.current.scrollLeft = inputRef.current.scrollLeft;
    }
  };

  const accept = (s: Suggestion) => {
    const next = value.slice(0, from) + s.insert + value.slice(cursor);
    const caret = from + s.insert.length;
    onChange(next);
    requestAnimationFrame(() => {
      inputRef.current?.setSelectionRange(caret, caret);
      setCursor(caret);
      syncScroll();
    });
    setOpen(!!s.reopen);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (open && items.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelected((s) => (s + 1) % items.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelected((s) => (s - 1 + items.length) % items.length);
        return;
      }
      if (e.key === "Tab" || e.key === "Enter") {
        e.preventDefault();
        accept(items[selected]);
        return;
      }
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
        return;
      }
    }
    if (e.key === "Escape") (e.target as HTMLInputElement).blur();
  };

  const updateCursor = (el: HTMLInputElement) => {
    setCursor(el.selectionStart ?? el.value.length);
  };

  return (
    <div className="relative w-full">
      <div
        className={`flex h-8 items-center gap-1 rounded-small border-2 bg-transparent pl-2 pr-2 transition-colors ${
          invalid
            ? "border-danger"
            : "border-default-300 focus-within:border-default-500 hover:border-default-400"
        }`}
      >
        <IconSearch size={14} className="shrink-0 text-default-400" />
        <div className="relative min-w-0 flex-1">
          {/* highlight layer */}
          <pre
            ref={preRef}
            aria-hidden
            className="pointer-events-none absolute inset-0 overflow-hidden whitespace-pre font-mono text-xs leading-7"
          >
            {tokens.map((t, i) => (
              <span key={i} className={TOKEN_CLASS[t.kind]}>
                {t.text}
              </span>
            ))}
          </pre>
          <input
            {...plainTextField}
            ref={inputRef}
            value={value}
            placeholder={placeholder}
            aria-label="KQL query"
            className="relative z-10 w-full bg-transparent font-mono text-xs leading-7 text-transparent caret-neon-cyan outline-none placeholder:text-default-400"
            onChange={(e) => {
              onChange(e.target.value);
              updateCursor(e.target);
              setOpen(true);
              syncScroll();
            }}
            onKeyDown={onKeyDown}
            onKeyUp={(e) => updateCursor(e.currentTarget)}
            onClick={(e) => updateCursor(e.currentTarget)}
            onScroll={syncScroll}
            onFocus={(e) => {
              updateCursor(e.currentTarget);
              setOpen(true);
            }}
            onBlur={() => setTimeout(() => setOpen(false), 150)}
          />
        </div>
      </div>

      {open && items.length > 0 && (
        <div className="absolute left-0 top-9 z-40 max-h-64 w-80 overflow-y-auto rounded-large border border-content3 bg-content1 p-1">
          {items.map((s, i) => (
            <button
              key={`${s.label}-${i}`}
              onMouseDown={(e) => {
                e.preventDefault();
                accept(s);
              }}
              className={`flex w-full items-baseline justify-between gap-3 rounded-md px-2 py-1 text-left text-xs ${
                i === selected
                  ? "bg-[rgba(255,42,109,0.16)] text-foreground shadow-[inset_2px_0_0_#ff2a6d]"
                  : "text-default-600 hover:bg-content2"
              }`}
            >
              <span className="truncate font-mono">{s.label}</span>
              {s.detail && (
                <span className="shrink-0 text-[10px] text-default-400">{s.detail}</span>
              )}
            </button>
          ))}
          <div className="border-t border-divider/60 px-2 pt-1 text-[9px] text-default-400">
            ↑↓ navigate · tab/enter accept · esc close
          </div>
        </div>
      )}
    </div>
  );
}

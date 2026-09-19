import { useEffect, useMemo, useRef, useState } from "react";
import { IconSearch } from "@tabler/icons-react";
import { plainTextField } from "../lib/inputProps";

export interface HlToken {
  text: string;
  className: string;
}

export interface Suggestion {
  label: string;
  detail?: string;
  /** Replacement for the current partial token. */
  insert: string;
  /** Keep the dropdown open after accepting (e.g. `field:` prefixes). */
  reopen?: boolean;
}

export interface SuggestResult {
  from: number;
  items: Suggestion[];
}

/** Single-line input with a syntax-highlight overlay and an autocomplete
 * dropdown. The grammar is supplied by the caller via `renderTokens` and
 * `suggest` — shared by the KQL bar and the trace attribute filter. */
export function HighlightedInput({
  value,
  onChange,
  renderTokens,
  suggest,
  invalid,
  placeholder,
  ariaLabel,
}: {
  value: string;
  onChange: (v: string) => void;
  renderTokens: (value: string) => HlToken[];
  suggest: (value: string, cursor: number) => SuggestResult;
  invalid?: boolean;
  placeholder?: string;
  ariaLabel: string;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const preRef = useRef<HTMLPreElement>(null);
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(0);
  const [selected, setSelected] = useState(0);

  const tokens = useMemo(() => renderTokens(value), [renderTokens, value]);
  const { from, items } = useMemo(
    () => (open ? suggest(value, cursor) : { from: 0, items: [] }),
    [open, suggest, value, cursor],
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
          <pre
            ref={preRef}
            aria-hidden
            className="pointer-events-none absolute inset-0 overflow-hidden whitespace-pre font-mono text-xs leading-7"
          >
            {tokens.map((t, i) => (
              <span key={i} className={t.className}>
                {t.text}
              </span>
            ))}
          </pre>
          <input
            {...plainTextField}
            ref={inputRef}
            value={value}
            placeholder={placeholder}
            aria-label={ariaLabel}
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

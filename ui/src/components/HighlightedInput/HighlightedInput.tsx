import { useEffect, useMemo, useRef, useState } from "react";
import { useAtom } from "jotai";
import { RESET } from "jotai/utils";
import {
  IconBookmark,
  IconBookmarkFilled,
  IconSearch,
  IconTrash,
  IconX,
} from "@tabler/icons-react";
import { withAppliedQuery } from "../../lib/history";
import { plainTextField } from "../../lib/inputProps";
import {
  findSaved,
  removeSaved,
  saveQuery,
  savedFor,
  type QueryKind,
} from "../../lib/savedQueries";
import { queryHistoryFamily, savedQueriesAtom } from "../../state/atoms";

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
  /** Replace the whole input value instead of just the token at `from`
   * (recent-query picks, which are a full query, not a token). */
  replaceAll?: boolean;
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
  historyKey,
  savedKind,
}: {
  value: string;
  onChange: (v: string) => void;
  renderTokens: (value: string) => HlToken[];
  suggest: (value: string, cursor: number) => SuggestResult;
  invalid?: boolean;
  placeholder?: string;
  ariaLabel: string;
  /** Persist applied queries under this key and offer them back when the
   * input is focused empty. Omit for inputs with nothing worth recalling. */
  historyKey?: string;
  /** Which signal saved queries belong to. Omit to hide saving entirely. */
  savedKind?: QueryKind;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const preRef = useRef<HTMLPreElement>(null);
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(0);
  const [selected, setSelected] = useState(0);

  // atomFamily needs a concrete key on every render; historyKey-less inputs
  // just get an inert, never-written slot.
  const [history, setHistory] = useAtom(queryHistoryFamily(historyKey ?? ""));

  // A query counts as applied when the user commits it — Enter outside the
  // dropdown, or leaving the field with text in it. Filters here apply live
  // while typing, so there is no submit event to hook instead.
  const recordApplied = () => {
    if (historyKey && value.trim()) {
      setHistory((h) => withAppliedQuery(h, value));
    }
  };

  const clearAllHistory = () => setHistory(RESET);

  const [allSaved, setAllSaved] = useAtom(savedQueriesAtom);
  const saved = useMemo(
    () => (savedKind ? savedFor(allSaved, savedKind) : []),
    [allSaved, savedKind],
  );
  const savedHere = savedKind ? findSaved(allSaved, savedKind, value) : undefined;
  // Naming happens inline rather than in a modal — the query is right there
  // and a dialog for one text field would be heavier than the task.
  const [naming, setNaming] = useState<string | null>(null);

  const commitSave = () => {
    if (!savedKind || naming === null) return;
    setAllSaved(saveQuery(allSaved, { name: naming, kind: savedKind, query: value }));
    setNaming(null);
  };

  const tokens = useMemo(() => renderTokens(value), [renderTokens, value]);
  const { from, items } = useMemo(() => {
    if (!open) return { from: 0, items: [] };
    const trimmed = value.trim();
    const lower = trimmed.toLowerCase();
    const hit = (s: string) => trimmed === "" || s.toLowerCase().includes(lower);
    // Saved queries lead: they were named on purpose, so they outrank both
    // the automatic history and the grammar's completions.
    const savedItems: Suggestion[] = saved
      .filter((s) => s.query !== value && (hit(s.query) || hit(s.name)))
      .map((s) => ({
        label: s.name,
        detail: "saved",
        insert: s.query,
        replaceAll: true,
      }));
    // Recent exact queries that match what's typed so far, offered as
    // full-value replacements ahead of the grammar's own completions.
    const historyItems: Suggestion[] = history
      .filter((q) => q !== value && hit(q) && !saved.some((s) => s.query === q))
      .map((q) => ({ label: q, detail: "recent", insert: q, replaceAll: true }));
    // What the input can recall leads; the grammar's own completions follow.
    // The empty case is the one that matters most: TraceQL and Lucene answer
    // it with starter queries, which is the only place their punctuation is
    // ever shown, so skipping the grammar here left them undiscoverable.
    const grammar = suggest(value, cursor);
    if (trimmed === "") {
      return { from: 0, items: [...savedItems, ...historyItems, ...grammar.items] };
    }
    return {
      from: grammar.from,
      items: [...savedItems, ...historyItems, ...grammar.items],
    };
  }, [open, suggest, value, cursor, history, saved]);

  useEffect(() => setSelected(0), [items.length, from]);

  const syncScroll = () => {
    if (preRef.current && inputRef.current) {
      preRef.current.scrollLeft = inputRef.current.scrollLeft;
    }
  };

  const accept = (s: Suggestion) => {
    const start = s.replaceAll ? 0 : from;
    const end = s.replaceAll ? value.length : cursor;
    const next = value.slice(0, start) + s.insert + value.slice(end);
    const caret = start + s.insert.length;
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
    if (e.key === "Enter") recordApplied();
    if (e.key === "Escape") (e.target as HTMLInputElement).blur();
  };

  const clear = () => {
    onChange("");
    setOpen(false);
    inputRef.current?.focus();
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
            onBlur={() => {
              recordApplied();
              setTimeout(() => setOpen(false), 150);
            }}
          />
        </div>
        {savedKind && value.trim() !== "" && (
          <button
            type="button"
            aria-label={savedHere ? "Unsave query" : "Save query"}
            title={savedHere ? `Saved as "${savedHere.name}" — click to remove` : "Save this query"}
            onMouseDown={(e) => {
              e.preventDefault();
              if (savedHere) {
                setAllSaved(removeSaved(allSaved, savedHere.id));
                return;
              }
              // Prefill with the query text: naming is optional, and Enter
              // straight away is a perfectly good outcome.
              setNaming(value.trim());
              setOpen(true);
            }}
            className={`shrink-0 rounded p-0.5 transition-colors ${
              savedHere
                ? "text-neon-yellow"
                : "text-default-400 hover:text-foreground"
            }`}
          >
            {savedHere ? <IconBookmarkFilled size={14} /> : <IconBookmark size={14} />}
          </button>
        )}
        {value !== "" && (
          <button
            type="button"
            aria-label="Clear query"
            title="Clear"
            onMouseDown={(e) => {
              // mousedown, not click: the input's blur handler closes the
              // dropdown on a 150ms timer and a click would land after it.
              e.preventDefault();
              clear();
            }}
            className="shrink-0 rounded p-0.5 text-default-400 transition-colors hover:text-foreground"
          >
            <IconX size={14} />
          </button>
        )}
      </div>

      {naming !== null && (
        <div className="absolute left-0 top-9 z-50 flex w-80 max-w-full items-center gap-1 rounded-large border border-content3 bg-content1 p-1.5">
          <input
            {...plainTextField}
            autoFocus
            aria-label="Name for the saved query"
            placeholder="name this query"
            value={naming}
            onChange={(e) => setNaming(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") commitSave();
              if (e.key === "Escape") setNaming(null);
            }}
            onBlur={() => setNaming(null)}
            className="min-w-0 flex-1 rounded bg-content2 px-2 py-1 text-xs text-foreground outline-none placeholder:text-default-400"
          />
          <button
            type="button"
            aria-label="Confirm save"
            onMouseDown={(e) => {
              e.preventDefault();
              commitSave();
            }}
            className="shrink-0 rounded px-2 py-1 text-[11px] text-neon-cyan hover:bg-content2"
          >
            save
          </button>
        </div>
      )}

      {naming === null && open && (items.length > 0 || (historyKey && history.length > 0)) && (
        <>
          {/* Mobile and tablet get a bottom sheet with a dismiss scrim — an
              absolutely-positioned 320px dropdown doesn't work with an
              on-screen keyboard eating half the viewport. Desktop (lg+)
              keeps the anchored dropdown. */}
          <div
            className="fixed inset-0 z-30 bg-black/50 lg:hidden"
            onClick={() => setOpen(false)}
          />
          <div
            className="fixed inset-x-0 bottom-0 z-40 max-h-[70vh] overflow-y-auto rounded-t-large
              border-t border-content3 bg-content1 p-2 pb-[max(0.5rem,env(safe-area-inset-bottom))]
              lg:absolute lg:inset-x-auto lg:inset-y-auto lg:left-0 lg:top-9 lg:max-h-64 lg:w-80
              lg:rounded-large lg:border lg:p-1"
          >
            {items.length === 0 && (
              <p className="px-2 py-1 text-[11px] text-default-400">no matches</p>
            )}
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
            <div className="flex items-center justify-between gap-2 border-t border-divider/60 px-2 pt-1 text-[9px] text-default-400">
              <span>↑↓ navigate · tab/enter accept · esc close</span>
              {historyKey && history.length > 0 && (
                <button
                  type="button"
                  aria-label="Clear history"
                  onMouseDown={(e) => {
                    // mousedown, not click: same blur-timing reason as the
                    // input's clear button above.
                    e.preventDefault();
                    clearAllHistory();
                  }}
                  title="Clear recent search history"
                  className="flex shrink-0 items-center gap-0.5 text-default-400 transition-colors hover:text-danger"
                >
                  <IconTrash size={11} />
                  clear history
                </button>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  );
}

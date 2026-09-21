import { IconSearch, IconX } from "@tabler/icons-react";
import { plainTextField } from "../../lib/inputProps";

/** Compact client-side filter for a list or a panel header.
 *
 * Deliberately not the query bar: this narrows what is already on screen
 * and never reaches the server, so there is no grammar, no history and no
 * submit — just typing. */
export function SearchBox({
  value,
  onChange,
  placeholder,
  ariaLabel,
  showing,
  total,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  ariaLabel: string;
  /** When filtering, say how much is hidden rather than leaving the user to
   * wonder where the rest went. */
  showing?: number;
  total?: number;
}) {
  const filtered = value.trim() !== "" && showing !== undefined && total !== undefined;
  return (
    <div className="flex h-8 shrink-0 items-center gap-1.5 border-b border-divider px-2">
      <IconSearch size={13} className="shrink-0 text-default-400" />
      <input
        {...plainTextField}
        value={value}
        aria-label={ariaLabel}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.stopPropagation();
            onChange("");
          }
        }}
        className="min-w-0 flex-1 bg-transparent text-xs text-foreground outline-none placeholder:text-default-400"
      />
      {filtered && (
        <span className="shrink-0 text-[10px] tabular-nums text-default-400">
          {showing}/{total}
        </span>
      )}
      {value !== "" && (
        <button
          type="button"
          aria-label={`Clear ${ariaLabel.toLowerCase()}`}
          onClick={() => onChange("")}
          className="shrink-0 rounded p-0.5 text-default-400 transition-colors hover:text-foreground"
        >
          <IconX size={12} />
        </button>
      )}
    </div>
  );
}

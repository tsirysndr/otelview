import { useCallback } from "react";
import type { FieldInfo } from "../lib/api";
import {
  HighlightedInput,
  type HlToken,
  type SuggestResult,
} from "./HighlightedInput";

/** Trace attribute filter editor. Grammar: `key=value` (exact attribute
 * match) or any bare text (substring over all attributes). */
export function AttrInput({
  value,
  onChange,
  fields,
  placeholder,
  historyKey,
}: {
  value: string;
  onChange: (v: string) => void;
  fields: FieldInfo[];
  placeholder?: string;
  historyKey?: string;
}) {
  const renderTokens = useCallback((v: string): HlToken[] => {
    const eq = v.indexOf("=");
    if (eq < 0) return [{ text: v, className: "text-foreground" }];
    return [
      { text: v.slice(0, eq), className: "text-neon-cyan" },
      { text: "=", className: "text-neon-purple" },
      { text: v.slice(eq + 1), className: "text-neon-yellow" },
    ];
  }, []);

  const suggest = useCallback(
    (v: string, cursor: number): SuggestResult => {
      const eq = v.indexOf("=");
      if (eq >= 0 && cursor > eq) {
        // value position: complete top values of the chosen key
        const key = v.slice(0, eq).trim();
        const partial = v.slice(eq + 1, cursor).trim().toLowerCase();
        const field = fields.find((f) => f.name.toLowerCase() === key.toLowerCase());
        return {
          from: eq + 1,
          items: (field?.top_values ?? [])
            .filter(([val]) => val.toLowerCase().includes(partial))
            .slice(0, 8)
            .map(([val, count]) => ({
              label: val || "∅",
              detail: String(count),
              insert: val,
            })),
        };
      }
      // key position
      const partial = v.slice(0, eq >= 0 ? eq : cursor).trim().toLowerCase();
      return {
        from: 0,
        items: fields
          .filter((f) => f.name.toLowerCase().includes(partial))
          .slice(0, 8)
          .map((f) => ({
            label: f.name,
            detail: `${f.count} · attribute`,
            insert: `${f.name}=`,
            reopen: true,
          })),
      };
    },
    [fields],
  );

  return (
    <HighlightedInput
      value={value}
      onChange={onChange}
      renderTokens={renderTokens}
      suggest={suggest}
      placeholder={placeholder}
      ariaLabel="Attribute filter"
      historyKey={historyKey}
    />
  );
}

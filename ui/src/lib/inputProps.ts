// Kill browser/webview text meddling on every form field: no autofill
// dropdown, no macOS autocorrect/auto-capitalization, no spellcheck squiggles.
// Spread into each <Input> (and the cmdk input).
export const plainTextField = {
  autoComplete: "off",
  autoCorrect: "off",
  autoCapitalize: "off",
  spellCheck: "false",
} as const;

// House style for every HeroUI Input/Select: bordered (no filled background —
// wrong on the dark surfaces), label floated outside so it never overlaps the
// placeholder, compact.
export const fieldProps = {
  variant: "bordered",
  radius: "sm",
  size: "sm",
  labelPlacement: "outside",
} as const;

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent);
}

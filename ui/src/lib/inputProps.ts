// Kill browser/webview text meddling on every form field: no autofill
// dropdown, no macOS autocorrect/auto-capitalization, no spellcheck squiggles.
// Spread into each <Input> (and the cmdk input).
export const plainTextField = {
  autoComplete: "off",
  autoCorrect: "off",
  autoCapitalize: "off",
  spellCheck: "false",
} as const;

// House style for HeroUI Input/Select in toolbars: bordered (no filled
// background — wrong on the dark surfaces), compact, and label-less — dense
// filter bars use placeholders + aria-labels; only roomy vertical forms
// (Settings) carry visible labels.
export const fieldProps = {
  variant: "bordered",
  radius: "sm",
  size: "sm",
} as const;

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent);
}

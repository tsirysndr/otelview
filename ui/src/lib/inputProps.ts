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
  classNames: {
    inputWrapper:
      "border-default-300 data-[hover=true]:border-default-400 group-data-[focus=true]:border-default-500",
  },
} as const;

// Compact switch: fixed 12px round thumb in a 32x16 track. Overrides
// HeroUI's pressed-state thumb stretch, which squeezes at this size.
export const switchClassNames = {
  wrapper: "h-4 w-8 mr-0 px-[1px]",
  thumb:
    "h-3.5 w-3.5 rounded-full shadow-none " +
    "group-data-[pressed=true]:w-3.5 " +
    "group-data-[selected=true]:ms-4 " +
    "group-data-[selected=true]:group-data-[pressed=true]:ms-4",
} as const;

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac/.test(navigator.userAgent);
}

import { useAtom } from "jotai";
import { IconX } from "@tabler/icons-react";
import { helpOpenAtom } from "../../state/atoms";
import { SHORTCUTS } from "../../hooks/useShortcuts";

/** Keyboard shortcuts overlay — same centered panel as the command palette. */
export function HelpModal() {
  const [open, setOpen] = useAtom(helpOpenAtom);
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/60 lg:p-4 lg:pt-[15vh]"
      onClick={() => setOpen(false)}
      role="presentation"
    >
      <div
        role="dialog"
        aria-label="Keyboard shortcuts"
        onClick={(e) => e.stopPropagation()}
        className="flex h-full w-full flex-col overflow-hidden bg-content1
          lg:h-auto lg:max-w-md lg:rounded-large lg:border lg:border-content3"
      >
        <div className="flex shrink-0 items-center justify-between border-b border-content3 px-4 py-3">
          <span className="text-[11px] uppercase tracking-wider text-default-500">
            keyboard shortcuts
          </span>
          <button
            type="button"
            aria-label="Close"
            onClick={() => setOpen(false)}
            className="rounded p-1 text-default-400 hover:text-foreground lg:hidden"
          >
            <IconX size={16} />
          </button>
        </div>
        <div className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto p-4 lg:flex-none">
          {SHORTCUTS.map((s) => (
            <div key={s.label} className="flex items-center justify-between gap-4">
              <span className="text-sm text-default-600">{s.label}</span>
              <span className="flex gap-1">
                {s.keys.map((k) => (
                  <kbd key={k} className="neon-key">
                    {k}
                  </kbd>
                ))}
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

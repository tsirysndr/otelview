import { useAtom } from "jotai";
import { helpOpenAtom } from "../state/atoms";
import { SHORTCUTS } from "../hooks/useShortcuts";

/** Keyboard shortcuts overlay — same centered panel as the command palette. */
export function HelpModal() {
  const [open, setOpen] = useAtom(helpOpenAtom);
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/60 p-4 pt-[15vh]"
      onClick={() => setOpen(false)}
      role="presentation"
    >
      <div
        role="dialog"
        aria-label="Keyboard shortcuts"
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-md overflow-hidden rounded-large border border-content3 bg-content1"
      >
        <div className="border-b border-content3 px-4 py-3 text-[11px] uppercase tracking-wider text-default-500">
          keyboard shortcuts
        </div>
        <div className="flex flex-col gap-1.5 p-4">
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

import { useEffect, useRef, useState } from "react";
import { useSetAtom } from "jotai";
import { IconCheck, IconServer2, IconSettings } from "@tabler/icons-react";
import { viewAtom } from "../../state/atoms";
import { describeTarget } from "../../lib/profiles";
import { useServerProfiles } from "../../hooks/useProfiles";

/** Which server the UI is reading from, and a one-click way to change it.
 *
 * Lives in the top bar rather than only in settings: switching between a
 * local instance and a remote one is a routine move, not a configuration
 * change, and it should never cost more than a click. */
export function ServerSwitcher() {
  const { profiles, active, switchTo } = useServerProfiles();
  const setView = useSetAtom(viewAtom);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div ref={ref} className="relative shrink-0">
      <button
        onClick={() => setOpen((v) => !v)}
        aria-label="Switch server"
        aria-expanded={open}
        title={`Reading from ${describeTarget(active)}`}
        className="flex h-7 items-center gap-1.5 rounded-lg bg-content2 px-2 text-xs text-default-500 transition-colors hover:text-foreground"
      >
        <IconServer2 size={13} className="shrink-0 text-neon-cyan" />
        <span className="max-w-28 truncate">{active.name}</span>
      </button>

      {open && (
        <>
          {/* Below lg the top bar scrolls horizontally, which would clip an
              absolutely-positioned menu — so there it is a fixed sheet. */}
          <div
            className="fixed inset-0 z-40 bg-black/50 lg:hidden"
            onClick={() => setOpen(false)}
          />
          <div
            className="fixed inset-x-0 bottom-0 z-50 max-h-[70vh] overflow-y-auto rounded-t-large
              border-t border-content3 bg-content1 p-1 pb-[max(0.25rem,env(safe-area-inset-bottom))]
              lg:absolute lg:inset-x-auto lg:bottom-auto lg:right-0 lg:top-9 lg:w-64 lg:rounded-large
              lg:border lg:p-1"
          >
            <p className="px-2 py-1 text-[10px] uppercase tracking-wider text-default-400">
              read from
            </p>
            {profiles.map((p) => {
              const isActive = p.id === active.id;
              return (
                <button
                  key={p.id}
                  onClick={() => {
                    switchTo(p.id);
                    setOpen(false);
                  }}
                  className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors ${
                    isActive ? "text-neon-cyan" : "text-default-600 hover:bg-content2"
                  }`}
                >
                  <IconCheck
                    size={13}
                    className={`shrink-0 ${isActive ? "" : "invisible"}`}
                  />
                  <span className="min-w-0 flex-1 truncate">{p.name}</span>
                  <span className="shrink-0 truncate text-[10px] text-default-400">
                    {describeTarget(p)}
                  </span>
                </button>
              );
            })}
            <div className="mt-1 border-t border-divider/60 pt-1">
              <button
                onClick={() => {
                  setView("settings");
                  setOpen(false);
                }}
                className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-default-500 transition-colors hover:bg-content2 hover:text-foreground"
              >
                <IconSettings size={13} className="shrink-0" />
                manage servers
              </button>
            </div>
          </div>
        </>
      )}
    </div>
  );
}

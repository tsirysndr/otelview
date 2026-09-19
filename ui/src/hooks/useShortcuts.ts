import { useEffect } from "react";
import { useAtom, useSetAtom } from "jotai";
import { useQueryClient } from "@tanstack/react-query";
import {
  helpOpenAtom,
  inspectorOpenAtom,
  liveAtom,
  openTraceIdAtom,
  paletteOpenAtom,
  railVisibleAtom,
  selectedLogAtom,
  selectedSpanIdAtom,
  themeAtom,
  viewAtom,
} from "../state/atoms";

function inEditable(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  if (!t) return false;
  const tag = t.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    t.isContentEditable
  );
}

/** Global keyboard shortcuts. Single-key shortcuts are suppressed while
 * typing in a form field; ⌘-chords always fire. */
export function useShortcuts() {
  const [palette, setPalette] = useAtom(paletteOpenAtom);
  const [help, setHelp] = useAtom(helpOpenAtom);
  const setRail = useSetAtom(railVisibleAtom);
  const setInspector = useSetAtom(inspectorOpenAtom);
  const setView = useSetAtom(viewAtom);
  const setTheme = useSetAtom(themeAtom);
  const setLive = useSetAtom(liveAtom);
  const [openTrace, setOpenTrace] = useAtom(openTraceIdAtom);
  const [selSpan, setSelSpan] = useAtom(selectedSpanIdAtom);
  const [selLog, setSelLog] = useAtom(selectedLogAtom);
  const qc = useQueryClient();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;

      // ⌘K — palette (works everywhere)
      if (mod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((v) => !v);
        return;
      }
      if (mod && e.key.toLowerCase() === "b") {
        e.preventDefault();
        setRail((v) => !v);
        return;
      }
      if (mod && e.key.toLowerCase() === "j") {
        e.preventDefault();
        setInspector((v) => !v);
        return;
      }

      if (inEditable(e) || mod || e.altKey) return;

      // Escape: close overlays, then inspector selection, then trace detail.
      if (e.key === "Escape") {
        if (palette) return setPalette(false);
        if (help) return setHelp(false);
        if (selSpan || selLog) {
          setSelSpan(null);
          setSelLog(null);
          return;
        }
        if (openTrace) setOpenTrace(null);
        return;
      }
      if (palette || help) return;

      switch (e.key) {
        case "/":
          e.preventDefault();
          setPalette(true);
          break;
        case "?":
          e.preventDefault();
          setHelp((v) => !v);
          break;
        case "1":
          setView("traces");
          break;
        case "2":
          setView("logs");
          break;
        case "3":
          setView("metrics");
          break;
        case "4":
          setView("settings");
          break;
        case "t":
          setTheme((t) => (t === "dark" ? "light" : "dark"));
          break;
        case "l":
          setLive((v) => !v);
          break;
        case "r":
          void qc.invalidateQueries();
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    palette,
    help,
    selSpan,
    selLog,
    openTrace,
    qc,
    setHelp,
    setInspector,
    setLive,
    setOpenTrace,
    setPalette,
    setRail,
    setSelLog,
    setSelSpan,
    setTheme,
    setView,
  ]);
}

export const SHORTCUTS: { keys: string[]; label: string }[] = [
  { keys: ["/", "⌘K"], label: "Search & commands" },
  { keys: ["?"], label: "Keyboard shortcuts" },
  { keys: ["1"], label: "Traces" },
  { keys: ["2"], label: "Logs" },
  { keys: ["3"], label: "Metrics" },
  { keys: ["4"], label: "Settings" },
  { keys: ["⌘B"], label: "Toggle left rail" },
  { keys: ["⌘J"], label: "Toggle inspector panel" },
  { keys: ["l"], label: "Toggle live refresh" },
  { keys: ["r"], label: "Refresh data" },
  { keys: ["t"], label: "Toggle theme" },
  { keys: ["esc"], label: "Close / back" },
];

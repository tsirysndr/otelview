import {
  IconAlignLeft,
  IconChartLine,
  IconRoute,
  IconSettings,
  IconTopologyStar3,
} from "@tabler/icons-react";
import { useAtom, useSetAtom } from "jotai";
import {
  openTraceIdAtom,
  selectedLogAtom,
  selectedSpanIdAtom,
  viewAtom,
  type View,
} from "../state/atoms";

const ITEMS: { view: View; label: string; icon: typeof IconRoute }[] = [
  { view: "traces", label: "Traces", icon: IconRoute },
  { view: "logs", label: "Logs", icon: IconAlignLeft },
  { view: "metrics", label: "Metrics", icon: IconChartLine },
  { view: "services", label: "Services", icon: IconTopologyStar3 },
];

function RailButton({
  active,
  label,
  onPress,
  icon: Icon,
}: {
  active: boolean;
  label: string;
  onPress: () => void;
  icon: typeof IconRoute;
}) {
  return (
    <button
      onClick={onPress}
      aria-label={label}
      className={`relative flex h-9 w-full items-center gap-2.5 px-3 text-sm transition-colors ${
        active
          ? "bg-content2 text-neon-cyan"
          : "text-default-500 hover:bg-content2/60 hover:text-foreground"
      }`}
    >
      {active && (
        <span className="absolute left-0 top-1 h-7 w-0.5 rounded-r bg-neon-cyan shadow-[0_0_6px_#05D9E8]" />
      )}
      <Icon size={18} stroke={1.6} className="shrink-0" />
      <span className="truncate">{label}</span>
    </button>
  );
}

/** Left sidebar: always shows icon + title (never minified). */
export function IconRail() {
  const [view, setView] = useAtom(viewAtom);
  const setOpenTrace = useSetAtom(openTraceIdAtom);
  const setSelectedSpan = useSetAtom(selectedSpanIdAtom);
  const setSelectedLog = useSetAtom(selectedLogAtom);

  const switchTo = (v: View) => {
    setView(v);
    if (v !== "traces") setOpenTrace(null);
    setSelectedSpan(null);
    setSelectedLog(null);
  };

  return (
    <nav className="flex w-52 shrink-0 flex-col border-r border-divider bg-content1 py-1.5">
      {ITEMS.map(({ view: v, label, icon }) => (
        <RailButton
          key={v}
          active={view === v}
          label={label}
          icon={icon}
          onPress={() => switchTo(v)}
        />
      ))}
      <div className="flex-1" />
      <RailButton
        active={view === "settings"}
        label="Settings"
        icon={IconSettings}
        onPress={() => switchTo("settings")}
      />
    </nav>
  );
}

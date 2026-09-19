import { Tooltip } from "@heroui/react";
import {
  IconAlignLeft,
  IconChartLine,
  IconRoute,
  IconSettings,
} from "@tabler/icons-react";
import { useAtom, useSetAtom } from "jotai";
import { openTraceIdAtom, selectedLogAtom, selectedSpanIdAtom, viewAtom, type View } from "../state/atoms";

const ITEMS: { view: View; label: string; icon: typeof IconRoute }[] = [
  { view: "traces", label: "Traces", icon: IconRoute },
  { view: "logs", label: "Logs", icon: IconAlignLeft },
  { view: "metrics", label: "Metrics", icon: IconChartLine },
];

function RailButton({
  active,
  label,
  onPress,
  children,
}: {
  active: boolean;
  label: string;
  onPress: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip content={label} placement="right" delay={400} closeDelay={0}>
      <button
        onClick={onPress}
        aria-label={label}
        className={`relative flex h-11 w-full items-center justify-center transition-colors ${
          active
            ? "text-neon-cyan"
            : "text-default-500 hover:text-foreground"
        }`}
      >
        {active && (
          <span className="absolute left-0 top-1.5 h-8 w-0.5 rounded-r bg-neon-cyan shadow-[0_0_6px_#05D9E8]" />
        )}
        {children}
      </button>
    </Tooltip>
  );
}

/** VS Code-style activity bar. */
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
    <nav className="flex w-12 shrink-0 flex-col items-center border-r border-divider bg-content1 py-1">
      {ITEMS.map(({ view: v, label, icon: Icon }) => (
        <RailButton key={v} active={view === v} label={label} onPress={() => switchTo(v)}>
          <Icon size={22} stroke={1.6} />
        </RailButton>
      ))}
      <div className="flex-1" />
      <RailButton
        active={view === "settings"}
        label="Settings"
        onPress={() => switchTo("settings")}
      >
        <IconSettings size={22} stroke={1.6} />
      </RailButton>
    </nav>
  );
}

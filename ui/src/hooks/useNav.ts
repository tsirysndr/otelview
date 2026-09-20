import { useSetAtom } from "jotai";
import {
  IconAlignLeft,
  IconChartLine,
  IconRoute,
  IconSettings,
  IconTopologyStar3,
} from "@tabler/icons-react";
import {
  openTraceIdAtom,
  selectedLogAtom,
  selectedSpanIdAtom,
  viewAtom,
  type View,
} from "../state/atoms";

export interface NavItem {
  view: View;
  label: string;
  icon: typeof IconRoute;
}

/** Primary destinations — shared by the desktop icon rail and the mobile
 * bottom nav so they can't drift out of sync. */
export const NAV_ITEMS: NavItem[] = [
  { view: "traces", label: "Traces", icon: IconRoute },
  { view: "logs", label: "Logs", icon: IconAlignLeft },
  { view: "metrics", label: "Metrics", icon: IconChartLine },
  { view: "services", label: "Services", icon: IconTopologyStar3 },
];

export const SETTINGS_NAV_ITEM: NavItem = {
  view: "settings",
  label: "Settings",
  icon: IconSettings,
};

/** Switches the active view and clears whatever trace/span/log selection
 * belonged to the view being left. */
export function useSwitchView() {
  const setView = useSetAtom(viewAtom);
  const setOpenTrace = useSetAtom(openTraceIdAtom);
  const setSelectedSpan = useSetAtom(selectedSpanIdAtom);
  const setSelectedLog = useSetAtom(selectedLogAtom);

  return (v: View) => {
    setView(v);
    if (v !== "traces") setOpenTrace(null);
    setSelectedSpan(null);
    setSelectedLog(null);
  };
}

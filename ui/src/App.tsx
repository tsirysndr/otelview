import { useEffect } from "react";
import { useAtomValue } from "jotai";
import { apiSettingsAtom, railVisibleAtom, themeAtom, viewAtom } from "./state/atoms";
import { setApiConfig } from "./lib/api";
import { applyTheme } from "./theme";
import { useShortcuts } from "./hooks/useShortcuts";
import { AuthGate } from "./components/AuthGate";
import { TopBar } from "./components/TopBar";
import { IconRail } from "./components/IconRail";
import { BottomNav } from "./components/BottomNav";
import { StatusLine } from "./components/StatusLine";
import { Inspector } from "./components/Inspector";
import { CommandPalette } from "./components/CommandPalette";
import { HelpModal } from "./components/HelpModal";
import { TracesView } from "./views/TracesView";
import { LogsView } from "./views/LogsView";
import { MetricsView } from "./views/MetricsView";
import { ServicesView } from "./views/ServicesView";
import { SettingsView } from "./views/SettingsView";

export default function App() {
  const theme = useAtomValue(themeAtom);
  const view = useAtomValue(viewAtom);
  const rail = useAtomValue(railVisibleAtom);
  const apiSettings = useAtomValue(apiSettingsAtom);

  // Keep the module-level config the fetch client reads in sync,
  // synchronously, so queries fired on mount see the right base URL.
  setApiConfig(apiSettings);

  useEffect(() => applyTheme(theme), [theme]);
  useShortcuts();

  return (
    <div className="flex h-[100dvh] flex-col overflow-hidden bg-background text-foreground">
      <AuthGate>
        <TopBar />
        <div className="flex min-h-0 flex-1">
          {rail && <IconRail />}
          <main className="min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
            {view === "traces" && <TracesView />}
            {view === "logs" && <LogsView />}
            {view === "metrics" && <MetricsView />}
            {view === "services" && <ServicesView />}
            {view === "settings" && <SettingsView />}
          </main>
          <Inspector />
        </div>
        <BottomNav />
        <StatusLine />
        <CommandPalette />
        <HelpModal />
      </AuthGate>
    </div>
  );
}

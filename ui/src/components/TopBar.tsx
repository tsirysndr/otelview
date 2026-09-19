import { Button, Switch, Tooltip } from "@heroui/react";
import { IconMoon, IconRefresh, IconSun } from "@tabler/icons-react";
import { useAtom } from "jotai";
import { useQueryClient } from "@tanstack/react-query";
import { liveAtom, lookbackAtom, themeAtom } from "../state/atoms";
import { Logo } from "./Logo";

const LOOKBACKS = ["5m", "15m", "1h", "6h", "24h", "7d", "all"];

export function TopBar() {
  const [theme, setTheme] = useAtom(themeAtom);
  const [lookback, setLookback] = useAtom(lookbackAtom);
  const [live, setLive] = useAtom(liveAtom);
  const qc = useQueryClient();

  return (
    <header className="flex h-11 shrink-0 items-center gap-3 border-b border-divider bg-content1 px-3">
      <Logo />
      <div className="flex-1" />

      {/* Global time range (Datadog-style) */}
      <div className="flex items-center gap-1 rounded-lg bg-content2 p-0.5">
        {LOOKBACKS.map((lb) => (
          <button
            key={lb}
            onClick={() => setLookback(lb)}
            className={`rounded-md px-2 py-0.5 text-xs transition-colors ${
              lookback === lb
                ? "bg-content4 text-foreground"
                : "text-default-500 hover:text-foreground"
            }`}
          >
            {lb}
          </button>
        ))}
      </div>

      <Tooltip content="Auto-refresh every 3s" delay={400}>
        <div className="flex items-center gap-1.5">
          <Switch size="sm" isSelected={live} onValueChange={setLive} aria-label="Live" />
          <span className={`text-xs ${live ? "text-neon-green" : "text-default-500"}`}>
            live
          </span>
        </div>
      </Tooltip>

      <Button
        isIconOnly
        size="sm"
        variant="light"
        aria-label="Refresh"
        onPress={() => qc.invalidateQueries()}
      >
        <IconRefresh size={17} />
      </Button>
      <Button
        isIconOnly
        size="sm"
        variant="light"
        aria-label="Toggle theme"
        onPress={() => setTheme(theme === "dark" ? "light" : "dark")}
      >
        {theme === "dark" ? <IconSun size={17} /> : <IconMoon size={17} />}
      </Button>
    </header>
  );
}

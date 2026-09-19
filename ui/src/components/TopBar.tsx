import { Button, Switch, Tooltip } from "@heroui/react";
import {
  IconLayoutSidebar,
  IconLayoutSidebarFilled,
  IconLayoutSidebarRight,
  IconLayoutSidebarRightFilled,
  IconMoon,
  IconRefresh,
  IconSearch,
  IconSun,
} from "@tabler/icons-react";
import { useAtom, useSetAtom } from "jotai";
import { useQueryClient } from "@tanstack/react-query";
import {
  inspectorOpenAtom,
  liveAtom,
  paletteOpenAtom,
  railVisibleAtom,
  themeAtom,
} from "../state/atoms";
import { isMac, isTauri } from "../lib/inputProps";
import { TimeRangePicker } from "./TimeRangePicker";

export function TopBar() {
  const [theme, setTheme] = useAtom(themeAtom);
  const [live, setLive] = useAtom(liveAtom);
  const [rail, setRail] = useAtom(railVisibleAtom);
  const [inspector, setInspector] = useAtom(inspectorOpenAtom);
  const setPaletteOpen = useSetAtom(paletteOpenAtom);
  const qc = useQueryClient();

  // On macOS in Tauri the native traffic lights overlay the top-left corner,
  // so the bar doubles as the drag region and leaves room for them.
  const macDesktop = isTauri() && isMac();

  return (
    <header
      data-tauri-drag-region
      className={`flex h-11 shrink-0 items-center gap-3 border-b border-divider bg-content1 pr-3 ${
        macDesktop ? "pl-20" : "pl-2"
      }`}
    >
      <span
        data-tauri-drag-region
        className="pointer-events-none select-none text-sm font-semibold tracking-widest"
      >
        otel<span className="text-neon-magenta">view</span>
      </span>

      <div data-tauri-drag-region className="flex-1" />

      {/* Global time range */}
      <TimeRangePicker />

      <Tooltip content="Auto-refresh (l)" delay={400}>
        <div className="flex items-center gap-1.5">
          <Switch size="sm" isSelected={live} onValueChange={setLive} aria-label="Live" />
          <span className={`text-xs ${live ? "text-neon-green" : "text-default-500"}`}>
            live
          </span>
        </div>
      </Tooltip>

      <Button
        size="sm"
        variant="flat"
        radius="sm"
        startContent={<IconSearch size={15} />}
        onPress={() => setPaletteOpen(true)}
      >
        <span className="hidden sm:inline">Search</span>
        <kbd className="neon-key ml-1">/</kbd>
      </Button>

      <Button
        isIconOnly
        size="sm"
        variant="light"
        radius="sm"
        aria-label="Refresh"
        onPress={() => qc.invalidateQueries()}
      >
        <IconRefresh size={16} />
      </Button>
      <Tooltip content={rail ? "Hide rail (⌘B)" : "Show rail (⌘B)"} delay={400}>
        <Button
          isIconOnly
          size="sm"
          variant="light"
          radius="sm"
          aria-label="Toggle left rail"
          className="text-default-500 data-[hover=true]:text-secondary"
          onPress={() => setRail(!rail)}
        >
          {rail ? <IconLayoutSidebarFilled size={18} /> : <IconLayoutSidebar size={18} />}
        </Button>
      </Tooltip>
      <Tooltip
        content={inspector ? "Hide inspector (⌘J)" : "Show inspector (⌘J)"}
        delay={400}
      >
        <Button
          isIconOnly
          size="sm"
          variant="light"
          radius="sm"
          aria-label="Toggle inspector panel"
          className="text-default-500 data-[hover=true]:text-secondary"
          onPress={() => setInspector(!inspector)}
        >
          {inspector ? (
            <IconLayoutSidebarRightFilled size={18} />
          ) : (
            <IconLayoutSidebarRight size={18} />
          )}
        </Button>
      </Tooltip>
      <Button
        isIconOnly
        size="sm"
        variant="light"
        radius="sm"
        aria-label="Toggle theme"
        onPress={() => setTheme(theme === "dark" ? "light" : "dark")}
      >
        {theme === "dark" ? <IconSun size={16} /> : <IconMoon size={16} />}
      </Button>
    </header>
  );
}

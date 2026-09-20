import { useAtomValue } from "jotai";
import { viewAtom } from "../../state/atoms";
import { NAV_ITEMS, SETTINGS_NAV_ITEM, useSwitchView, type NavItem } from "../../hooks/useNav";

function RailButton({
  active,
  label,
  onPress,
  icon: Icon,
}: {
  active: boolean;
  label: string;
  onPress: () => void;
  icon: NavItem["icon"];
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

/** Left sidebar on desktop (lg+): always shows icon + title, never
 * minified. Hidden on mobile/tablet, where BottomNav takes over. */
export function IconRail() {
  const view = useAtomValue(viewAtom);
  const switchTo = useSwitchView();

  return (
    <nav className="hidden w-52 shrink-0 flex-col border-r border-divider bg-content1 py-1.5 lg:flex">
      {NAV_ITEMS.map(({ view: v, label, icon }) => (
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
        active={view === SETTINGS_NAV_ITEM.view}
        label={SETTINGS_NAV_ITEM.label}
        icon={SETTINGS_NAV_ITEM.icon}
        onPress={() => switchTo(SETTINGS_NAV_ITEM.view)}
      />
    </nav>
  );
}

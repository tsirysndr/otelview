import { useAtomValue } from "jotai";
import { viewAtom } from "../../state/atoms";
import { NAV_ITEMS, SETTINGS_NAV_ITEM, useSwitchView } from "../../hooks/useNav";

const ITEMS = [...NAV_ITEMS, SETTINGS_NAV_ITEM];

/** Bottom tab bar shown in place of the icon rail on mobile and tablet
 * (below lg) — always visible there, unlike the desktop rail which the
 * user can hide with ⌘B. */
export function BottomNav() {
  const view = useAtomValue(viewAtom);
  const switchTo = useSwitchView();

  return (
    <nav
      className="flex shrink-0 items-stretch border-t border-divider bg-content1 pb-[env(safe-area-inset-bottom)] lg:hidden"
      aria-label="Primary"
    >
      {ITEMS.map(({ view: v, label, icon: Icon }) => {
        const active = view === v;
        return (
          <button
            key={v}
            onClick={() => switchTo(v)}
            aria-label={label}
            aria-current={active ? "page" : undefined}
            className={`flex flex-1 flex-col items-center justify-center gap-0.5 py-1.5 text-[10px] transition-colors ${
              active ? "text-neon-cyan" : "text-default-500 hover:text-foreground"
            }`}
          >
            <Icon size={19} stroke={1.6} />
            <span>{label}</span>
          </button>
        );
      })}
    </nav>
  );
}

import { IconChevronDown } from "@tabler/icons-react";

export interface FilterOption {
  value: string;
  label: string;
}

/** Native <select> styled like the bordered inputs. Used in filter bars
 * instead of HeroUI Select: no floating-label layers, no ghost text, and
 * selection always applies. */
export function FilterSelect({
  value,
  onChange,
  options,
  ariaLabel,
}: {
  value: string;
  onChange: (value: string) => void;
  options: FilterOption[];
  ariaLabel: string;
}) {
  return (
    <div className="relative w-full">
      <select
        aria-label={ariaLabel}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="h-8 w-full cursor-pointer appearance-none rounded-small border-2 border-default-300 bg-transparent py-0 pl-2 pr-7 text-xs text-foreground outline-none transition-colors hover:border-default-400 focus:border-default-foreground"
      >
        {options.map((o) => (
          <option
            key={o.value}
            value={o.value}
            className="bg-content1 text-foreground"
          >
            {o.label}
          </option>
        ))}
      </select>
      <IconChevronDown
        size={14}
        className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-default-400"
      />
    </div>
  );
}

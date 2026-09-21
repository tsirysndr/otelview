import { Autocomplete, AutocompleteItem } from "@heroui/react";
import type { FilterOption } from "../FilterSelect";

/** Filter-bar dropdown with type-to-filter, for the lists that grow with the
 * deployment — services and operations run to dozens of entries, which is
 * more than a plain select is comfortable to scan.
 *
 * Takes the same options as [`FilterSelect`], including its catch-all entry,
 * but does not render that entry as a row. react-aria has no key meaning "no
 * selection" other than null, so the catch-all is modelled as the *cleared*
 * state: its label becomes the placeholder and clearing means "all".
 *
 * Carrying it as a real option instead needs a sentinel key, and a sentinel
 * has to be kept out of the text box — which means controlling the input,
 * which reopens the menu on every programmatic change, which clears the box
 * again. Autocomplete exposes no controlled open state to break that loop,
 * so this version lets the component own its own.
 *
 * Known rough edge: with a value already chosen the box shows its label, so
 * typing over it appends rather than replacing. Selecting the text on focus
 * fixes that but stops the menu opening at all — any focus-time text
 * manipulation does — so the clear button next to it is the way to start a
 * fresh query.
 *
 * [`FilterSelect`] still suits the short fixed enums (severity, aggregation)
 * where filtering would be pure overhead. */
export function FilterAutocomplete({
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
  const all = options.find((o) => o.value === "");
  const items = options.filter((o) => o.value !== "");

  return (
    <Autocomplete
      aria-label={ariaLabel}
      size="sm"
      variant="bordered"
      radius="sm"
      // Opens on focus so a click is enough. Safe now that the input is
      // uncontrolled — this only looped when programmatic text changes kept
      // re-triggering it.
      menuTrigger="focus"
      selectedKey={value === "" ? null : value}
      onSelectionChange={(key) => onChange(key === null ? "" : String(key))}
      allowsCustomValue={false}
      // The clear button is how you get back to the catch-all. onClear is
      // separate from onSelectionChange: clearing empties the text without
      // reporting a selection change, so without this the box would look
      // reset while the filter stayed applied.
      isClearable
      onClear={() => onChange("")}
      classNames={{ base: "w-full" }}
      inputProps={{
        placeholder: all?.label,
        classNames: {
          inputWrapper:
            "h-8 min-h-8 border-2 border-default-300 bg-transparent " +
            "data-[hover=true]:border-default-400 group-data-[focus=true]:border-default-500",
          input: "text-xs",
        },
      }}
      // Same as the range picker: the scale-in entrance reads as a flicker
      // on a list this small, so the menu just appears.
      popoverProps={{
        disableAnimation: true,
        classNames: { content: "border border-content3 bg-content1" },
      }}
      listboxProps={{ itemClasses: { base: "text-xs data-[selected=true]:text-neon-cyan" } }}
    >
      {items.map((o) => (
        <AutocompleteItem key={o.value} textValue={o.label}>
          {o.label}
        </AutocompleteItem>
      ))}
    </Autocomplete>
  );
}

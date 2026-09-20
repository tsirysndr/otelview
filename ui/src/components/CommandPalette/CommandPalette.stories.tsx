import type { Meta, StoryObj } from "@storybook/react";
import { useHydrateAtoms } from "jotai/utils";
import { paletteOpenAtom } from "../../state/atoms";
import { CommandPalette } from "./CommandPalette";

const meta: Meta = { title: "Shell/CommandPalette" };
export default meta;

/** Pre-set jotai atoms for a story (the preview provides a fresh store). */
function Hydrate({
  values,
  children,
}: {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  values: [any, any][];
  children: React.ReactNode;
}) {
  useHydrateAtoms(values);
  return <>{children}</>;
}

export const Open: StoryObj = {
  render: () => (
    <Hydrate values={[[paletteOpenAtom, true]]}>
      <CommandPalette />
    </Hydrate>
  ),
};

import type { Meta, StoryObj } from "@storybook/react";
import { useHydrateAtoms } from "jotai/utils";
import { helpOpenAtom } from "../../state/atoms";
import { HelpModal } from "./HelpModal";

const meta: Meta = { title: "Shell/HelpModal" };
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
    <Hydrate values={[[helpOpenAtom, true]]}>
      <HelpModal />
    </Hydrate>
  ),
};

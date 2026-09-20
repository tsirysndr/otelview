import type { Meta, StoryObj } from "@storybook/react";
import { IconRail } from "./IconRail";

const meta: Meta = { title: "Shell/IconRail" };
export default meta;

export const Default: StoryObj = {
  render: () => (
    <div className="h-96 w-14 border border-divider">
      <IconRail />
    </div>
  ),
};

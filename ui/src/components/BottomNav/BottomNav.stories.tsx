import type { Meta, StoryObj } from "@storybook/react";
import { BottomNav } from "./BottomNav";

const meta: Meta = { title: "Shell/BottomNav" };
export default meta;

export const Default: StoryObj = {
  render: () => (
    <div className="w-96 border border-divider">
      <BottomNav />
    </div>
  ),
};

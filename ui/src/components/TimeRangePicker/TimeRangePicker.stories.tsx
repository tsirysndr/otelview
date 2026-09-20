import type { Meta, StoryObj } from "@storybook/react";
import { TimeRangePicker } from "./TimeRangePicker";

const meta: Meta = { title: "Shell/TimeRangePicker" };
export default meta;

export const AlignedLeft: StoryObj = {
  render: () => (
    <div className="flex justify-start">
      <TimeRangePicker align="left" />
    </div>
  ),
};

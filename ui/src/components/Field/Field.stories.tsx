import type { Meta, StoryObj } from "@storybook/react";
import { Field } from "./Field";

const meta: Meta = { title: "Primitives/Field" };
export default meta;

export const WithChild: StoryObj = {
  render: () => (
    <Field label="min duration (ms)" className="w-40">
      <input
        className="h-8 w-full rounded-small border-2 border-default-300 bg-transparent px-2 text-xs"
        placeholder="e.g. 300"
      />
    </Field>
  ),
};

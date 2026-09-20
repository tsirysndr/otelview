import type { Meta, StoryObj } from "@storybook/react";
import { GettingStarted } from "./GettingStarted";

const meta: Meta = { title: "Shell/GettingStarted" };
export default meta;

export const Default: StoryObj = { render: () => <GettingStarted /> };

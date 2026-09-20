import type { Meta, StoryObj } from "@storybook/react";
import { StatusLine } from "./StatusLine";

const meta: Meta = { title: "Shell/StatusLine" };
export default meta;

export const Default: StoryObj = { render: () => <StatusLine /> };

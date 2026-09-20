import type { Meta, StoryObj } from "@storybook/react";
import { TopBar } from "./TopBar";

const meta: Meta = { title: "Shell/TopBar" };
export default meta;

export const Default: StoryObj = { render: () => <TopBar /> };

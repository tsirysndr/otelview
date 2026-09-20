import type { Meta, StoryObj } from "@storybook/react";
import { Logo } from "./Logo";

const meta: Meta = { title: "Primitives/Logo" };
export default meta;

export const Default: StoryObj = { render: () => <Logo /> };

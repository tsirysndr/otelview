import type { Preview } from "@storybook/react";
import { HeroUIProvider } from "@heroui/react";
import React from "react";
import "@fontsource-variable/roboto-mono";
import "../src/index.css";

document.documentElement.classList.add("dark");

const preview: Preview = {
  decorators: [
    (Story) => (
      <HeroUIProvider>
        <div className="min-h-screen bg-background p-6 text-foreground">
          <Story />
        </div>
      </HeroUIProvider>
    ),
  ],
  parameters: {
    backgrounds: { disable: true },
  },
};

export default preview;

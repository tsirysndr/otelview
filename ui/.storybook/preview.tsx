import type { Preview } from "@storybook/react";
import { HeroUIProvider } from "@heroui/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Provider as JotaiProvider } from "jotai";
import React from "react";
import "@fontsource-variable/roboto-mono";
import "../src/index.css";

// Several components read app state (jotai) or fetch (react-query). Fresh
// providers per story keep state from leaking between stories; queries fail
// fast and quiet — a story shows the component, not the network.
const makeQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: { retry: false, refetchOnWindowFocus: false, staleTime: Infinity },
    },
  });

const preview: Preview = {
  // Dark is the product's default; the toolbar offers light for checking the
  // other theme. The class on <html> is what the Tailwind/HeroUI theming
  // keys on, exactly as in the app.
  globalTypes: {
    theme: {
      description: "Color theme",
      toolbar: {
        title: "Theme",
        icon: "mirror",
        items: [
          { value: "dark", title: "Dark (default)" },
          { value: "light", title: "Light" },
        ],
        dynamicTitle: true,
      },
    },
  },
  initialGlobals: { theme: "dark" },
  decorators: [
    (Story, context) => {
      const theme = context.globals.theme ?? "dark";
      document.documentElement.classList.remove("dark", "light");
      document.documentElement.classList.add(theme);
      return (
        <JotaiProvider>
          <QueryClientProvider client={makeQueryClient()}>
            <HeroUIProvider>
              <div className="min-h-screen bg-background p-6 text-foreground">
                <Story />
              </div>
            </HeroUIProvider>
          </QueryClientProvider>
        </JotaiProvider>
      );
    },
  ],
  parameters: {
    backgrounds: { disable: true },
  },
};

export default preview;

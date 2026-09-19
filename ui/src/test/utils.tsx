import { HeroUIProvider } from "@heroui/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import { Provider as JotaiProvider } from "jotai";
import type { ReactElement } from "react";

export function renderApp(ui: ReactElement) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
  });
  return render(
    <HeroUIProvider>
      <JotaiProvider>
        <QueryClientProvider client={qc}>{ui}</QueryClientProvider>
      </JotaiProvider>
    </HeroUIProvider>,
  );
}

import { HeroUIProvider } from "@heroui/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import { createStore, Provider as JotaiProvider } from "jotai";
import type { ReactElement } from "react";

/** Render with fresh providers. The jotai store is returned so a test can
 * drive state the component does not own — arriving from another view, for
 * instance, which is otherwise only reachable by clicking through it. */
export function renderApp(ui: ReactElement) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
  });
  const store = createStore();
  return {
    store,
    ...render(
      <HeroUIProvider>
        <JotaiProvider store={store}>
          <QueryClientProvider client={qc}>{ui}</QueryClientProvider>
        </JotaiProvider>
      </HeroUIProvider>,
    ),
  };
}

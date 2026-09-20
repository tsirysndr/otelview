import type { Meta, StoryObj } from "@storybook/react";
import { useHydrateAtoms } from "jotai/utils";
import { inspectorOpenAtom, selectedLogAtom } from "../../state/atoms";
import { Inspector } from "./Inspector";

const meta: Meta = { title: "Shell/Inspector" };
export default meta;

/** Pre-set jotai atoms for a story (the preview provides a fresh store). */
function Hydrate({
  values,
  children,
}: {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  values: [any, any][];
  children: React.ReactNode;
}) {
  useHydrateAtoms(values);
  return <>{children}</>;
}

export const WithSelectedLog: StoryObj = {
  render: () => (
    <Hydrate
      values={[
        [inspectorOpenAtom, true],
        [
          selectedLogAtom,
          {
            time_unix_nano: 1_700_000_000_000_000_000,
            observed_time_unix_nano: 1_700_000_000_000_000_000,
            severity_number: 17,
            severity_text: "ERROR",
            body: "payment failed: card declined",
            attributes: { "http.route": "/checkout", retries: 2 },
            resource_attributes: { "service.name": "payments" },
            service_name: "payments",
            trace_id: "5b8aa5a2d2c872e8321cf37308d69df2",
            span_id: "051581bf3cb55c13",
            scope_name: "app",
          },
        ],
      ]}
    >
      <div className="flex h-96 justify-end border border-divider">
        <Inspector />
      </div>
    </Hydrate>
  ),
};

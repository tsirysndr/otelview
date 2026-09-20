import type { Meta, StoryObj } from "@storybook/react";
import { ScatterPlot } from "../ScatterPlot";
import type { TraceSummary } from "../../lib/api";

const meta: Meta<typeof ScatterPlot> = {
  title: "Trace/ScatterPlot",
  component: ScatterPlot,
};
export default meta;

type Story = StoryObj<typeof ScatterPlot>;

const NOW = 1_700_000_000_000_000_000;
const traces: TraceSummary[] = Array.from({ length: 30 }, (_, i) => ({
  trace_id: `t-${i}`,
  root_name: `GET /page/${i % 5}`,
  root_service: "frontend",
  start_time_unix_nano: NOW + i * 10_000_000_000,
  duration_nanos: (20 + ((i * 37) % 400)) * 1_000_000,
  span_count: 3 + (i % 9),
  error_count: i % 7 === 0 ? 1 : 0,
  services: ["frontend", "api"],
}));

export const ThirtyTraces: Story = {
  args: { traces, onOpen: () => {} },
};

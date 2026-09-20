import type { Meta, StoryObj } from "@storybook/react";
import { LineChart } from "../LineChart";

const meta: Meta<typeof LineChart> = {
  title: "Metrics/LineChart",
  component: LineChart,
};
export default meta;

type Story = StoryObj<typeof LineChart>;

const NOW = 1_700_000_000_000_000_000;
const mk = (offset: number, f: (i: number) => number) =>
  Array.from({ length: 40 }, (_, i) => ({
    t: NOW + i * 30_000_000_000,
    v: f(i) + offset,
  }));

export const MultiSeries: Story = {
  args: {
    unit: "req/s",
    series: [
      { label: "frontend · /checkout", points: mk(20, (i) => 10 * Math.sin(i / 4) + i / 3) },
      { label: "frontend · /search", points: mk(8, (i) => 6 * Math.cos(i / 5)) },
      { label: "api · /v1/orders", points: mk(2, (i) => (i % 7)) },
    ],
  },
};

export const SingleSeries: Story = {
  args: {
    unit: "%",
    series: [{ label: "cpu", points: mk(30, (i) => 15 * Math.sin(i / 3)) }],
  },
};

import type { Meta, StoryObj } from "@storybook/react";
import type { LogBucket } from "../../lib/api";
import { LogHistogram } from "../LogHistogram";

const meta: Meta<typeof LogHistogram> = {
  title: "Logs/LogHistogram",
  component: LogHistogram,
};
export default meta;

type Story = StoryObj<typeof LogHistogram>;

const NOW = 1_700_000_000_000_000_000;
const bucket = (i: number, overrides: Partial<LogBucket>): LogBucket => ({
  time_unix_nano: NOW + i * 60_000_000_000,
  trace: 0,
  debug: 0,
  info: 0,
  warn: 0,
  error: 0,
  fatal: 0,
  ...overrides,
});

export const QuietHour: Story = {
  args: {
    buckets: Array.from({ length: 60 }, (_, i) =>
      bucket(i, { info: 20 + Math.round(12 * Math.sin(i / 6)), debug: 4 }),
    ),
  },
};

export const ErrorSpike: Story = {
  args: {
    buckets: Array.from({ length: 60 }, (_, i) =>
      bucket(i, {
        info: 25,
        warn: i > 38 && i < 46 ? 18 : 1,
        error: i > 40 && i < 44 ? 60 : 0,
        fatal: i === 42 ? 6 : 0,
      }),
    ),
  },
};

export const SparseTraffic: Story = {
  args: {
    buckets: Array.from({ length: 30 }, (_, i) =>
      bucket(i, { info: i % 7 === 0 ? 3 : 0, error: i === 20 ? 1 : 0 }),
    ),
  },
};

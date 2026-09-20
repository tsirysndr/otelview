import type { Meta, StoryObj } from "@storybook/react";
import { Waterfall } from "../Waterfall";
import { spanFixtures } from "../../test/fixtures";
import type { SpanRecord } from "../../lib/api";

const meta: Meta<typeof Waterfall> = {
  title: "Trace/Waterfall",
  component: Waterfall,
};
export default meta;

type Story = StoryObj<typeof Waterfall>;

export const CheckoutTrace: Story = {
  args: { spans: spanFixtures },
};

const NOW = 1_700_000_000_000_000_000;
const deep: SpanRecord[] = [
  ["r", "", "gateway", "POST /orders", 0, 900, 0],
  ["a", "r", "orders", "create order", 50, 700, 0],
  ["b", "a", "inventory", "reserve stock", 80, 260, 0],
  ["c", "a", "payments", "charge card", 300, 640, 2],
  ["d", "c", "payments", "psp roundtrip", 340, 600, 2],
  ["e", "r", "notifications", "enqueue email", 720, 780, 0],
].map(([id, parent, svc, name, s, e, status]) => ({
  trace_id: "t-deep",
  span_id: String(id),
  parent_span_id: String(parent),
  name: String(name),
  service_name: String(svc),
  kind: "internal",
  start_time_unix_nano: NOW + Number(s) * 1_000_000,
  end_time_unix_nano: NOW + Number(e) * 1_000_000,
  status_code: Number(status),
  status_message: status === 2 ? "card declined" : "",
  attributes: {},
  resource_attributes: {},
  events: [],
  links: [],
  scope_name: "",
  scope_version: "",
}));

export const DeepTraceWithErrors: Story = {
  args: { spans: deep },
};

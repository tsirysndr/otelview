import type { Meta, StoryObj } from "@storybook/react";
import { ServiceMap } from "../ServiceMap";

const meta: Meta<typeof ServiceMap> = {
  title: "Services/ServiceMap",
  component: ServiceMap,
};
export default meta;

type Story = StoryObj<typeof ServiceMap>;

export const SmallFleet: Story = {
  args: {
    graph: {
      sampled_traces: 180,
      nodes: [
        { service: "gateway", span_count: 900, error_count: 4, avg_ms: 12 },
        { service: "api", span_count: 700, error_count: 0, avg_ms: 35 },
        { service: "db", span_count: 650, error_count: 2, avg_ms: 4 },
        { service: "cache", span_count: 300, error_count: 0, avg_ms: 1 },
      ],
      edges: [
        { source: "gateway", target: "api", calls: 700, errors: 4, avg_ms: 40 },
        { source: "api", target: "db", calls: 650, errors: 2, avg_ms: 5 },
        { source: "api", target: "cache", calls: 300, errors: 0, avg_ms: 1 },
      ],
    },
  },
};

export const ErrorHeavyDependency: Story = {
  args: {
    graph: {
      sampled_traces: 42,
      nodes: [
        { service: "scrobbler", span_count: 200, error_count: 40, avg_ms: 220 },
        { service: "musicbrainz", span_count: 180, error_count: 90, avg_ms: 800 },
        { service: "deezer", span_count: 120, error_count: 0, avg_ms: 90 },
      ],
      edges: [
        { source: "scrobbler", target: "musicbrainz", calls: 180, errors: 90, avg_ms: 800 },
        { source: "scrobbler", target: "deezer", calls: 120, errors: 0, avg_ms: 90 },
      ],
    },
  },
};

export const SingleService: Story = {
  args: {
    graph: {
      sampled_traces: 12,
      nodes: [{ service: "otelview", span_count: 40, error_count: 0, avg_ms: 3 }],
      edges: [],
    },
  },
};

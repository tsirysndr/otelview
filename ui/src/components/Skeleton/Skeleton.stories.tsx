import type { Meta, StoryObj } from "@storybook/react";
import {
  SkeletonCards,
  SkeletonChart,
  SkeletonHistogram,
  SkeletonList,
  SkeletonRows,
  SkeletonTable,
  SkeletonWaterfall,
} from "../Skeleton";

const meta: Meta = { title: "Loading/Skeletons" };
export default meta;

export const LogRows: StoryObj = { render: () => <SkeletonRows rows={10} /> };
export const SidebarList: StoryObj = { render: () => <SkeletonList rows={8} /> };
export const MetricChart: StoryObj = {
  render: () => (
    <div className="h-72">
      <SkeletonChart />
    </div>
  ),
};
export const Histogram: StoryObj = { render: () => <SkeletonHistogram /> };
export const ServiceCards: StoryObj = { render: () => <SkeletonCards cards={6} /> };
export const RedMetricsTable: StoryObj = { render: () => <SkeletonTable rows={6} cols={5} /> };
export const TraceWaterfall: StoryObj = { render: () => <SkeletonWaterfall rows={8} /> };

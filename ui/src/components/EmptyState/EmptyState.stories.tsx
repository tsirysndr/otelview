import type { Meta, StoryObj } from "@storybook/react";
import { IconAlignLeft, IconRoute } from "@tabler/icons-react";
import { EmptyState } from "./EmptyState";

const meta: Meta = { title: "Primitives/EmptyState" };
export default meta;

export const WithHint: StoryObj = {
  render: () => (
    <EmptyState
      icon={<IconRoute size={44} stroke={1.2} />}
      title="no traces yet"
      hint="Nothing matches the current filters and time range — or no spans have been received."
    />
  ),
};

export const WithoutIngestHelp: StoryObj = {
  render: () => (
    <EmptyState
      icon={<IconAlignLeft size={44} stroke={1.2} />}
      title="no log records yet"
      showIngestHelp={false}
    />
  ),
};

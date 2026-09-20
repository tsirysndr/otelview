import type { Meta, StoryObj } from "@storybook/react";
import { useHydrateAtoms } from "jotai/utils";
import {
  helpOpenAtom,
  inspectorOpenAtom,
  paletteOpenAtom,
  selectedLogAtom,
} from "../state/atoms";
import { CommandPalette } from "./CommandPalette";
import { GettingStarted } from "./GettingStarted";
import { HelpModal } from "./HelpModal";
import { IconRail } from "./IconRail";
import { Inspector } from "./Inspector";
import { StatusLine } from "./StatusLine";
import { TimeRangePicker } from "./TimeRangePicker";
import { TopBar } from "./TopBar";

const meta: Meta = { title: "Shell" };
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

export const TopBarDefault: StoryObj = { render: () => <TopBar /> };

export const IconRailDefault: StoryObj = {
  render: () => (
    <div className="h-96 w-14 border border-divider">
      <IconRail />
    </div>
  ),
};

export const StatusLineDefault: StoryObj = { render: () => <StatusLine /> };

export const TimeRange: StoryObj = {
  render: () => (
    <div className="flex justify-start">
      <TimeRangePicker align="left" />
    </div>
  ),
};

export const GettingStartedPanel: StoryObj = { render: () => <GettingStarted /> };

export const HelpOpen: StoryObj = {
  render: () => (
    <Hydrate values={[[helpOpenAtom, true]]}>
      <HelpModal />
    </Hydrate>
  ),
};

export const CommandPaletteOpen: StoryObj = {
  render: () => (
    <Hydrate values={[[paletteOpenAtom, true]]}>
      <CommandPalette />
    </Hydrate>
  ),
};

export const InspectorWithLog: StoryObj = {
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

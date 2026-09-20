import type { Meta, StoryObj } from "@storybook/react";
import { ServiceChip } from "./ServiceChip";

const meta: Meta = { title: "Primitives/ServiceChip" };
export default meta;

export const Fleet: StoryObj = {
  render: () => (
    <div className="flex flex-wrap items-center gap-2">
      {["rocksky-api", "scrobbler", "musicbrainz", "spotify-proxy", "jetstream"].map((s) => (
        <ServiceChip key={s} service={s} />
      ))}
    </div>
  ),
};

export const Small: StoryObj = { render: () => <ServiceChip service="deezer" small /> };

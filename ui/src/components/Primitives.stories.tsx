import type { Meta, StoryObj } from "@storybook/react";
import { IconAlignLeft, IconRoute } from "@tabler/icons-react";
import { useState } from "react";
import { EmptyState } from "./EmptyState";
import { Field } from "./Field";
import { FilterSelect } from "./FilterSelect";
import { KeyValueTable } from "./KeyValueTable";
import { Logo } from "./Logo";
import { ServiceChip } from "./ServiceChip";

const meta: Meta = { title: "Primitives" };
export default meta;

export const LogoMark: StoryObj = { render: () => <Logo /> };

export const ServiceChips: StoryObj = {
  render: () => (
    <div className="flex flex-wrap items-center gap-2">
      {["rocksky-api", "scrobbler", "musicbrainz", "spotify-proxy", "jetstream"].map((s) => (
        <ServiceChip key={s} service={s} />
      ))}
      <span className="text-xs text-default-500">small:</span>
      <ServiceChip service="deezer" small />
    </div>
  ),
};

function SelectHarness() {
  const [value, setValue] = useState("");
  return (
    <div className="flex max-w-md gap-2">
      <Field label="service" className="w-44">
        <FilterSelect
          ariaLabel="Service"
          value={value}
          onChange={setValue}
          options={[
            { value: "", label: "all services" },
            { value: "rocksky-api", label: "rocksky-api" },
            { value: "scrobbler", label: "scrobbler" },
          ]}
        />
      </Field>
    </div>
  );
}
export const LabelledSelect: StoryObj = { render: () => <SelectHarness /> };

export const AttributesTable: StoryObj = {
  render: () => (
    <div className="max-w-lg">
      <KeyValueTable
        data={{
          "http.method": "POST",
          "http.route": "/xrpc/app.rocksky.feed.getFeed",
          "http.status_code": 200,
          "mb.query.track": "Bohemian Rhapsody",
          nested: { retries: 2, source: "riff-mb" },
        }}
      />
    </div>
  ),
};

export const EmptyWithHint: StoryObj = {
  render: () => (
    <EmptyState
      icon={<IconRoute size={44} stroke={1.2} />}
      title="no traces yet"
      hint="Nothing matches the current filters and time range — or no spans have been received."
    />
  ),
};

export const EmptyWithoutIngestHelp: StoryObj = {
  render: () => (
    <EmptyState
      icon={<IconAlignLeft size={44} stroke={1.2} />}
      title="no log records yet"
      showIngestHelp={false}
    />
  ),
};

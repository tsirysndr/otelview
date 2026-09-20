import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";
import type { FieldInfo } from "../../lib/api";
import { TraceQlInput } from "./TraceQlInput";

const meta: Meta = { title: "Search/TraceQlInput" };
export default meta;

const FIELDS: FieldInfo[] = [
  {
    name: "http.method",
    count: 1240,
    top_values: [
      ["GET", 900],
      ["POST", 300],
    ],
  },
  {
    name: "http.status_code",
    count: 1240,
    top_values: [
      ["200", 1100],
      ["502", 90],
    ],
  },
];

function Harness({ initial, invalid }: { initial: string; invalid?: boolean }) {
  const [value, setValue] = useState(initial);
  return (
    <div className="max-w-xl">
      <TraceQlInput
        value={value}
        onChange={setValue}
        fields={FIELDS}
        invalid={invalid}
        placeholder="{ status = error && duration > 100ms }"
      />
    </div>
  );
}

export const Empty: StoryObj = {
  render: () => (
    <div className="max-w-xl">
      <p className="mb-2 text-xs text-default-500">
        focus the empty input — starter queries double as documentation
      </p>
      <Harness initial="" />
    </div>
  ),
};

export const SpansetWithIntrinsics: StoryObj = {
  render: () => <Harness initial="{ status = error && duration > 100ms }" />,
};

export const ScopedAttributesAndRegex: StoryObj = {
  render: () => (
    <Harness initial={'{ resource.service.name = "checkout" && name =~ "^GET" }'} />
  ),
};

export const SpansetOperatorsAndAggregates: StoryObj = {
  render: () => <Harness initial={'{ status = error } && {} | count() > 3'} />,
};

export const InvalidQuery: StoryObj = {
  render: () => <Harness initial="{ status = " invalid />,
};

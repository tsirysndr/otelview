import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";
import type { FieldInfo } from "../../lib/api";
import { KqlInput } from "./KqlInput";

const meta: Meta = { title: "Search/KqlInput" };
export default meta;

const FIELDS: FieldInfo[] = [
  {
    name: "http.method",
    count: 1240,
    top_values: [
      ["GET", 900],
      ["POST", 300],
      ["PUT", 40],
    ],
  },
  {
    name: "status_code",
    count: 1240,
    top_values: [
      ["200", 1100],
      ["500", 90],
      ["404", 50],
    ],
  },
  {
    name: "service.name",
    count: 1240,
    top_values: [
      ["rocksky-api", 700],
      ["scrobbler", 540],
    ],
  },
];

function Harness({ initial, invalid }: { initial: string; invalid?: boolean }) {
  const [value, setValue] = useState(initial);
  return (
    <div className="max-w-xl">
      <KqlInput
        value={value}
        onChange={setValue}
        fields={FIELDS}
        invalid={invalid}
        placeholder='http.method:POST and status_code:>=500 · body:"card declined"'
      />
    </div>
  );
}

export const Empty: StoryObj = { render: () => <Harness initial="" /> };

export const WithQuery: StoryObj = {
  render: () => (
    <Harness initial='http.method:POST and status_code:>=500 and body:"card declined"' />
  ),
};

export const InvalidQuery: StoryObj = {
  render: () => <Harness initial="status_code:>>=oops(" invalid />,
};

function WithHistoryHarness() {
  // Seed the popup's memory (same localStorage shape the jotai-backed
  // history atom reads), then focus the empty input to see it.
  localStorage.setItem(
    "otelview.history.storybook.kql",
    JSON.stringify([
      'service.name:"rocksky-api" and http.method:POST',
      "status_code:>=500",
    ]),
  );
  const [value, setValue] = useState("");
  return (
    <div className="max-w-xl">
      <p className="mb-2 text-xs text-default-500">
        focus the empty input — recently applied queries are offered back
      </p>
      <KqlInput value={value} onChange={setValue} fields={FIELDS} historyKey="storybook.kql" />
    </div>
  );
}
export const RecentQuerySuggestions: StoryObj = { render: () => <WithHistoryHarness /> };

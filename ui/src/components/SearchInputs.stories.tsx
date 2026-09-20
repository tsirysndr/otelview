import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";
import type { FieldInfo } from "../lib/api";
import { pushHistory } from "../lib/history";
import { AttrInput } from "./AttrInput";
import { KqlInput } from "./KqlInput";

const meta: Meta = { title: "Search/Inputs" };
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

function KqlHarness({ initial, invalid }: { initial: string; invalid?: boolean }) {
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

export const Empty: StoryObj = { render: () => <KqlHarness initial="" /> };

export const WithQuery: StoryObj = {
  render: () => (
    <KqlHarness initial='http.method:POST and status_code:>=500 and body:"card declined"' />
  ),
};

export const InvalidQuery: StoryObj = {
  render: () => <KqlHarness initial="status_code:>>=oops(" invalid />,
};

function WithHistoryHarness() {
  // Seed the popup's memory, then focus the empty input to see it.
  pushHistory("storybook.kql", "status_code:>=500");
  pushHistory("storybook.kql", 'service.name:"rocksky-api" and http.method:POST');
  const [value, setValue] = useState("");
  return (
    <div className="max-w-xl">
      <p className="mb-2 text-xs text-default-500">
        focus the empty input — recently applied queries are offered back
      </p>
      <KqlInput
        value={value}
        onChange={setValue}
        fields={FIELDS}
        historyKey="storybook.kql"
      />
    </div>
  );
}
export const RecentQuerySuggestions: StoryObj = { render: () => <WithHistoryHarness /> };

function AttrHarness({ initial }: { initial: string }) {
  const [value, setValue] = useState(initial);
  return (
    <div className="max-w-xl">
      <AttrInput
        value={value}
        onChange={setValue}
        fields={FIELDS}
        placeholder="http.method=GET or any text"
      />
    </div>
  );
}

export const TraceAttributeFilter: StoryObj = {
  render: () => <AttrHarness initial="http.method=GET" />,
};

import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";
import type { FieldInfo } from "../../lib/api";
import { LuceneInput, type LuceneSignal } from "./LuceneInput";

const meta: Meta = { title: "Search/LuceneInput" };
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

function Harness({
  initial,
  invalid,
  signal = "logs",
}: {
  initial: string;
  invalid?: boolean;
  signal?: LuceneSignal;
}) {
  const [value, setValue] = useState(initial);
  return (
    <div className="max-w-xl">
      <LuceneInput
        value={value}
        onChange={setValue}
        fields={FIELDS}
        signal={signal}
        invalid={invalid}
        placeholder='level:ERROR AND "card declined"'
      />
    </div>
  );
}

export const Empty: StoryObj = {
  render: () => (
    <div className="max-w-xl">
      <p className="mb-2 text-xs text-default-500">
        focus the empty input — the starters show the punctuation nobody
        remembers
      </p>
      <Harness initial="" />
    </div>
  ),
};

/** The starters differ per signal — a span has no `level`, a log no `kind`. */
export const TraceStarters: StoryObj = {
  render: () => (
    <div className="max-w-xl">
      <p className="mb-2 text-xs text-default-500">
        the same input on the traces side, with span-shaped starters
      </p>
      <Harness initial="" signal="traces" />
    </div>
  ),
};

export const FieldsAndBooleans: StoryObj = {
  render: () => <Harness initial="http.method:POST AND NOT level:DEBUG" />,
};

export const PhraseAndProximity: StoryObj = {
  render: () => <Harness initial={'"card declined" AND "payment order"~3'} />,
};

export const RangesAndWildcards: StoryObj = {
  render: () => <Harness initial="http.status_code:[500 TO *] AND service:pay*" />,
};

export const RequireExcludeAndFuzzy: StoryObj = {
  render: () => <Harness initial="+service:payments -level:DEBUG timeout~2" />,
};

export const InvalidQuery: StoryObj = {
  render: () => <Harness initial='"unterminated' invalid />,
};

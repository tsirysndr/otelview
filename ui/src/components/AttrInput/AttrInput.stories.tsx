import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";
import type { FieldInfo } from "../../lib/api";
import { AttrInput } from "./AttrInput";

const meta: Meta = { title: "Search/AttrInput" };
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

function Harness({ initial }: { initial: string }) {
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

export const Empty: StoryObj = { render: () => <Harness initial="" /> };
export const KeyValueFilter: StoryObj = { render: () => <Harness initial="http.method=GET" /> };

import type { Meta, StoryObj } from "@storybook/react";
import { KeyValueTable } from "./KeyValueTable";

const meta: Meta = { title: "Primitives/KeyValueTable" };
export default meta;

export const Attributes: StoryObj = {
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

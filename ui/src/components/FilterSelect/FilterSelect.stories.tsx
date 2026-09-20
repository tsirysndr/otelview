import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";
import { Field } from "../Field";
import { FilterSelect } from "./FilterSelect";

const meta: Meta = { title: "Primitives/FilterSelect" };
export default meta;

function Harness() {
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
export const Labelled: StoryObj = { render: () => <Harness /> };

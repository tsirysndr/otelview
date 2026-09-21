import type { Meta, StoryObj } from "@storybook/react";
import { createStore, Provider } from "jotai";
import { TimeRangePicker } from "./TimeRangePicker";
import { customRangeAtom } from "../../state/atoms";

const meta: Meta = { title: "Shell/TimeRangePicker" };
export default meta;

/** The quick-lookback pills, which is what the bar shows by default. */
export const Lookbacks: StoryObj = {
  render: () => (
    <div className="flex justify-start">
      <TimeRangePicker />
    </div>
  ),
};

/** With an absolute range set, the pills give way to HeroUI's
 * DateRangePicker — calendar, both ends and time of day in one control. */
export const CustomRange: StoryObj = {
  render: () => {
    const store = createStore();
    const to = Date.now();
    store.set(customRangeAtom, { from: to - 6 * 60 * 60 * 1000, to });
    return (
      <Provider store={store}>
        <div className="flex justify-start">
          <TimeRangePicker />
        </div>
      </Provider>
    );
  },
};

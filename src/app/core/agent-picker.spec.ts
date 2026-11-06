import { describe, expect, it } from "vitest";
import { AgentPicker } from "./agent-picker";

describe("AgentPicker", () => {
  const choices = [
    { paneId: "w1:p1", label: "claude" },
    { paneId: "w1:p2", label: "copilot" },
  ];

  it("resolves to the chosen pane and closes", async () => {
    const picker = new AgentPicker();
    const result = picker.pick(choices);
    expect(picker.choices()).toEqual(choices);
    picker.answer("w1:p2");
    expect(await result).toBe("w1:p2");
    expect(picker.choices()).toBeNull();
  });

  it("resolves to null when cancelled or replaced", async () => {
    const picker = new AgentPicker();
    const first = picker.pick(choices);
    const second = picker.pick(choices);
    expect(await first).toBeNull();
    picker.answer(null);
    expect(await second).toBeNull();
  });
});

import { describe, expect, it, vi } from "vitest";
import { ResultDialog } from "./result-dialog";

describe("ResultDialog", () => {
  it("shows and closes", () => {
    const d = new ResultDialog();
    d.show({ title: "t", summary: "s", text: "out" });
    expect(d.content()?.text).toBe("out");
    d.close();
    expect(d.content()).toBeNull();
  });

  it("closes before it runs the action", () => {
    const d = new ResultDialog();
    const run = vi.fn(() => expect(d.content()).toBeNull());
    d.show({ title: "t", summary: "s", text: "", action: { label: "Go", run } });
    d.runAction();
    expect(run).toHaveBeenCalledOnce();
  });
});

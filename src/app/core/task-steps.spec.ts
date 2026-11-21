import { describe, expect, it } from "vitest";
import { stepsAt } from "./task-steps";

describe("task steps", () => {
  it("marks the current step and the ones before it done", () => {
    expect(stepsAt("setup").map((s) => s.state)).toEqual(["done", "current", "pending"]);
    expect(stepsAt("worktree").map((s) => s.state)).toEqual(["current", "pending", "pending"]);
    expect(stepsAt(null).every((s) => s.state === "done")).toBe(true);
  });

  it("names the steps in order", () => {
    expect(stepsAt("worktree").map((s) => s.label)).toEqual([
      "Create worktree",
      "Run setup",
      "Start agent",
    ]);
  });
});

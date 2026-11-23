import { describe, expect, it } from "vitest";
import type { Terminal } from "../core/snapshot";
import { taskTerminals } from "./task-terminal";

const term = (terminal_id: string, extra: Partial<Terminal> = {}): Terminal => ({
  terminal_id,
  tab_id: "tab1",
  space_id: "s1",
  agent_status: "working",
  ...extra,
});

describe("taskTerminals", () => {
  const run = { run: "r1" };

  it("matches by run and task only", () => {
    const terminals = [
      term("t1", { run: "r1", task: "#7" }),
      term("t2", { run: "r1", task: "#8" }),
      term("t3", { run: "r2", task: "#7" }),
      term("t4", { label: "#7 r1" }),
    ];
    expect(taskTerminals(terminals, run, "#7").map((t) => t.terminal_id)).toEqual(["t1"]);
  });

  it("lists several newest first by creation, not by text order", () => {
    const terminals = [
      term("t9", { run: "r1", task: "#7" }),
      term("t10", { run: "r1", task: "#7" }),
      term("t2", { run: "r1", task: "#7" }),
    ];
    expect(taskTerminals(terminals, run, "#7").map((t) => t.terminal_id)).toEqual([
      "t10",
      "t9",
      "t2",
    ]);
  });

  it("finds nothing for a session poimen did not start or an empty task", () => {
    expect(taskTerminals([term("t1")], run, "#7")).toEqual([]);
    expect(taskTerminals([term("t1", { run: "r1", task: "#7" })], run, "")).toEqual([]);
  });

  it("uses the terminals of the run's machine only", () => {
    const local = term("t1", { run: "r1", task: "#7" });
    const remote = term("box::t1", { run: "r1", task: "#7", machine: "box" });
    expect(taskTerminals([local, remote], run, "#7")).toEqual([local]);
    expect(taskTerminals([local, remote], { run: "r1", machine: "box" }, "#7")).toEqual([remote]);
    expect(taskTerminals([local, remote], { run: "r1", machine: "other" }, "#7")).toEqual([]);
  });

  it("orders remote ids by their own counter", () => {
    const a = term("box::t3", { run: "r1", task: "#7", machine: "box" });
    const b = term("box::t12", { run: "r1", task: "#7", machine: "box" });
    expect(taskTerminals([a, b], { run: "r1", machine: "box" }, "#7")).toEqual([b, a]);
  });

  it("shows nothing for an offline machine", () => {
    const remote = term("box::t1", { run: "r1", task: "#7", machine: "box" });
    expect(taskTerminals([remote], { run: "r1", machine: "box", offline: true }, "#7")).toEqual([]);
  });
});

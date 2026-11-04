import { describe, expect, it } from "vitest";
import { MAIN_NODE_ID, buildFlowGraph, flowDuration, inputField } from "./flow-graph";
import { layoutFlow, FLOW_NODE_WIDTH } from "./flow-layout";
import type { MainCall, SubagentInfo, TranscriptEntry } from "./transcripts";

const at = (s: number) => new Date(Date.UTC(2026, 10, 2, 4, 0, s)).toISOString();

const agent = (id: string, over: Partial<SubagentInfo> = {}): SubagentInfo => ({
  agent_id: id,
  agent_type: "general-purpose",
  description: `Agent ${id}`,
  tool_use_id: `toolu_${id}`,
  spawn_depth: 1,
  worktree_branch: null,
  status: "running",
  started: at(10),
  last: at(20),
  ...over,
});

const mainCall = (id: string, name: string, over: Partial<MainCall> = {}): MainCall => ({
  tool_use_id: id,
  name,
  target: id,
  detail: "",
  started: at(1),
  ended: null,
  is_error: false,
  ...over,
});

const use = (id: string, name: string, input: object, s: number): TranscriptEntry => ({
  uuid: `u-${id}`,
  role: "assistant",
  timestamp: at(s),
  blocks: [{ type: "tool_use", id, name, input: JSON.stringify(input) }],
});

const result = (id: string, s: number, is_error = false): TranscriptEntry => ({
  uuid: `r-${id}`,
  role: "user",
  timestamp: at(s),
  blocks: [{ type: "tool_result", tool_use_id: id, text: "ok", is_error }],
});

const noEntries = new Map<string, TranscriptEntry[]>();
const node = (g: ReturnType<typeof buildFlowGraph>, id: string) =>
  g.nodes.find((n) => n.id === id)!;

describe("buildFlowGraph", () => {
  it("is just the main agent without calls", () => {
    const g = buildFlowGraph([], noEntries, { calls: [], tools: 0 });
    expect(g.nodes.map((n) => n.id)).toEqual([MAIN_NODE_ID]);
    expect(g.edges).toEqual([]);
    expect(g.nodes[0].status).toBe("done");
  });

  it("names the main agent after the followed agent, Claude Code by default", () => {
    const main = { calls: [], tools: 0 };
    expect(buildFlowGraph([], noEntries, main).nodes[0].type).toBe("Claude Code");
    const copilot = buildFlowGraph([], noEntries, main, "GitHub Copilot CLI");
    expect(copilot.nodes[0].type).toBe("GitHub Copilot CLI");
  });

  it("makes nodes of skill and agent calls only and counts the plain tools", () => {
    const g = buildFlowGraph(
      [agent("a")],
      new Map([
        ["a", [use("t1", "Bash", {}, 11), use("t2", "Edit", {}, 12), use("t3", "Read", {}, 13)]],
      ]),
      {
        calls: [
          mainCall("toolu_s", "Skill", { target: "implementation", detail: "104", ended: at(2) }),
          mainCall("toolu_a", "Agent", { started: at(5) }),
        ],
        tools: 7,
      },
    );
    expect(g.nodes.map((n) => n.kind)).toEqual(["main", "skill", "agent"]);
    expect(node(g, MAIN_NODE_ID).tools).toBe(7);
    expect(node(g, "toolu_s")).toMatchObject({
      title: "implementation",
      type: "104",
      status: "done",
    });
    // toolu_a has no subagent file yet, the subagent `a` (toolu_a) is its node.
    expect(g.nodes).toHaveLength(3);
  });

  it("links a call to its subagent by tool use id and reads status and times from the meta", () => {
    const g = buildFlowGraph(
      [agent("a", { status: "done", started: at(10), last: at(70), tool_use_id: "toolu_a" })],
      noEntries,
      { calls: [mainCall("toolu_a", "Agent", { started: at(9) })], tools: 0 },
    );
    expect(node(g, "toolu_a")).toMatchObject({
      title: "Agent a",
      type: "general-purpose",
      status: "done",
      agentId: "a",
    });
    expect(flowDuration(node(g, "toolu_a"), Date.parse(at(999)))).toBe(60_000);
  });

  it("chains sequential calls in call order", () => {
    const g = buildFlowGraph([], noEntries, {
      calls: [
        mainCall("s1", "Skill", { started: at(1), ended: at(2) }),
        mainCall("s2", "Skill", { started: at(5), ended: at(6) }),
      ],
      tools: 0,
    });
    expect(g.edges.map((e) => e.id)).toEqual([`${MAIN_NODE_ID}->s1`, "s1->s2"]);
  });

  it("puts parallel subagents side by side: both hang off the caller, the next call follows both", () => {
    const g = buildFlowGraph(
      [
        agent("a", { tool_use_id: "ta", started: at(10), last: at(50), status: "done" }),
        agent("b", { tool_use_id: "tb", started: at(10), last: at(60), status: "done" }),
      ],
      noEntries,
      {
        calls: [
          mainCall("ta", "Agent", { started: at(10) }),
          mainCall("tb", "Agent", { started: at(10) }),
          mainCall("s", "Skill", { started: at(70), ended: at(71) }),
        ],
        tools: 0,
      },
    );
    expect(g.edges.map((e) => e.id)).toEqual([
      `${MAIN_NODE_ID}->ta`,
      `${MAIN_NODE_ID}->tb`,
      "ta->s",
      "tb->s",
    ]);
    const layout = layoutFlow(g);
    expect(layout.get("ta")!.x).toBe(layout.get("tb")!.x);
    expect(layout.get("ta")!.y).not.toBe(layout.get("tb")!.y);
  });

  it("nests a subagent under the subagent that called it", () => {
    const g = buildFlowGraph(
      [
        agent("a", { tool_use_id: "ta", spawn_depth: 1 }),
        agent("b", { tool_use_id: "tb", spawn_depth: 2, started: at(15) }),
      ],
      new Map([["a", [use("tb", "Agent", { description: "Inner" }, 15)]]]),
      { calls: [mainCall("ta", "Agent", { started: at(9) })], tools: 0 },
    );
    expect(g.edges.map((e) => e.id)).toEqual([`${MAIN_NODE_ID}->ta`, "ta->tb"]);
    expect(node(g, "tb").agentId).toBe("b");
  });

  it("places a subagent without a known call by its spawn depth", () => {
    const g = buildFlowGraph(
      [
        agent("a", { tool_use_id: "ta", spawn_depth: 1, started: at(10) }),
        agent("b", { tool_use_id: "tb", spawn_depth: 2, started: at(20) }),
        agent("c", { tool_use_id: "tc", spawn_depth: 1, started: at(30) }),
      ],
      noEntries,
      { calls: [mainCall("ta", "Agent")], tools: 0 },
    );
    const edges = g.edges.map((e) => e.id);
    expect(edges).toContain("ta->tb");
    expect(edges).toContain(`${MAIN_NODE_ID}->tc`);
    expect(edges).not.toContain("ta->tc");
  });

  it("shows skills called by a subagent under it", () => {
    const g = buildFlowGraph(
      [agent("a", { tool_use_id: "ta" })],
      new Map([
        ["a", [use("sk", "Skill", { skill: "grill-me", args: "x" }, 12), result("sk", 13)]],
      ]),
      { calls: [mainCall("ta", "Agent")], tools: 0 },
    );
    expect(node(g, "sk")).toMatchObject({ kind: "skill", title: "grill-me", status: "done" });
    expect(g.edges.map((e) => e.id)).toContain("ta->sk");
  });

  it("derives running, done and failed", () => {
    const g = buildFlowGraph(
      [agent("a", { tool_use_id: "ta", status: "done" }), agent("b", { tool_use_id: "tb" })],
      noEntries,
      {
        calls: [
          mainCall("ta", "Agent", { started: at(1) }),
          mainCall("tb", "Agent", { started: at(1) }),
          mainCall("sk", "Skill", { started: at(30), ended: at(31), is_error: true }),
          mainCall("sr", "Skill", { started: at(60) }),
        ],
        tools: 0,
      },
    );
    expect(node(g, "ta").status).toBe("done");
    expect(node(g, "tb").status).toBe("running");
    expect(node(g, "sk").status).toBe("failed");
    expect(node(g, "sr").status).toBe("running");
    expect(node(g, MAIN_NODE_ID).status).toBe("running");
  });

  it("does not count the ack of a background launch inside a subagent as done", () => {
    const g = buildFlowGraph(
      [agent("a", { tool_use_id: "ta" })],
      new Map([["a", [use("tb", "Agent", { description: "Bg" }, 12), result("tb", 13)]]]),
      { calls: [mainCall("ta", "Agent")], tools: 0 },
    );
    expect(node(g, "tb").status).toBe("running");
  });

  it("marks a nested agent call with an error result as failed", () => {
    const g = buildFlowGraph(
      [agent("a", { tool_use_id: "ta" })],
      new Map([["a", [use("tb", "Agent", { description: "Bad" }, 12), result("tb", 13, true)]]]),
      { calls: [mainCall("ta", "Agent")], tools: 0 },
    );
    expect(node(g, "tb").status).toBe("failed");
  });

  it("is done when nothing runs and counts running time up to now", () => {
    const g = buildFlowGraph([], noEntries, {
      calls: [mainCall("sr", "Skill", { started: at(0) })],
      tools: 0,
    });
    expect(flowDuration(node(g, "sr"), Date.parse(at(5)))).toBe(5000);
    expect(flowDuration(node(g, MAIN_NODE_ID), 1)).toBeNull();
  });
});

describe("inputField", () => {
  it("reads complete and truncated tool input", () => {
    expect(inputField('{"skill":"a","args":"b"}', "args")).toBe("b");
    expect(inputField('{"description":"Say \\"hi\\"","prompt":"long te…', "description")).toBe(
      'Say "hi"',
    );
    expect(inputField("{}", "skill")).toBe("");
    expect(inputField("garbage", "skill")).toBe("");
  });
});

describe("layoutFlow", () => {
  it("places every node left to right in call order", () => {
    const g = buildFlowGraph([], noEntries, {
      calls: [
        mainCall("s1", "Skill", { started: at(1), ended: at(2) }),
        mainCall("s2", "Skill", { started: at(5), ended: at(6) }),
      ],
      tools: 0,
    });
    const p = layoutFlow(g);
    expect(p.size).toBe(3);
    expect(p.get("s1")!.x).toBeGreaterThanOrEqual(p.get(MAIN_NODE_ID)!.x + FLOW_NODE_WIDTH);
    expect(p.get("s2")!.x).toBeGreaterThan(p.get("s1")!.x);
  });
});

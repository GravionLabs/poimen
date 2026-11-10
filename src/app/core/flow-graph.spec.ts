import { describe, expect, it } from "vitest";
import {
  MAIN_NODE_ID,
  buildFlowGraph,
  flowContextLine,
  flowDuration,
  flowTokenLine,
  flowTooltip,
  inputField,
} from "./flow-graph";
import { layoutFlow, FLOW_NODE_HEIGHT, FLOW_NODE_WIDTH } from "./flow-layout";
import type {
  MainCall,
  SubagentInfo,
  TokenUsage,
  TranscriptEntry,
  UsageSample,
} from "./transcripts";

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

const tok = (input: number, output: number, cache_read = 0, cache_creation = 0): TokenUsage => ({
  input,
  output,
  cache_read,
  cache_creation,
});

const sample = (id: string, s: number, usage: TokenUsage): UsageSample => ({
  id,
  timestamp: at(s),
  model: "claude-opus-4-7",
  usage,
});

const withUsage = (e: TranscriptEntry, id: string, usage: TokenUsage): TranscriptEntry => ({
  ...e,
  message_id: id,
  model: "claude-haiku-4-5",
  usage,
});

describe("token usage per node", () => {
  it("sums the main agent and shows the context fill of its latest message", () => {
    const g = buildFlowGraph([], noEntries, {
      calls: [],
      tools: 0,
      usage: [sample("m1", 1, tok(2, 10, 1000, 100)), sample("m2", 5, tok(3, 20, 5000, 0))],
    });
    const main = node(g, MAIN_NODE_ID);
    expect(main.tokens).toEqual({
      input: 6105,
      output: 30,
      rawInput: 5,
      cacheRead: 6000,
      cacheCreation: 100,
    });
    expect(main.context).toBe(5003);
    expect(main.model).toBe("claude-opus-4-7");
    expect(flowTokenLine(main)).toBe("6.1k in · 30 out");
    expect(flowContextLine(main)).toBe("context 5k");
  });

  it("sums a skill from the messages between its call and its result", () => {
    const g = buildFlowGraph([], noEntries, {
      calls: [mainCall("sr", "Skill", { started: at(2), ended: at(8), context: 900 })],
      tools: 0,
      usage: [
        sample("m0", 2, tok(1, 1)), // the message that made the call is not the skill's
        sample("m1", 4, tok(10, 100, 50, 5)),
        sample("m2", 8, tok(20, 200)),
        sample("m3", 9, tok(40, 400)),
      ],
    });
    const skill = node(g, "sr");
    expect(skill.tokens).toEqual({
      input: 85,
      output: 300,
      rawInput: 30,
      cacheRead: 50,
      cacheCreation: 5,
    });
    expect(skill.context).toBe(900);
    expect(flowContextLine(skill)).toBe("context 900 at start");
  });

  it("sums a running skill up to the latest message", () => {
    const g = buildFlowGraph([], noEntries, {
      calls: [mainCall("sr", "Skill", { started: at(2) })],
      tools: 0,
      usage: [sample("m1", 4, tok(10, 100)), sample("m2", 9, tok(20, 200))],
    });
    expect(node(g, "sr").tokens?.output).toBe(300);
  });

  it("sums a subagent from its own transcript and counts a message of several lines once", () => {
    const a = agent("1", { tool_use_id: "ta" });
    const first = withUsage(use("x", "Bash", { command: "ls" }, 11), "msg_1", tok(1, 5, 100, 0));
    const second = withUsage(
      { ...use("y", "Bash", { command: "ls" }, 12), uuid: "u2" },
      "msg_1",
      tok(1, 50, 100, 0),
    );
    const third = withUsage(
      { ...use("z", "Read", {}, 15), uuid: "u3" },
      "msg_2",
      tok(2, 7, 300, 20),
    );
    const g = buildFlowGraph([a], new Map([["1", [first, second, third]]]), {
      calls: [mainCall("ta", "Agent", { context: 777 })],
      tools: 0,
      usage: [sample("m1", 1, tok(500, 500))],
    });
    const sub = node(g, "ta");
    expect(sub.tokens).toEqual({
      input: 423,
      output: 57,
      rawInput: 3,
      cacheRead: 400,
      cacheCreation: 20,
    });
    expect(sub.model).toBe("claude-haiku-4-5");
    expect(sub.context).toBe(777);
    // not counted again in the parent
    expect(node(g, MAIN_NODE_ID).tokens?.output).toBe(500);
    expect(node(g, MAIN_NODE_ID).tokens?.input).toBe(500);
  });

  it("takes the context fill at the start of a call made by a subagent from its message", () => {
    const a = agent("1", { tool_use_id: "ta" });
    const call = withUsage(
      use("sk", "Skill", { skill: "grill-me" }, 12),
      "msg_1",
      tok(1, 5, 4000, 90),
    );
    const g = buildFlowGraph([a], new Map([["1", [call]]]), {
      calls: [mainCall("ta", "Agent")],
      tools: 0,
    });
    expect(node(g, "sk").context).toBe(4091);
  });

  it("has no tokens, context or model without usage data (no zeros)", () => {
    const g = buildFlowGraph([agent("1")], new Map([["1", [use("x", "Bash", {}, 11)]]]), {
      calls: [mainCall("sr", "Skill"), mainCall("toolu_1", "Agent")],
      tools: 0,
    });
    for (const n of g.nodes) {
      expect(n.tokens).toBeNull();
      expect(n.context).toBeNull();
      expect(n.model).toBeNull();
      expect(flowTokenLine(n)).toBe("");
      expect(flowContextLine(n)).toBe("");
      expect(flowTooltip(n)).not.toContain("tokens");
    }
  });

  it("lists the four values, the context and the model in the tooltip", () => {
    const g = buildFlowGraph([], noEntries, {
      calls: [],
      tools: 0,
      usage: [sample("m1", 1, tok(2, 10, 1000, 100))],
    });
    expect(flowTooltip(node(g, MAIN_NODE_ID)).split("\n")).toEqual([
      "Main agent (main)",
      "Input tokens: 2",
      "Output tokens: 10",
      "Cache read tokens: 1,000",
      "Cache creation tokens: 100",
      "Context fill: 1.1k",
      "Model: claude-opus-4-7",
    ]);
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

describe("layoutFlow direction", () => {
  const g = buildFlowGraph([], noEntries, {
    calls: [mainCall("s1", "Skill", { started: at(1), ended: at(2) })],
    tools: 0,
  });

  it("goes down the page, not across, from top to bottom", () => {
    const p = layoutFlow(g, "TB");
    expect(p.get("s1")!.y).toBeGreaterThanOrEqual(p.get(MAIN_NODE_ID)!.y + FLOW_NODE_HEIGHT);
    expect(p.get("s1")!.x).toBe(p.get(MAIN_NODE_ID)!.x);
  });

  it("keeps left to right as the default", () => {
    const p = layoutFlow(g);
    expect(p.get("s1")!.y).toBe(p.get(MAIN_NODE_ID)!.y);
    expect(p.get("s1")!.x).toBeGreaterThan(p.get(MAIN_NODE_ID)!.x);
  });
});

import { describe, expect, it } from "vitest";
import {
  MAIN_NODE_ID,
  buildFlowGraph,
  flowStatusText,
  startedTerminals,
  withStartedTerminals,
} from "./flow-graph";
import type { Terminal } from "./snapshot";
import type { TokenTotals } from "./token-totals";

const term = (id: string, over: Partial<Terminal> = {}): Terminal => ({
  terminal_id: id,
  tab_id: "t1",
  space_id: "s1",
  agent_status: "working",
  label: `agent ${id}`,
  ...over,
});

const totals = (n: number): TokenTotals => ({
  input: n,
  output: 5,
  cache_read: 10,
  cache_creation: 20,
});

const base = () => buildFlowGraph([], new Map(), { calls: [], tools: 0 });

describe("startedTerminals", () => {
  const terminals = [
    term("a"),
    term("b", { started_by: "a", agent_status: "done" }),
    term("c", { started_by: "a" }),
    term("d", { started_by: "b" }),
  ];

  it("takes the terminals whose started_by is the starter, with their totals", () => {
    const got = startedTerminals("a", terminals, new Map([["b", totals(100)]]));
    expect(got.map((t) => t.terminalId)).toEqual(["b", "c"]);
    expect(got[0]).toMatchObject({ status: "done", closed: false, tokens: totals(100) });
    expect(got[1].tokens).toBeNull();
  });

  it("qualifies the starter of a remote terminal with its machine", () => {
    const remote = [term("m::b", { started_by: "a", machine: "m" })];
    expect(startedTerminals("m::a", remote, new Map())).toHaveLength(1);
    expect(startedTerminals("a", remote, new Map())).toHaveLength(0);
  });

  it("keeps a terminal that left the snapshot as closed with its last status", () => {
    const seen = startedTerminals("a", terminals, new Map([["c", totals(7)]]));
    const got = startedTerminals("a", [term("a")], new Map(), seen);
    expect(got.map((t) => [t.terminalId, t.closed, t.status])).toEqual([
      ["b", true, "done"],
      ["c", true, "working"],
    ]);
    expect(got[1].tokens).toEqual(totals(7));
  });
});

describe("withStartedTerminals", () => {
  it("returns the graph itself without started terminals", () => {
    const g = base();
    expect(withStartedTerminals(g, [])).toBe(g);
  });

  it("adds a terminal node with an edge from the main node", () => {
    const started = startedTerminals(
      "a",
      [term("b", { started_by: "a", agent_status: "blocked" })],
      new Map([["b", totals(100)]]),
    );
    const g = withStartedTerminals(base(), started);
    const node = g.nodes.find((n) => n.kind === "terminal")!;
    expect(node).toMatchObject({
      id: "terminal:b",
      title: "agent b",
      terminalId: "b",
      agentStatus: "blocked",
      status: "running",
      closed: false,
      tokens: { input: 130, output: 5, rawInput: 100, cacheRead: 10, cacheCreation: 20 },
    });
    expect(g.edges).toContainEqual({
      id: `${MAIN_NODE_ID}->terminal:b`,
      from: MAIN_NODE_ID,
      to: "terminal:b",
    });
  });

  it("maps agent status to node status and shows closed in the text", () => {
    const mk = (status: Terminal["agent_status"], closed = false) =>
      withStartedTerminals(base(), [
        { terminalId: "x", name: "x", status, tokens: null, closed },
      ]).nodes.at(-1)!;
    expect(mk("error").status).toBe("failed");
    expect(mk("done").status).toBe("done");
    expect(flowStatusText(mk("working"))).toBe("working");
    expect(flowStatusText(mk("working", true))).toBe("closed, last: working");
    expect(flowStatusText(base().nodes[0])).toBe("done");
  });
});

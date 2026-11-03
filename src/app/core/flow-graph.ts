import type { MainSnapshot, SubagentInfo, TranscriptEntry } from "./transcripts";

// The flow graph of an agent's run: the main agent, its skill calls and subagents (nested by who
// called them), built from the transcripts that `core/transcripts.ts` already holds. Pure; the
// layout is `flow-layout.ts`, the view `bottom-panel/flow-view.ts`.

export type FlowStatus = "running" | "done" | "failed";

export interface FlowNode {
  id: string;
  kind: "main" | "skill" | "agent";
  title: string;
  /** Agent type, skill arguments, or the agent product for the main node. */
  type: string;
  status: FlowStatus;
  startedMs: number | null;
  /** `null` while running (and for the main node). */
  endedMs: number | null;
  /** Plain tool calls (Bash, Edit, ...) of this agent; they are counted, not drawn. */
  tools: number;
  /** The subagent whose transcript a click opens; `null` for main and skills. */
  agentId: string | null;
}

export interface FlowEdge {
  id: string;
  from: string;
  to: string;
}

export interface FlowGraph {
  nodes: FlowNode[];
  edges: FlowEdge[];
}

export const MAIN_NODE_ID = "main";

interface Call {
  id: string;
  name: "Skill" | "Agent";
  target: string;
  detail: string;
  start: number | null;
  /** End of a skill call, or of a main-transcript agent call with a final result. */
  end: number | null;
  error: boolean;
}

const time = (ts: string | null): number | null => {
  const ms = ts ? Date.parse(ts) : NaN;
  return Number.isNaN(ms) ? null : ms;
};

/** A string field of a tool input, also from JSON cut off by the transcript size limit. */
export function inputField(input: string, key: string): string {
  try {
    const parsed: unknown = JSON.parse(input);
    if (typeof parsed === "object" && parsed !== null) {
      const value = (parsed as Record<string, unknown>)[key];
      return typeof value === "string" ? value : "";
    }
  } catch {
    // truncated: fall through to the pattern
  }
  const match = new RegExp(`"${key}"\\s*:\\s*"((?:[^"\\\\]|\\\\.)*)"`).exec(input);
  if (!match) return "";
  try {
    return JSON.parse(`"${match[1]}"`) as string;
  } catch {
    return match[1];
  }
}

/** Skill and agent calls of a subagent's messages, and how many other tools it called. */
function callsOf(entries: readonly TranscriptEntry[]): { calls: Call[]; tools: number } {
  const results = new Map<string, { at: number | null; error: boolean }>();
  for (const entry of entries)
    for (const block of entry.blocks)
      if (block.type === "tool_result")
        results.set(block.tool_use_id, { at: time(entry.timestamp), error: block.is_error });
  const calls: Call[] = [];
  let tools = 0;
  for (const entry of entries) {
    for (const block of entry.blocks) {
      if (block.type !== "tool_use") continue;
      if (block.name !== "Skill" && block.name !== "Agent") {
        tools++;
        continue;
      }
      const skill = block.name === "Skill";
      const result = results.get(block.id);
      calls.push({
        id: block.id,
        name: block.name,
        target: inputField(block.input, skill ? "skill" : "description"),
        detail: inputField(block.input, skill ? "args" : "subagent_type"),
        start: time(entry.timestamp),
        // An agent call's first result may be the acknowledgement of a background launch: the
        // status of an agent comes from its meta, not from here.
        end: skill ? (result?.at ?? null) : null,
        error: result?.error === true,
      });
    }
  }
  return { calls, tools };
}

/** Groups calls that overlap in time (parallel subagents) in call order. */
function groupCalls(calls: readonly Call[], endOf: (call: Call) => number | null): Call[][] {
  const groups: Call[][] = [];
  let reach = -Infinity;
  for (const call of [...calls].sort((a, b) => (a.start ?? 0) - (b.start ?? 0))) {
    const group = groups.at(-1);
    if (group && (call.start ?? 0) < reach) {
      group.push(call);
    } else {
      groups.push([call]);
      reach = -Infinity;
    }
    const end = endOf(call);
    reach = Math.max(reach, end ?? Infinity);
  }
  return groups;
}

/**
 * Builds the graph. Subagents are placed under the call that started them (`toolUseId`, found in
 * the main transcript or in a subagent's messages); one whose call is not known is placed under the
 * latest earlier subagent one level up (`spawnDepth`), else under the main agent.
 */
export function buildFlowGraph(
  agents: readonly SubagentInfo[],
  entries: ReadonlyMap<string, readonly TranscriptEntry[]>,
  main: MainSnapshot,
): FlowGraph {
  const byTool = new Map(agents.filter((a) => a.tool_use_id).map((a) => [a.tool_use_id, a]));
  const nodeId = (a: SubagentInfo) => a.tool_use_id || `agent:${a.agent_id}`;

  const owners = new Map<string, { calls: Call[]; tools: number }>();
  owners.set(MAIN_NODE_ID, {
    tools: main.tools,
    calls: main.calls.flatMap((c): Call[] =>
      c.name === "Skill" || c.name === "Agent"
        ? [
            {
              id: c.tool_use_id,
              name: c.name,
              target: c.target,
              detail: c.detail,
              start: time(c.started),
              end: time(c.ended),
              error: c.is_error,
            },
          ]
        : [],
    ),
  });
  const claimed = new Set(main.calls.map((c) => c.tool_use_id));
  for (const agent of agents) {
    const own = callsOf(entries.get(agent.agent_id) ?? []);
    owners.set(nodeId(agent), own);
    for (const call of own.calls) claimed.add(call.id);
  }

  // Subagents whose call nobody has shown: attach by depth.
  const sorted = [...agents].sort((a, b) => (time(a.started) ?? 0) - (time(b.started) ?? 0));
  for (const agent of sorted) {
    if (agent.tool_use_id && claimed.has(agent.tool_use_id)) continue;
    const start = time(agent.started);
    const parent = sorted
      .filter(
        (p) =>
          p !== agent &&
          p.spawn_depth === agent.spawn_depth - 1 &&
          (time(p.started) ?? 0) <= (start ?? 0),
      )
      .at(-1);
    owners.get(parent ? nodeId(parent) : MAIN_NODE_ID)?.calls.push({
      id: nodeId(agent),
      name: "Agent",
      target: agent.description,
      detail: agent.agent_type,
      start,
      end: null,
      error: false,
    });
  }

  const nodes: FlowNode[] = [];
  const edges: FlowEdge[] = [];
  const agentOf = (call: Call) => byTool.get(call.id) ?? agents.find((a) => nodeId(a) === call.id);
  const endOf = (call: Call): number | null => {
    const agent = call.name === "Agent" ? agentOf(call) : undefined;
    if (agent) return agent.status === "done" ? time(agent.last) : null;
    return call.end;
  };

  const place = (ownerId: string, seen: Set<string>) => {
    const owner = owners.get(ownerId);
    if (!owner) return;
    let previous: string[] = [ownerId];
    for (const group of groupCalls(owner.calls, endOf)) {
      for (const call of group) {
        if (seen.has(call.id)) continue;
        seen.add(call.id);
        const agent = call.name === "Agent" ? agentOf(call) : undefined;
        const node: FlowNode =
          call.name === "Skill"
            ? {
                id: call.id,
                kind: "skill",
                title: call.target || "Skill",
                type: call.detail,
                status: call.error ? "failed" : call.end !== null ? "done" : "running",
                startedMs: call.start,
                endedMs: call.end,
                tools: 0,
                agentId: null,
              }
            : {
                id: call.id,
                kind: "agent",
                title: agent?.description || call.target || agent?.agent_id || "Subagent",
                type: agent?.agent_type || call.detail,
                status: call.error
                  ? "failed"
                  : (agent ? agent.status === "done" : call.end !== null)
                    ? "done"
                    : "running",
                startedMs: (agent ? time(agent.started) : null) ?? call.start,
                endedMs: agent ? endOf(call) : call.end,
                tools: owners.get(call.id)?.tools ?? 0,
                agentId: agent?.agent_id ?? null,
              };
        nodes.push(node);
        for (const from of previous) edges.push({ id: `${from}->${call.id}`, from, to: call.id });
        if (call.name === "Agent") place(call.id, seen);
      }
      previous = group.map((c) => c.id);
    }
  };

  const mainNode: FlowNode = {
    id: MAIN_NODE_ID,
    kind: "main",
    title: "Main agent",
    type: "Claude Code",
    status: "done",
    startedMs: null,
    endedMs: null,
    tools: main.tools,
    agentId: null,
  };
  nodes.push(mainNode);
  place(MAIN_NODE_ID, new Set());
  if (nodes.some((n) => n.status === "running")) mainNode.status = "running";
  return { nodes, edges };
}

/** Elapsed milliseconds of a node: its run, or up to `now` while it runs. */
export function flowDuration(node: FlowNode, now: number): number | null {
  if (node.startedMs === null) return null;
  const end = node.endedMs ?? (node.status === "running" ? now : null);
  return end === null ? null : Math.max(0, end - node.startedMs);
}

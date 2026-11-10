import {
  formatTokens,
  type MainSnapshot,
  type SubagentInfo,
  type TokenUsage,
  type TranscriptEntry,
} from "./transcripts";

// The flow graph of an agent's run: the main agent, its skill calls and subagents (nested by who
// called them), built from the transcripts that `core/transcripts.ts` already holds. Pure; the
// layout is `flow-layout.ts`, the view `bottom-panel/flow-view.ts`.

export type FlowStatus = "running" | "done" | "failed";

/** Tokens of a node: the four counts and the derived total input. */
export interface NodeTokens {
  /** Total input: input + cache read + cache creation. */
  input: number;
  output: number;
  rawInput: number;
  cacheRead: number;
  cacheCreation: number;
}

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
  /** `null` when the transcript has no usage for the node (the row is hidden, never zeros). */
  tokens: NodeTokens | null;
  /** The model the transcript names for the node, if any. */
  model: string | null;
  /**
   * Context fill in tokens: at the latest message for the main node, when the node started for
   * skills and subagents; `null` when unknown.
   */
  context: number | null;
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
  /** Context fill of the message that made the call. */
  context: number | null;
}

/** Usage of one assistant message of an agent. */
interface Sample {
  at: number | null;
  model: string | null;
  usage: TokenUsage;
}

const contextOf = (u: TokenUsage): number => u.input + u.cache_read + u.cache_creation;

/** Sums samples; `null` for none (unknown, not zero). */
function sumTokens(samples: readonly Sample[]): NodeTokens | null {
  if (samples.length === 0) return null;
  const sum: NodeTokens = { input: 0, output: 0, rawInput: 0, cacheRead: 0, cacheCreation: 0 };
  for (const { usage } of samples) {
    sum.rawInput += usage.input;
    sum.output += usage.output;
    sum.cacheRead += usage.cache_read;
    sum.cacheCreation += usage.cache_creation;
  }
  sum.input = sum.rawInput + sum.cacheRead + sum.cacheCreation;
  return sum;
}

const modelOf = (samples: readonly Sample[]): string | null =>
  samples.filter((s) => s.model).at(-1)?.model ?? null;

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

/**
 * Samples of a subagent's entries: a message written in several lines counts once (the last
 * line carries the final output count).
 */
function samplesOfEntries(entries: readonly TranscriptEntry[]): Sample[] {
  const byMessage = new Map<string, Sample>();
  for (const entry of entries) {
    if (entry.role !== "assistant" || !entry.usage) continue;
    byMessage.set(entry.message_id || entry.uuid, {
      at: time(entry.timestamp),
      model: entry.model ?? null,
      usage: entry.usage,
    });
  }
  return [...byMessage.values()];
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
        context: entry.usage ? contextOf(entry.usage) : null,
      });
    }
  }
  return { calls, tools };
}

/**
 * Groups calls that overlap in time (parallel subagents) in call order. Only subagent calls run in
 * parallel: a skill runs inside its owner, so it always follows the calls before it, also while an
 * earlier subagent still runs (otherwise sequential skills lose the arrows between them, #626).
 */
function groupCalls(calls: readonly Call[], endOf: (call: Call) => number | null): Call[][] {
  const groups: Call[][] = [];
  let reach = -Infinity;
  for (const call of [...calls].sort((a, b) => (a.start ?? 0) - (b.start ?? 0))) {
    const group = groups.at(-1);
    const parallel =
      group &&
      call.name === "Agent" &&
      group.every((c) => c.name === "Agent") &&
      (call.start ?? 0) < reach;
    if (group && parallel) {
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
  agentName = "Claude Code",
): FlowGraph {
  const byTool = new Map(agents.filter((a) => a.tool_use_id).map((a) => [a.tool_use_id, a]));
  const nodeId = (a: SubagentInfo) => a.tool_use_id || `agent:${a.agent_id}`;

  const owners = new Map<string, { calls: Call[]; tools: number; samples: Sample[] }>();
  owners.set(MAIN_NODE_ID, {
    tools: main.tools,
    samples: (main.usage ?? []).map((u) => ({
      at: time(u.timestamp),
      model: u.model,
      usage: u.usage,
    })),
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
              context: c.context ?? null,
            },
          ]
        : [],
    ),
  });
  const claimed = new Set(main.calls.map((c) => c.tool_use_id));
  for (const agent of agents) {
    const list = entries.get(agent.agent_id) ?? [];
    const own = { ...callsOf(list), samples: samplesOfEntries(list) };
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
      context: null,
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
        // A skill runs inside its owner: its tokens are the owner's messages after the call
        // started, up to its result. A subagent has its own transcript.
        const start = call.start;
        const during =
          call.name === "Skill" && start !== null
            ? owner.samples.filter(
                (s) => s.at !== null && s.at > start && (call.end === null || s.at <= call.end),
              )
            : [];
        const own = call.name === "Agent" ? (owners.get(call.id)?.samples ?? []) : during;
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
                tokens: sumTokens(own),
                model: modelOf(own),
                context: call.context,
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
                tokens: sumTokens(own),
                model: modelOf(own),
                context: call.context,
              };
        nodes.push(node);
        for (const from of previous) edges.push({ id: `${from}->${call.id}`, from, to: call.id });
        if (call.name === "Agent") place(call.id, seen);
      }
      previous = group.map((c) => c.id);
    }
  };

  const mainSamples = owners.get(MAIN_NODE_ID)?.samples ?? [];
  const mainNode: FlowNode = {
    id: MAIN_NODE_ID,
    kind: "main",
    title: "Main agent",
    type: agentName,
    status: "done",
    startedMs: null,
    endedMs: null,
    tools: main.tools,
    agentId: null,
    tokens: sumTokens(mainSamples),
    model: modelOf(mainSamples),
    context: main.usage?.length ? contextOf(main.usage[main.usage.length - 1].usage) : null,
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

/** The token row of a node (`12.3k in · 1.2k out`), `""` without usage data. */
export function flowTokenLine(node: FlowNode): string {
  const t = node.tokens;
  return t ? `${formatTokens(t.input)} in · ${formatTokens(t.output)} out` : "";
}

/** The context fill of a node, `""` when unknown: latest for main, "at start" for the others. */
export function flowContextLine(node: FlowNode): string {
  if (node.context === null) return "";
  const text = `context ${formatTokens(node.context)}`;
  return node.kind === "main" ? text : `${text} at start`;
}

/** Tooltip of a node: the four counts, the context fill and the model, if known. */
export function flowTooltip(node: FlowNode): string {
  const t = node.tokens;
  const lines = [`${node.title} (${node.kind})`];
  if (t) {
    lines.push(
      `Input tokens: ${t.rawInput.toLocaleString("en-US")}`,
      `Output tokens: ${t.output.toLocaleString("en-US")}`,
      `Cache read tokens: ${t.cacheRead.toLocaleString("en-US")}`,
      `Cache creation tokens: ${t.cacheCreation.toLocaleString("en-US")}`,
    );
  }
  const context = flowContextLine(node);
  if (context) lines.push(`Context fill: ${context.replace("context ", "")}`);
  if (node.model) lines.push(`Model: ${node.model}`);
  return lines.join("\n");
}

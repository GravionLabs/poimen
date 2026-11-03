import { Injectable, computed, inject, signal } from "@angular/core";
import { TauriBridge } from "./tauri";

// Wire shapes of `src-tauri/src/transcripts.rs` (the `subagents` event and its commands).

export type TranscriptBlock =
  | { type: "text"; text: string }
  | { type: "thinking"; text: string }
  | { type: "tool_use"; id: string; name: string; input: string }
  | { type: "tool_result"; tool_use_id: string; text: string; is_error: boolean };

export interface TranscriptEntry {
  uuid: string;
  role: "user" | "assistant";
  timestamp: string | null;
  blocks: TranscriptBlock[];
}

export interface SubagentInfo {
  agent_id: string;
  agent_type: string;
  description: string;
  tool_use_id: string;
  spawn_depth: number;
  worktree_branch: string | null;
  status: "running" | "done";
  started: string | null;
  last: string | null;
}

export type SubagentUpdate =
  | { kind: "list"; agents: SubagentInfo[] }
  | { kind: "entries"; agent_id: string; entries: TranscriptEntry[] };

/** Payload of the `subagents` Tauri event. */
export interface SubagentEvent {
  session: string;
  update: SubagentUpdate;
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;
const str = (v: unknown): string => (typeof v === "string" ? v : "");

function parseBlock(raw: unknown): TranscriptBlock | null {
  if (!isObject(raw)) return null;
  switch (raw["type"]) {
    case "text":
    case "thinking":
      return { type: raw["type"], text: str(raw["text"]) };
    case "tool_use":
      return {
        type: "tool_use",
        id: str(raw["id"]),
        name: str(raw["name"]),
        input: str(raw["input"]),
      };
    case "tool_result":
      return {
        type: "tool_result",
        tool_use_id: str(raw["tool_use_id"]),
        text: str(raw["text"]),
        is_error: raw["is_error"] === true,
      };
    default:
      return null;
  }
}

function parseEntry(raw: unknown): TranscriptEntry | null {
  if (!isObject(raw) || (raw["role"] !== "user" && raw["role"] !== "assistant")) return null;
  const blocks = Array.isArray(raw["blocks"]) ? raw["blocks"] : [];
  return {
    uuid: str(raw["uuid"]),
    role: raw["role"],
    timestamp: typeof raw["timestamp"] === "string" ? raw["timestamp"] : null,
    blocks: blocks.map(parseBlock).filter((b): b is TranscriptBlock => b !== null),
  };
}

function parseInfo(raw: unknown): SubagentInfo | null {
  if (!isObject(raw) || typeof raw["agent_id"] !== "string" || !raw["agent_id"]) return null;
  const time = (v: unknown) => (typeof v === "string" ? v : null);
  return {
    agent_id: raw["agent_id"],
    agent_type: str(raw["agent_type"]),
    description: str(raw["description"]),
    tool_use_id: str(raw["tool_use_id"]),
    spawn_depth: typeof raw["spawn_depth"] === "number" ? raw["spawn_depth"] : 0,
    worktree_branch: typeof raw["worktree_branch"] === "string" ? raw["worktree_branch"] : null,
    status: raw["status"] === "done" ? "done" : "running",
    started: time(raw["started"]),
    last: time(raw["last"]),
  };
}

/** Reads a `subagents` event payload; anything unrecognized is `null`. */
export function parseSubagentEvent(raw: unknown): SubagentEvent | null {
  if (!isObject(raw) || typeof raw["session"] !== "string" || !isObject(raw["update"])) return null;
  const update = raw["update"];
  if (update["kind"] === "list" && Array.isArray(update["agents"])) {
    const agents = update["agents"].map(parseInfo).filter((a): a is SubagentInfo => a !== null);
    return { session: raw["session"], update: { kind: "list", agents } };
  }
  if (update["kind"] === "entries" && typeof update["agent_id"] === "string") {
    const list = Array.isArray(update["entries"]) ? update["entries"] : [];
    const entries = list.map(parseEntry).filter((e): e is TranscriptEntry => e !== null);
    return {
      session: raw["session"],
      update: { kind: "entries", agent_id: update["agent_id"], entries },
    };
  }
  return null;
}

/** Milliseconds a subagent ran: until its last line when done, until `now` while running. */
export function subagentDuration(info: SubagentInfo, now: number): number | null {
  const start = info.started ? Date.parse(info.started) : NaN;
  if (Number.isNaN(start)) return null;
  const end = info.status === "done" && info.last ? Date.parse(info.last) : now;
  return Number.isNaN(end) ? null : Math.max(0, end - start);
}

/** `850 ms`, `12 s`, `3 min 05 s`, `1 h 02 min`. */
export function formatDuration(ms: number): string {
  const seconds = Math.floor(ms / 1000);
  if (seconds < 1) return `${Math.round(ms)} ms`;
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min ${String(seconds % 60).padStart(2, "0")} s`;
  return `${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, "0")} min`;
}

/** Dropdown text: description, type, status and duration. */
export function subagentLabel(info: SubagentInfo, now: number): string {
  const duration = subagentDuration(info, now);
  return [
    info.description || info.agent_id,
    info.agent_type,
    info.status,
    duration === null ? "" : formatDuration(duration),
  ]
    .filter(Boolean)
    .join(" · ");
}

/** The subagent to show when none is picked: the newest running one, else the newest. */
export function defaultSubagent(agents: readonly SubagentInfo[]): string | null {
  const running = agents.filter((a) => a.status === "running");
  return (running.at(-1) ?? agents.at(-1))?.agent_id ?? null;
}

/** A transcript shaped for reading (and for other views of the panel, such as the flow tab). */
export type TranscriptItem =
  | {
      kind: "prompt" | "message" | "thinking";
      role: "user" | "assistant";
      text: string;
      time: string | null;
    }
  | {
      kind: "tool";
      id: string;
      name: string;
      input: string;
      /** `null` while the call has no result yet. */
      result: { text: string; isError: boolean } | null;
      time: string | null;
    };

/**
 * Turns entries into display items: the first user text is the prompt, a tool call absorbs the
 * result that answers it (by id), and a result without its call is dropped.
 */
export function transcriptItems(entries: readonly TranscriptEntry[]): TranscriptItem[] {
  const items: TranscriptItem[] = [];
  const calls = new Map<string, Extract<TranscriptItem, { kind: "tool" }>>();
  for (const entry of entries) {
    for (const block of entry.blocks) {
      if (block.type === "tool_use") {
        const item = {
          kind: "tool" as const,
          id: block.id,
          name: block.name,
          input: block.input,
          result: null,
          time: entry.timestamp,
        };
        calls.set(block.id, item);
        items.push(item);
      } else if (block.type === "tool_result") {
        const call = calls.get(block.tool_use_id);
        if (call) call.result = { text: block.text, isError: block.is_error };
      } else if (block.text.trim()) {
        const kind =
          block.type === "thinking"
            ? "thinking"
            : entry.role === "user" && items.length === 0
              ? "prompt"
              : "message";
        items.push({ kind, role: entry.role, text: block.text, time: entry.timestamp });
      }
    }
  }
  return items;
}

/** The last assistant message of a finished subagent is its report. */
export function reportIndex(items: readonly TranscriptItem[]): number {
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i];
    if (item.kind === "message" && item.role === "assistant") return i;
  }
  return -1;
}

/**
 * The subagents of the Claude Code agent in one pane and their transcripts. The backend watches
 * the transcript files and pushes `subagents` events; nothing here reads or polls files.
 */
@Injectable({ providedIn: "root" })
export class Subagents {
  private readonly bridge = inject(TauriBridge);
  private listening: Promise<unknown> | null = null;
  private session: string | null = null;
  private generation = 0;

  private readonly _agents = signal<readonly SubagentInfo[]>([]);
  private readonly _entries = signal<ReadonlyMap<string, readonly TranscriptEntry[]>>(new Map());
  private readonly _picked = signal<string | null>(null);
  private readonly _supported = signal(false);
  readonly agents = this._agents.asReadonly();
  /** `false` for a pane without a Claude Code session (other agents, plain terminals). */
  readonly supported = this._supported.asReadonly();
  /** The shown subagent: the picked one while it exists, else the default. */
  readonly selectedId = computed(() => {
    const picked = this._picked();
    return picked !== null && this._agents().some((a) => a.agent_id === picked)
      ? picked
      : defaultSubagent(this._agents());
  });
  readonly selected = computed(() => this._agents().find((a) => a.agent_id === this.selectedId()));
  readonly items = computed(() =>
    transcriptItems(this._entries().get(this.selectedId() ?? "") ?? []),
  );

  pick(id: string): void {
    this._picked.set(id);
  }

  /**
   * Follows the agent of `paneId` (`null` stops). Call again when the pane or its session id
   * changes; the previous state is dropped first.
   */
  async follow(paneId: string | null, session: string | null): Promise<void> {
    const generation = ++this.generation;
    this.session = null;
    this._agents.set([]);
    this._entries.set(new Map());
    this._picked.set(null);
    this._supported.set(false);
    this.listening ??= this.bridge.listen<unknown>("subagents", (raw) => this.apply(raw));
    await this.listening;
    if (generation !== this.generation) return;
    if (paneId === null || session === null) {
      await this.bridge.invoke("subagents_unwatch").catch(() => undefined);
      return;
    }
    this.session = session;
    try {
      const watching = await this.bridge.invoke<boolean>("subagents_watch", { paneId });
      if (generation === this.generation) this._supported.set(watching === true);
    } catch {
      if (generation === this.generation) this._supported.set(false);
    }
  }

  private apply(raw: unknown): void {
    const event = parseSubagentEvent(raw);
    if (!event || event.session !== this.session) return;
    const { update } = event;
    if (update.kind === "list") {
      this._agents.set(update.agents);
    } else {
      this._entries.update((map) => {
        const known = map.get(update.agent_id) ?? [];
        // A truncated file is read again from the start: skip what is already shown.
        const seen = new Set(known.map((e) => e.uuid).filter(Boolean));
        const fresh = update.entries.filter((e) => !e.uuid || !seen.has(e.uuid));
        return new Map(map).set(update.agent_id, [...known, ...fresh]);
      });
    }
  }
}

const SUMMARY_KEYS = [
  "command",
  "file_path",
  "path",
  "pattern",
  "url",
  "description",
  "prompt",
  "query",
];

/** One line for a tool call: the argument that says most (command, file, pattern), else the input. */
export function toolSummary(input: string, limit = 120): string {
  let text = input;
  try {
    const parsed: unknown = JSON.parse(input);
    if (isObject(parsed)) {
      const key = SUMMARY_KEYS.find((k) => typeof parsed[k] === "string" && parsed[k] !== "");
      if (key) text = parsed[key] as string;
    }
  } catch {
    // truncated JSON: show the raw text
  }
  const line = text.replace(/\s+/g, " ").trim();
  return line.length > limit ? `${line.slice(0, limit)}…` : line;
}

/** Whether a scrollable box shows its end (within a few pixels), so new lines may follow. */
export function atBottom(box: {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
}): boolean {
  return box.scrollHeight - box.scrollTop - box.clientHeight <= 8;
}

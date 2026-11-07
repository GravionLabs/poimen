import { DestroyRef, Injectable, computed, inject, signal } from "@angular/core";
import { TauriBridge } from "./tauri";

// Wire shapes of `src-tauri/src/transcripts.rs` (the `subagents` event and its commands).

export type TranscriptBlock =
  | { type: "text"; text: string }
  | { type: "thinking"; text: string }
  | { type: "tool_use"; id: string; name: string; input: string }
  | { type: "tool_result"; tool_use_id: string; text: string; is_error: boolean };

/** Token counts of one assistant message (`message.usage`). */
export interface TokenUsage {
  input: number;
  output: number;
  cache_read: number;
  cache_creation: number;
}

export interface TranscriptEntry {
  uuid: string;
  role: "user" | "assistant";
  timestamp: string | null;
  blocks: TranscriptBlock[];
  /** API message id; lines of one message share it and its usage. */
  message_id?: string | null;
  model?: string | null;
  /** `null`/absent when the transcript has no usage for the message (unknown, not zero). */
  usage?: TokenUsage | null;
}

/** Usage of one assistant message of the main transcript. */
export interface UsageSample {
  id: string;
  timestamp: string | null;
  model: string | null;
  usage: TokenUsage;
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

/** A `Skill` or `Agent` call of the main transcript (flow graph node). */
export interface MainCall {
  tool_use_id: string;
  name: string;
  /** The skill name, or the description of the subagent. */
  target: string;
  /** The skill arguments, or the subagent type. */
  detail: string;
  started: string | null;
  /** Timestamp of the final result; `null` while there is none. */
  ended: string | null;
  is_error: boolean;
  /** Context fill of the message that made the call; `null` when unknown. */
  context?: number | null;
}

export interface MainSnapshot {
  calls: MainCall[];
  /** Tool calls that are neither skills nor agents. */
  tools: number;
  /** One sample per assistant message of the main transcript; empty without usage data. */
  usage?: UsageSample[];
}

export type SubagentUpdate =
  | { kind: "list"; agents: SubagentInfo[] }
  | ({ kind: "main" } & MainSnapshot)
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

/** The string, else `null`. */
function strOrNull(v: unknown): string | null {
  return typeof v === "string" ? v : null;
}

/** Four non-negative counts, else `null` (unknown). */
export function parseUsage(raw: unknown): TokenUsage | null {
  if (!isObject(raw)) return null;
  const counts = [raw["input"], raw["output"], raw["cache_read"], raw["cache_creation"]];
  if (!counts.every((n) => typeof n === "number" && Number.isFinite(n) && n >= 0)) return null;
  const [input, output, cache_read, cache_creation] = counts as number[];
  return { input, output, cache_read, cache_creation };
}

function parseEntry(raw: unknown): TranscriptEntry | null {
  if (!isObject(raw) || (raw["role"] !== "user" && raw["role"] !== "assistant")) return null;
  const blocks = Array.isArray(raw["blocks"]) ? raw["blocks"] : [];
  return {
    uuid: str(raw["uuid"]),
    role: raw["role"],
    timestamp: typeof raw["timestamp"] === "string" ? raw["timestamp"] : null,
    blocks: blocks.map(parseBlock).filter((b): b is TranscriptBlock => b !== null),
    message_id: typeof raw["message_id"] === "string" ? raw["message_id"] : null,
    model: typeof raw["model"] === "string" ? raw["model"] : null,
    usage: parseUsage(raw["usage"]),
  };
}

function parseInfo(raw: unknown): SubagentInfo | null {
  if (!isObject(raw) || typeof raw["agent_id"] !== "string" || !raw["agent_id"]) return null;
  return {
    agent_id: raw["agent_id"],
    agent_type: str(raw["agent_type"]),
    description: str(raw["description"]),
    tool_use_id: str(raw["tool_use_id"]),
    spawn_depth: typeof raw["spawn_depth"] === "number" ? raw["spawn_depth"] : 0,
    worktree_branch: typeof raw["worktree_branch"] === "string" ? raw["worktree_branch"] : null,
    status: raw["status"] === "done" ? "done" : "running",
    started: strOrNull(raw["started"]),
    last: strOrNull(raw["last"]),
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
  if (update["kind"] === "main" && Array.isArray(update["calls"])) {
    const calls = update["calls"].flatMap((raw): MainCall[] =>
      isObject(raw) && typeof raw["tool_use_id"] === "string" && raw["tool_use_id"]
        ? [
            {
              tool_use_id: raw["tool_use_id"],
              name: str(raw["name"]),
              target: str(raw["target"]),
              detail: str(raw["detail"]),
              started: strOrNull(raw["started"]),
              ended: strOrNull(raw["ended"]),
              is_error: raw["is_error"] === true,
              context: typeof raw["context"] === "number" ? raw["context"] : null,
            },
          ]
        : [],
    );
    const tools = typeof update["tools"] === "number" ? update["tools"] : 0;
    const usage = (Array.isArray(update["usage"]) ? update["usage"] : []).flatMap(
      (raw): UsageSample[] => {
        const counts = isObject(raw) ? parseUsage(raw["usage"]) : null;
        return isObject(raw) && counts
          ? [
              {
                id: str(raw["id"]),
                timestamp: strOrNull(raw["timestamp"]),
                model: strOrNull(raw["model"]),
                usage: counts,
              },
            ]
          : [];
      },
    );
    return { session: raw["session"], update: { kind: "main", calls, tools, usage } };
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

/** Compact token count: `842`, `12.3k`, `123k`, `1.2M`. */
export function formatTokens(n: number): string {
  const compact = (value: number, unit: string) =>
    `${value < 100 ? value.toFixed(1).replace(/\.0$/, "") : Math.round(value)}${unit}`;
  if (n < 1000) return String(Math.round(n));
  if (n < 999_950) return compact(n / 1000, "k");
  return compact(n / 1_000_000, "M");
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

/** Each `Subagents` instance watches under its own key, so the watches of followers are separate. */
let followers = 0;

/**
 * The subagents of the Claude Code or Copilot CLI agent in one pane and their transcripts. The backend watches
 * the transcript files and pushes `subagents` events; nothing here reads or polls files. The root
 * instance follows the pane of the output panel; a flow tab provides its own instance for its pane.
 * Every instance has its own backend watch (by key) and stops it when it is destroyed.
 */
/** Retries of a watch that found no transcript yet: delay grows by the step up to the cap. */
const WATCH_RETRIES = 60;
const WATCH_RETRY_MS = 1000;
const WATCH_RETRY_MAX_MS = 10_000;

@Injectable({ providedIn: "root" })
export class Subagents {
  private readonly bridge = inject(TauriBridge);
  private readonly key = `follower-${++followers}`;
  private listening: Promise<() => void> | null = null;
  private session: string | null = null;
  private generation = 0;

  private readonly _agents = signal<readonly SubagentInfo[]>([]);
  private readonly _entries = signal<ReadonlyMap<string, readonly TranscriptEntry[]>>(new Map());
  private readonly _picked = signal<string | null>(null);
  private readonly _supported = signal(false);
  private readonly _main = signal<MainSnapshot>({ calls: [], tools: 0, usage: [] });
  readonly agents = this._agents.asReadonly();
  /** Skill and agent calls of the main transcript. */
  readonly main = this._main.asReadonly();
  /** Messages of every subagent seen so far, by agent id. */
  readonly entries = this._entries.asReadonly();
  private readonly _paneId = signal<string | null>(null);
  /** The pane followed; `null` when nothing is followed. */
  readonly paneId = this._paneId.asReadonly();
  /** `false` for a pane without a followed session (other agents, plain terminals). */
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

  constructor() {
    inject(DestroyRef).onDestroy(() => {
      ++this.generation;
      void this.listening?.then((stop) => stop());
      void this.bridge.invoke("subagents_unwatch", { key: this.key }).catch(() => undefined);
    });
  }

  /** Shows the subagent `id`; kept across panes, since agent ids are unique (else the default). */
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
    this._main.set({ calls: [], tools: 0, usage: [] });
    this._supported.set(false);
    this._paneId.set(session === null ? null : paneId);
    this.listening ??= this.bridge.listen<unknown>("subagents", (raw) => {
      if (isObject(raw) && raw["key"] === this.key) this.apply(raw);
    });
    await this.listening;
    if (generation !== this.generation) return;
    if (paneId === null || session === null) {
      await this.bridge.invoke("subagents_unwatch", { key: this.key }).catch(() => undefined);
      return;
    }
    this.session = session;
    await this.watch(paneId, session, generation, 0);
  }

  /**
   * Starts the backend watch. A new session (after `/clear`) has no transcript file until its
   * first prompt, so a miss is retried with a growing delay while this is still the followed
   * session; a failure is not.
   */
  private async watch(
    paneId: string,
    session: string,
    generation: number,
    attempt: number,
  ): Promise<void> {
    try {
      const watching = await this.bridge.invoke<boolean>("subagents_watch", {
        key: this.key,
        paneId,
      });
      if (generation !== this.generation) return;
      this._supported.set(watching === true);
      if (watching !== true && attempt < WATCH_RETRIES) {
        setTimeout(
          () => {
            if (generation === this.generation) {
              void this.watch(paneId, session, generation, attempt + 1);
            }
          },
          Math.min(WATCH_RETRY_MS * (attempt + 1), WATCH_RETRY_MAX_MS),
        );
      }
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
    } else if (update.kind === "main") {
      this._main.set({ calls: update.calls, tools: update.tools, usage: update.usage });
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

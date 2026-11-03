import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";
import {
  Subagents,
  defaultSubagent,
  formatDuration,
  parseSubagentEvent,
  reportIndex,
  subagentDuration,
  subagentLabel,
  toolSummary,
  atBottom,
  transcriptItems,
  type SubagentInfo,
  type TranscriptEntry,
} from "./transcripts";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const info = (over: Partial<SubagentInfo> = {}): SubagentInfo => ({
  agent_id: "a1",
  agent_type: "general-purpose",
  description: "Review PBI",
  tool_use_id: "toolu_1",
  spawn_depth: 1,
  worktree_branch: null,
  status: "running",
  started: "2026-11-02T04:00:00.000Z",
  last: "2026-11-02T04:03:05.000Z",
  ...over,
});

const entry = (role: "user" | "assistant", blocks: TranscriptEntry["blocks"]): TranscriptEntry => ({
  uuid: "u",
  role,
  timestamp: "2026-11-02T04:00:01.000Z",
  blocks,
});

beforeEach(() => tauri.reset());

describe("parseSubagentEvent", () => {
  it("reads a list and entries, dropping what it does not know", () => {
    const list = parseSubagentEvent({
      session: "S",
      update: { kind: "list", agents: [info(), { nope: 1 }, "x"] },
    });
    expect(list?.update).toMatchObject({ kind: "list", agents: [{ agent_id: "a1" }] });
    const entries = parseSubagentEvent({
      session: "S",
      update: {
        kind: "entries",
        agent_id: "a1",
        entries: [
          {
            uuid: "u",
            role: "assistant",
            timestamp: null,
            blocks: [{ type: "text", text: "hi" }, { type: "hologram" }],
          },
          { role: "system", blocks: [] },
        ],
      },
    });
    expect(entries?.update).toEqual({
      kind: "entries",
      agent_id: "a1",
      entries: [
        { uuid: "u", role: "assistant", timestamp: null, blocks: [{ type: "text", text: "hi" }] },
      ],
    });
  });

  it("rejects malformed payloads", () => {
    expect(parseSubagentEvent(null)).toBeNull();
    expect(parseSubagentEvent({ session: "S", update: { kind: "future" } })).toBeNull();
    expect(parseSubagentEvent({ update: { kind: "list", agents: [] } })).toBeNull();
  });
});

describe("durations", () => {
  const now = Date.parse("2026-11-02T04:10:00.000Z");
  it("runs until now while running and until the last line when done", () => {
    expect(subagentDuration(info(), now)).toBe(600_000);
    expect(subagentDuration(info({ status: "done" }), now)).toBe(185_000);
  });
  it("is unknown without a start", () => {
    expect(subagentDuration(info({ started: null }), now)).toBeNull();
    expect(subagentDuration(info({ started: "garbage" }), now)).toBeNull();
  });
  it("formats", () => {
    expect(formatDuration(400)).toBe("400 ms");
    expect(formatDuration(12_000)).toBe("12 s");
    expect(formatDuration(185_000)).toBe("3 min 05 s");
    expect(formatDuration(3_720_000)).toBe("1 h 02 min");
  });
  it("labels with description, type, status and duration", () => {
    expect(subagentLabel(info({ status: "done" }), now)).toBe(
      "Review PBI · general-purpose · done · 3 min 05 s",
    );
    expect(subagentLabel(info({ description: "", started: null }), now)).toBe(
      "a1 · general-purpose · running",
    );
  });
});

describe("defaultSubagent", () => {
  it("prefers the newest running one, else the newest", () => {
    const done = info({ agent_id: "d", status: "done" });
    const run = info({ agent_id: "r" });
    expect(defaultSubagent([run, done])).toBe("r");
    expect(defaultSubagent([done, info({ agent_id: "e", status: "done" })])).toBe("e");
    expect(defaultSubagent([])).toBeNull();
  });
});

describe("transcriptItems", () => {
  it("makes the first user text the prompt and joins calls with their results", () => {
    const items = transcriptItems([
      entry("user", [{ type: "text", text: "Review it" }]),
      entry("assistant", [
        { type: "thinking", text: "hm" },
        { type: "tool_use", id: "t1", name: "Bash", input: "{}" },
        { type: "tool_use", id: "t2", name: "Read", input: "{}" },
      ]),
      entry("user", [{ type: "tool_result", tool_use_id: "t1", text: "ok", is_error: false }]),
      entry("assistant", [{ type: "text", text: "Done." }]),
    ]);
    expect(items.map((i) => i.kind)).toEqual(["prompt", "thinking", "tool", "tool", "message"]);
    expect(items[2]).toMatchObject({ id: "t1", result: { text: "ok", isError: false } });
    expect(items[3]).toMatchObject({ id: "t2", result: null });
    expect(reportIndex(items)).toBe(4);
  });

  it("drops a result without its call and blank text", () => {
    const items = transcriptItems([
      entry("user", [{ type: "tool_result", tool_use_id: "x", text: "?", is_error: true }]),
      entry("assistant", [{ type: "text", text: "  " }]),
    ]);
    expect(items).toEqual([]);
    expect(reportIndex(items)).toBe(-1);
  });
});

describe("Subagents", () => {
  const create = () => Injector.create({ providers: [TauriBridge, Subagents] }).get(Subagents);
  const send = (session: string, update: unknown) => tauri.emit("subagents", { session, update });

  it("watches the pane and collects list and entries of its session", async () => {
    tauri.answer("subagents_watch", () => true);
    const s = create();
    await s.follow("w1:p1", "S");
    expect(tauri.calls("subagents_watch")).toEqual([{ paneId: "w1:p1" }]);
    expect(s.supported()).toBe(true);
    send("S", { kind: "list", agents: [info()] });
    send("S", {
      kind: "entries",
      agent_id: "a1",
      entries: [entry("user", [{ type: "text", text: "go" }])],
    });
    send("S", {
      kind: "entries",
      agent_id: "a1",
      entries: [entry("assistant", [{ type: "text", text: "ok" }])],
    });
    expect(s.selectedId()).toBe("a1");
    expect(s.items().map((i) => i.kind)).toEqual(["prompt", "message"]);
  });

  it("ignores events of another session and keeps a picked subagent", async () => {
    tauri.answer("subagents_watch", () => true);
    const s = create();
    await s.follow("w1:p1", "S");
    send("OTHER", { kind: "list", agents: [info({ agent_id: "z" })] });
    expect(s.agents()).toEqual([]);
    send("S", { kind: "list", agents: [info({ agent_id: "a" }), info({ agent_id: "b" })] });
    expect(s.selectedId()).toBe("b");
    s.pick("a");
    send("S", { kind: "list", agents: [info({ agent_id: "a" }), info({ agent_id: "b" })] });
    expect(s.selectedId()).toBe("a");
    send("S", { kind: "list", agents: [info({ agent_id: "b" })] });
    expect(s.selectedId()).toBe("b");
  });

  it("is unsupported when the backend finds no transcript", async () => {
    tauri.answer("subagents_watch", () => false);
    const s = create();
    await s.follow("w1:p1", "S");
    expect(s.supported()).toBe(false);
  });

  it("is unsupported when the backend fails", async () => {
    tauri.answer("subagents_watch", () => {
      throw new Error("boom");
    });
    const s = create();
    await s.follow("w1:p1", "S");
    expect(s.supported()).toBe(false);
  });

  it("stops watching without a pane and clears the state", async () => {
    tauri.answer("subagents_watch", () => true);
    const s = create();
    await s.follow("w1:p1", "S");
    send("S", { kind: "list", agents: [info()] });
    await s.follow(null, null);
    expect(tauri.calls("subagents_unwatch")).toHaveLength(1);
    expect(s.agents()).toEqual([]);
    expect(s.supported()).toBe(false);
  });
});

describe("toolSummary", () => {
  it("shows the most telling argument", () => {
    expect(toolSummary('{"command":"git   diff\\n--stat","description":"d"}')).toBe(
      "git diff --stat",
    );
    expect(toolSummary('{"file_path":"/a/b.ts","content":"x"}')).toBe("/a/b.ts");
  });
  it("falls back to the raw text for truncated or unknown input and shortens it", () => {
    expect(toolSummary('{"command":"unterminated')).toBe('{"command":"unterminated');
    expect(toolSummary('{"other":1}')).toBe('{"other":1}');
    expect(toolSummary("x".repeat(200), 10)).toBe("xxxxxxxxxx…");
  });
});

describe("atBottom", () => {
  it("is true within a few pixels of the end", () => {
    expect(atBottom({ scrollTop: 392, clientHeight: 100, scrollHeight: 500 })).toBe(true);
    expect(atBottom({ scrollTop: 100, clientHeight: 100, scrollHeight: 500 })).toBe(false);
    expect(atBottom({ scrollTop: 0, clientHeight: 100, scrollHeight: 100 })).toBe(true);
  });
});

// @vitest-environment jsdom
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentStatus, Terminal } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { tauri } from "../core/tauri.mock";
import { UI_STATE_KEY, UI_STORAGE, UiState, parseLayout, type UiStorage } from "../core/ui-state";
import { MAX_PROMPT_LENGTH } from "./comment-prompt";
import { LineComments, agentsOf, type NewComment } from "./line-comments";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

const DIR = "/work/repo";
const panes = signal(new Map<string, Terminal>());

function pane(id: string, cwd: string, agent?: string, status: AgentStatus = "idle"): Terminal {
  return { terminal_id: id, tab_id: "t", space_id: "s", cwd, agent, agent_status: status };
}

function setPanes(...list: Terminal[]) {
  panes.set(new Map(list.map((p) => [p.terminal_id, p])));
}

function memory(): UiStorage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => void data.set(k, v),
  };
}

const NEW: NewComment = {
  path: "src/a.ts",
  side: "new",
  start: 3,
  end: 4,
  quote: "a\nb",
  text: "  Please rename.  ",
};

let storage = memory();

function service(): LineComments {
  TestBed.configureTestingModule({
    providers: [
      { provide: UI_STORAGE, useValue: storage },
      { provide: HerdrStore, useValue: { panes } },
      LineComments,
    ],
  });
  const comments = TestBed.inject(LineComments);
  comments.follow(DIR);
  return comments;
}

beforeEach(() => {
  tauri.reset();
  storage = memory();
  setPanes();
});
afterEach(() => TestBed.resetTestingModule());

describe("pending comments", () => {
  it("adds trimmed comments, edits and removes them, ignoring empty text", () => {
    const c = service();
    c.add(NEW);
    c.add({ ...NEW, text: "   " });
    expect(c.pending().map((x) => x.text)).toEqual(["Please rename."]);
    const id = c.pending()[0].id;
    c.edit(id, "Better name.");
    c.edit(id, " ");
    expect(c.pending()[0].text).toBe("Better name.");
    expect(c.forFile("src/a.ts")).toHaveLength(1);
    expect(c.forFile("other.ts")).toHaveLength(0);
    c.remove(id);
    expect(c.pending()).toEqual([]);
  });

  it("survives a new session and stays per worktree", () => {
    const first = service();
    first.add(NEW);
    TestBed.resetTestingModule();

    const second = service();
    expect(second.pending()).toHaveLength(1);
    expect(second.pending()[0]).toMatchObject({
      path: "src/a.ts",
      start: 3,
      end: 4,
      quote: "a\nb",
    });
    second.follow("/work/other");
    expect(second.pending()).toEqual([]);
  });

  it("drops malformed stored comments", () => {
    const stored = {
      lineComments: {
        [DIR]: {
          pending: [
            { id: "ok", path: "a", side: "old", start: 1, end: 2, quote: "q", text: "t" },
            { id: "bad", path: "a", side: "left", start: 1, end: 2, quote: "q", text: "t" },
            { id: "bad2", path: "a", side: "new", start: 5, end: 2, quote: "q", text: "t" },
            "nope",
          ],
          sent: "x",
        },
        "/empty": { pending: [], sent: [] },
      },
    };
    const state = parseLayout(JSON.stringify(stored));
    expect(Object.keys(state.lineComments ?? {})).toEqual([DIR]);
    expect(state.lineComments?.[DIR].pending.map((c) => c.id)).toEqual(["ok"]);
    expect(state.lineComments?.[DIR].sent).toEqual([]);
    expect(parseLayout(JSON.stringify({ lineComments: [1] })).lineComments).toBeUndefined();
  });

  it("writes the state key without comments once none are left", () => {
    const c = service();
    c.add(NEW);
    expect(storage.data.get(UI_STATE_KEY)).toContain("Please rename.");
    c.remove(c.pending()[0].id);
    expect(JSON.parse(storage.data.get(UI_STATE_KEY)!).lineComments).toBeUndefined();
    expect(TestBed.inject(UiState).lineComments()).toEqual({});
  });
});

describe("target agent", () => {
  it("finds the agent panes of the worktree only", () => {
    const list = [
      pane("a", DIR, "claude"),
      pane("b", `${DIR}/sub`, "copilot"),
      pane("c", DIR),
      pane("d", "/work/repo-other", "claude"),
    ];
    expect(agentsOf(list, DIR).map((p) => p.terminal_id)).toEqual(["a", "b"]);
    expect(agentsOf(list, `${DIR}/`).map((p) => p.terminal_id)).toEqual(["a", "b"]);
  });

  it("explains why sending is blocked", () => {
    const c = service();
    expect(c.blocked()).toBe("No pending comments");
    c.add(NEW);
    expect(c.blocked()).toContain("No agent");
    setPanes(pane("a", DIR, "claude"), pane("b", DIR, "copilot"));
    expect(c.target()).toBeNull();
    expect(c.blocked()).toContain("Pick");
    c.chosen.set("b");
    expect(c.target()).toBe("b");
    expect(c.blocked()).toBeNull();
  });

  it("uses the only agent without a pick", () => {
    const c = service();
    c.add(NEW);
    setPanes(pane("a", DIR, "claude"));
    expect(c.target()).toBe("a");
    expect(c.blocked()).toBeNull();
  });
});

describe("send to agent", () => {
  it("sends one prompt and moves the comments to sent", async () => {
    const c = service();
    c.add(NEW);
    setPanes(pane("a", DIR, "claude"));
    tauri.answer("agent_prompt", () => ({ kind: "started", status: "working" }));

    expect(await c.send()).toBe(true);

    const calls = tauri.calls("agent_prompt") as { target: string; text: string }[];
    expect(calls).toHaveLength(1);
    expect(calls[0].target).toBe("a");
    expect(calls[0].text).toContain("src/a.ts:3-4");
    expect(calls[0].text).toContain("Please rename.");
    expect(c.pending()).toEqual([]);
    expect(c.sent().map((x) => x.text)).toEqual(["Please rename."]);
    expect(c.error()).toBeNull();
    expect(tauri.calls("changes_write_comments")).toEqual([]);
  });

  it("keeps the comments pending and shows the error when the prompt fails", async () => {
    const c = service();
    c.add(NEW);
    setPanes(pane("a", DIR, "claude"));
    tauri.answer("agent_prompt", () => ({ kind: "failed", message: "pane not found" }));
    expect(await c.send()).toBe(false);
    expect(c.pending()).toHaveLength(1);
    expect(c.sent()).toEqual([]);
    expect(c.error()).toBe("pane not found");

    tauri.answer("agent_prompt", () => {
      throw new Error("socket closed");
    });
    expect(await c.send()).toBe(false);
    expect(c.pending()).toHaveLength(1);
    expect(c.error()).toContain("socket closed");
  });

  it("keeps the comments pending when the agent is blocked, then sends after a retry", async () => {
    const c = service();
    c.add(NEW);
    setPanes(pane("a", DIR, "claude"));
    tauri.answer("agent_prompt", () => ({ kind: "blocked" }));
    expect(await c.send()).toBe(false);
    expect(c.pending()).toHaveLength(1);
    expect(c.error()).toContain("nothing was sent");

    tauri.answer("agent_prompt", () => ({ kind: "started", status: "working" }));
    expect(await c.send()).toBe(true);
    expect(c.error()).toBeNull();
    expect(c.pending()).toEqual([]);
  });

  it("does nothing while blocked", async () => {
    const c = service();
    c.add(NEW);
    expect(await c.send()).toBe(false);
    expect(tauri.calls("agent_prompt")).toEqual([]);
  });

  it("writes long comments to a file and names it in the prompt", async () => {
    const c = service();
    c.add({ ...NEW, text: "z".repeat(MAX_PROMPT_LENGTH) });
    setPanes(pane("a", DIR, "claude"));
    tauri.answer("changes_write_comments", () => "/work/repo/.agents/state/comments-1.md");
    tauri.answer("agent_prompt", () => ({ kind: "started", status: "working" }));

    expect(await c.send()).toBe(true);

    const [write] = tauri.calls("changes_write_comments") as { dir: string; content: string }[];
    expect(write.dir).toBe(DIR);
    expect(write.content).toContain("src/a.ts:3-4");
    const [prompt] = tauri.calls("agent_prompt") as { text: string }[];
    expect(prompt.text).toContain("/work/repo/.agents/state/comments-1.md");
    expect(prompt.text.length).toBeLessThan(MAX_PROMPT_LENGTH);
    expect(c.pending()).toEqual([]);
  });

  it("keeps the comments and sends nothing when the file cannot be written", async () => {
    const c = service();
    c.add({ ...NEW, text: "z".repeat(MAX_PROMPT_LENGTH) });
    setPanes(pane("a", DIR, "claude"));
    tauri.answer("changes_write_comments", () => {
      throw new Error("disk full");
    });
    expect(await c.send()).toBe(false);
    expect(tauri.calls("agent_prompt")).toEqual([]);
    expect(c.pending()).toHaveLength(1);
    expect(c.error()).toContain("disk full");
  });

  it("keeps a comment added while the prompt was on its way", async () => {
    const c = service();
    c.add(NEW);
    setPanes(pane("a", DIR, "claude"));
    tauri.answer("agent_prompt", () => {
      c.add({ ...NEW, path: "late.ts", text: "late" });
      return { kind: "started", status: "working" };
    });
    expect(await c.send()).toBe(true);
    expect(c.pending().map((x) => x.path)).toEqual(["late.ts"]);
    expect(c.sent().map((x) => x.path)).toEqual(["src/a.ts"]);
  });
});

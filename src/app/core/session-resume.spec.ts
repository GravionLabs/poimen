// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  RESUME_REMOTE_REASON,
  SessionResume,
  parseSessions,
  resumeCommand,
  resumeUnavailable,
} from "./session-resume";
import type { HarnessInfo } from "./settings";
import { SpaceActions } from "./space-actions";
import { SpacesStore } from "./spaces-store";
import { TauriBridge } from "./tauri";

const harness = (over: Partial<HarnessInfo> & { id: string }): HarnessInfo => ({
  command: [over.id],
  models: [],
  defaultModel: null,
  exitInput: null,
  resumeArgs: ["--resume", "{session}"],
  printArgs: [],
  available: true,
  ...over,
});

const session = (agent: string, id: string) => ({
  agent,
  session: id,
  started: "2026-11-01T10:00:00Z",
  lastActivity: "2026-11-01T11:00:00Z",
  firstPrompt: "fix it",
});

describe("resumeCommand", () => {
  it("expands {session} per element for a hook kind", () => {
    expect(resumeCommand(harness({ id: "claude" }), "a b")).toEqual({
      kind: "claude",
      args: ["--resume", "a b"],
    });
  });

  it("runs the own program and base arguments of another harness", () => {
    const h = harness({
      id: "mine",
      command: ["my-agent", "--fast"],
      resumeArgs: ["-r={session}"],
    });
    expect(resumeCommand(h, "s1")).toEqual({ kind: "my-agent", args: ["--fast", "-r=s1"] });
  });

  it("is null without resumeArgs, without the program or without a harness", () => {
    expect(resumeCommand(harness({ id: "claude", resumeArgs: [] }), "s")).toBeNull();
    expect(resumeCommand(harness({ id: "claude", available: false }), "s")).toBeNull();
    expect(resumeCommand(undefined, "s")).toBeNull();
  });
});

describe("parseSessions and resumeUnavailable", () => {
  it("skips malformed entries", () => {
    expect(parseSessions([session("claude", "a"), { agent: "claude" }, 3, null])).toHaveLength(1);
    expect(parseSessions("x")).toEqual([]);
  });

  it("names the reason for a machine id only", () => {
    expect(resumeUnavailable("s1")).toBeNull();
    expect(resumeUnavailable("box::s1")).toBe(RESUME_REMOTE_REASON);
  });
});

describe("SessionResume (#986)", () => {
  const calls: [string, unknown][] = [];
  const toast = { notify: vi.fn(), fail: vi.fn() };
  let sessions: unknown[];
  let harnesses: unknown[];

  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  beforeEach(() => {
    calls.length = 0;
    toast.notify.mockClear();
    toast.fail.mockClear();
    sessions = [session("claude", "s1"), session("copilot", "s2")];
    harnesses = [harness({ id: "claude" }), harness({ id: "copilot", resumeArgs: [] })];
    const pane = { terminal_id: "t1", space_id: "sp", cwd: "/w/p" };
    const store = {
      pane: (id: string) => (id === "t1" || id === "t2" ? { ...pane, terminal_id: id } : undefined),
      snapshot: () => ({ spaces: [], tabs: [], terminals: [pane] }),
      refresh: vi.fn(async () => undefined),
    };
    const bridge = {
      invoke: vi.fn(async (cmd: string, args?: unknown): Promise<unknown> => {
        calls.push([cmd, args]);
        if (cmd === "get_harnesses") return harnesses;
        if (cmd === "list_agent_sessions") return sessions;
        if (cmd === "terminal_create") return "t2";
        return undefined;
      }),
    };
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        { provide: TauriBridge, useValue: bridge },
        { provide: SpacesStore, useValue: store },
        { provide: SpaceActions, useValue: toast },
      ],
    });
  });

  it("lists the sessions of the pane's directory, without a harness that cannot resume", async () => {
    const resume = TestBed.inject(SessionResume);
    await resume.open({ kind: "pane", paneId: "t1" });
    expect(calls).toContainEqual(["list_agent_sessions", { dir: "/w/p" }]);
    expect(resume.dialog()?.sessions.map((s) => s.session)).toEqual(["s1"]);
    expect(resume.dialog()?.loading).toBe(false);
  });

  it("starts the harness in the pane at its prompt", async () => {
    const resume = TestBed.inject(SessionResume);
    await resume.open({ kind: "pane", paneId: "t1" });
    const created = await resume.pick(resume.dialog()!.sessions[0]!);
    expect(created).toBeUndefined();
    expect(calls.at(-1)).toEqual([
      "agent_start",
      { terminalId: "t1", kind: "claude", name: null, args: ["--resume", "s1"] },
    ]);
    expect(resume.dialog()).toBeNull();
  });

  it("starts it in a new tab of the space", async () => {
    const resume = TestBed.inject(SessionResume);
    await resume.open({ kind: "space", spaceId: "sp" });
    const created = await resume.pick(resume.dialog()!.sessions[0]!);
    expect(calls).toContainEqual(["terminal_create", { spaceId: "sp", cwd: "/w/p" }]);
    expect(calls.at(-1)?.[1]).toMatchObject({ terminalId: "t2", args: ["--resume", "s1"] });
    expect(created?.terminal_id).toBe("t2");
  });

  it("does not open for a remote machine and says why", async () => {
    const resume = TestBed.inject(SessionResume);
    await resume.open({ kind: "pane", paneId: "box::t1" });
    expect(resume.dialog()).toBeNull();
    expect(toast.notify).toHaveBeenCalledWith(RESUME_REMOTE_REASON);
    expect(calls).toEqual([]);
  });

  it("tells a failed start in a toast", async () => {
    const resume = TestBed.inject(SessionResume);
    await resume.open({ kind: "pane", paneId: "t1" });
    const bridge = TestBed.inject(TauriBridge) as unknown as { invoke: ReturnType<typeof vi.fn> };
    bridge.invoke.mockRejectedValueOnce("pane is busy");
    await resume.pick(resume.dialog()!.sessions[0]!);
    expect(toast.fail).toHaveBeenCalledWith("Cannot resume the session: pane is busy");
  });
});

// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { EditorTabs } from "../core/editor-tabs";
import type { AgentStatus } from "../core/herdr";
import { UI_STORAGE } from "../core/ui-state";
import { AgentRuns, advance, type AgentRun } from "./agent-runs";
import { GithubIssues } from "./issues";
import { GithubPulls } from "./pulls";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

const run = (over: Partial<AgentRun> = {}): AgentRun => ({
  paneId: "p1",
  repo: "o/n",
  numbers: [7],
  actionId: "refine",
  label: "Refine",
  seen: false,
  state: "starting",
  ...over,
});
const st = (entries: [string, AgentStatus][]) => new Map(entries);

describe("advance", () => {
  it("working is running, blocked is waiting, back to working is running again", () => {
    let r = advance([run()], st([["p1", "working"]]));
    expect(r.runs[0]).toMatchObject({ state: "running", seen: true });
    r = advance(r.runs, st([["p1", "blocked"]]));
    expect(r.runs[0].state).toBe("waiting");
    r = advance(r.runs, st([["p1", "working"]]));
    expect(r.runs[0].state).toBe("running");
    expect(r.finished).toEqual([]);
  });

  it("idle keeps the state, done ends the run", () => {
    const r = advance([run({ state: "running", seen: true })], st([["p1", "idle"]]));
    expect(r.runs[0].state).toBe("running");
    const d = advance(r.runs, st([["p1", "done"]]));
    expect(d.runs).toEqual([]);
    expect(d.finished.map((f) => f.paneId)).toEqual(["p1"]);
  });

  it("a pane not listed yet is kept, a listed pane that vanished ends the run", () => {
    expect(advance([run()], st([])).runs).toHaveLength(1);
    const gone = advance([run({ seen: true, state: "running" })], st([]));
    expect(gone.runs).toEqual([]);
    expect(gone.finished).toHaveLength(1);
  });
});

describe("AgentRuns", () => {
  let runs: AgentRuns;
  let pulls: { load: ReturnType<typeof vi.fn> };
  let issues: { load: ReturnType<typeof vi.fn> };
  let editor: { open: ReturnType<typeof vi.fn> };

  beforeEach(() => {
    TestBed.resetTestingModule();
    pulls = { load: vi.fn() };
    issues = { load: vi.fn() };
    editor = { open: vi.fn() };
    TestBed.configureTestingModule({
      providers: [
        { provide: UI_STORAGE, useValue: window.localStorage },
        { provide: GithubPulls, useValue: pulls },
        { provide: GithubIssues, useValue: issues },
        { provide: EditorTabs, useValue: editor },
      ],
    });
    runs = TestBed.inject(AgentRuns);
  });

  const add = (over: Partial<Parameters<AgentRuns["add"]>[0]> = {}) =>
    runs.add({
      paneId: "p1",
      repo: "o/n",
      numbers: [7, 8],
      actionId: "refine",
      label: "Refine",
      ...over,
    });

  it("shows no badge before the agent reports, then running and waiting for every number", () => {
    add();
    expect(runs.badge("o/n", 7)).toBeNull();
    runs.observe(st([["p1", "working"]]));
    expect(runs.badge("o/n", 7)).toEqual({
      paneId: "p1",
      state: "running",
      text: "Refine running",
    });
    expect(runs.badge("o/n", 8)?.state).toBe("running");
    expect(runs.badge("o/n", 9)).toBeNull();
    expect(runs.badge("other/n", 7)).toBeNull();
    runs.observe(st([["p1", "blocked"]]));
    expect(runs.badge("o/n", 7)?.text).toBe("Refine waiting");
    expect(pulls.load).not.toHaveBeenCalled();
  });

  it("done removes the badge and reloads pulls and issues once", () => {
    add();
    runs.observe(st([["p1", "working"]]));
    runs.observe(st([["p1", "done"]]));
    runs.observe(st([["p1", "done"]]));
    expect(runs.badge("o/n", 7)).toBeNull();
    expect(pulls.load).toHaveBeenCalledTimes(1);
    expect(issues.load).toHaveBeenCalledTimes(1);
  });

  it("a closed pane removes the entry and reloads once for several ended runs", () => {
    add();
    add({ paneId: "p2", numbers: [9] });
    runs.observe(
      st([
        ["p1", "working"],
        ["p2", "working"],
      ]),
    );
    runs.observe(st([]));
    expect(runs.runs()).toEqual([]);
    expect(pulls.load).toHaveBeenCalledTimes(1);
  });

  it("a click on the badge shows the pane", () => {
    runs.focus("p1");
    expect(editor.open).toHaveBeenCalledWith("p1");
  });

  it("the mapping lives in the root service, so a rebuilt GitHub tab still sees it", () => {
    add();
    runs.observe(st([["p1", "working"]]));
    expect(TestBed.inject(AgentRuns).badge("o/n", 8)?.state).toBe("running");
  });
});

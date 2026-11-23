import { Injector } from "@angular/core";
import { lastToast, testMessages } from "../../testing/toasts";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentPicker } from "./agent-picker";
import { AgentPrompts } from "./agent-prompt";
import type { Snapshot } from "./snapshot";
import { SpacesStore } from "./spaces-store";
import { ProjectEntries, type EntryOutcome } from "./project-entries";
import { SpaceActions } from "./space-actions";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";
import { UI_STORAGE, UiState } from "./ui-state";

let injector: Injector;

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const pane = (id: string, space: string, agent?: string) => ({
  terminal_id: id,
  tab_id: `${space}:t1`,
  space_id: space,
  agent,
  agent_status: "idle" as const,
  cwd: "/work/app",
});

function snapshot(...terminals: Snapshot["terminals"]): Snapshot {
  return {
    spaces: [{ space_id: "w1", label: "app", number: 1, agent_status: "idle" }],
    tabs: [{ tab_id: "w1:t1", space_id: "w1", label: "main", number: 1, agent_status: "idle" }],
    terminals,
  };
}

describe("ProjectEntries.run", () => {
  let entries: ProjectEntries;
  let spaces: SpaceActions;
  let picker: AgentPicker;

  async function setup(snap: Snapshot, outcome: EntryOutcome) {
    tauri.reset();
    tauri.answer("snapshot", () => snap);
    tauri.answer("project_run", () => outcome);
    tauri.answer("agent_prompt", () => ({ kind: "started", status: "working" }));
    injector = Injector.create({
      providers: [
        testMessages(),
        { provide: UI_STORAGE, useValue: null },
        TauriBridge,
        SpacesStore,
        UiState,
        SpaceActions,
        AgentPrompts,
        AgentPicker,
        ProjectEntries,
      ],
    });
    await injector.get(SpacesStore).refresh();
    entries = injector.get(ProjectEntries);
    spaces = injector.get(SpaceActions);
    picker = injector.get(AgentPicker);
  }

  const command = { kind: "command", name: "test" } as const;

  beforeEach(() => tauri.reset());

  it("asks the backend to run a terminal command in the space directory", async () => {
    await setup(snapshot(pane("w1:p1", "w1")), { kind: "done" });
    await entries.run(command, "w1", "/work/app");
    expect(tauri.calls("project_run")).toEqual([
      { spaceId: "w1", dir: "/work/app", entry: command, decision: null },
    ]);
    expect(tauri.calls("agent_prompt")).toEqual([]);
  });

  it("sends an agent command as the prompt of the only agent", async () => {
    await setup(snapshot(pane("w1:p1", "w1", "claude"), pane("w1:p2", "w1")), {
      kind: "agent",
      command: "/review",
    });
    await entries.run({ kind: "command", name: "review" }, "w1", "/work/app");
    expect(tauri.calls("agent_prompt")).toEqual([{ target: "w1:p1", text: "/review" }]);
  });

  it("lets the user pick among several agents, and sends nothing when cancelled", async () => {
    await setup(snapshot(pane("w1:p1", "w1", "claude"), pane("w1:p2", "w1", "copilot")), {
      kind: "agent",
      command: "/review",
    });
    const run = entries.run(command, "w1", "/work/app");
    await vi.waitFor(() => expect(picker.choices()).toHaveLength(2));
    picker.answer("w1:p2");
    await run;
    expect(tauri.calls("agent_prompt")).toEqual([{ target: "w1:p2", text: "/review" }]);

    tauri.reset();
    tauri.answer("project_run", () => ({ kind: "agent", command: "/x" }));
    const cancelled = entries.run(command, "w1", "/work/app");
    await vi.waitFor(() => expect(picker.choices()).not.toBeNull());
    picker.answer(null);
    await cancelled;
    expect(tauri.calls("agent_prompt")).toEqual([]);
  });

  it("says so when the space has no agent", async () => {
    await setup(snapshot(pane("w1:p1", "w1")), { kind: "agent", command: "/review" });
    await entries.run(command, "w1", "/work/app");
    expect(lastToast(injector)).toContain("no agent");
    expect(tauri.calls("agent_prompt")).toEqual([]);
  });

  it("opens the trust dialog for an untrusted file and runs again with the trusted hash", async () => {
    const request = { root: "/work/app", files: ["f"], hash: "abc", commands: [] };
    await setup(snapshot(pane("w1:p1", "w1")), { kind: "ask", request });
    await entries.run({ kind: "preset", name: "duo" }, "w1", "/work/app");
    expect(spaces.trust()?.request).toEqual(request);

    tauri.answer("project_run", () => ({ kind: "done" }));
    await spaces.answerTrust("trust");
    expect(spaces.trust()).toBeNull();
    expect(tauri.calls("project_run")[1]).toMatchObject({
      entry: { kind: "preset", name: "duo" },
      decision: { kind: "trust", hash: "abc" },
    });
  });

  it("runs nothing more when the trust dialog is cancelled or answered without", async () => {
    const request = { root: "/work/app", files: ["f"], hash: "abc", commands: [] };
    await setup(snapshot(pane("w1:p1", "w1")), { kind: "ask", request });
    await entries.run(command, "w1", "/work/app");
    await spaces.answerTrust("cancel");
    await entries.run(command, "w1", "/work/app");
    await spaces.answerTrust("without");
    expect(tauri.calls("project_run")).toHaveLength(2);
  });

  it("reports a failed run", async () => {
    await setup(snapshot(pane("w1:p1", "w1")), { kind: "done" });
    tauri.answer("project_run", () => {
      throw new Error("herdr said no");
    });
    await entries.run(command, "w1", "/work/app");
    expect(lastToast(injector)).toContain("Project: test failed");
  });
});

describe("ProjectEntries detected scripts", () => {
  let entries: ProjectEntries;
  const script = { source: "npm", name: "build", description: "tsc" };

  beforeEach(async () => {
    tauri.reset();
    tauri.answer("snapshot", () => snapshot());
    tauri.answer("project_entries", () => ({ commands: [], presets: [], locked: false }));
    tauri.answer("detected_entries", () => [script]);
    injector = Injector.create({
      providers: [
        testMessages(),
        { provide: UI_STORAGE, useValue: null },
        TauriBridge,
        SpacesStore,
        UiState,
        SpaceActions,
        AgentPrompts,
        AgentPicker,
        ProjectEntries,
      ],
    });
    entries = injector.get(ProjectEntries);
  });

  it("reads them with the project file when the directory changes", async () => {
    entries.setDir("/work/app");
    await entries.reload();
    expect(entries.detected()).toEqual([script]);
    expect(tauri.calls("detected_entries")).toContainEqual({ dir: "/work/app" });
  });

  it("keeps the project entries when detection fails", async () => {
    tauri.answer("detected_entries", () => {
      throw new Error("boom");
    });
    tauri.answer("project_entries", () => ({
      commands: [{ name: "t", target: "terminal" }],
      presets: [],
      locked: false,
    }));
    entries.setDir("/work/app");
    await entries.reload();
    expect(entries.detected()).toEqual([]);
    expect(entries.entries().commands).toHaveLength(1);
  });

  it("runs by name and source only, never sending a command line", async () => {
    tauri.answer("detected_run", () => true);
    await entries.runDetected(script, "w1", "/work/app");
    expect(tauri.calls("detected_run")).toEqual([
      { spaceId: "w1", dir: "/work/app", source: "npm", name: "build" },
    ]);
  });

  it("tells the user when the script is gone", async () => {
    tauri.answer("detected_run", () => false);
    await entries.runDetected(script, "w1", "/work/app");
    expect(lastToast(injector)).toContain("no longer exists");
  });

  it("reports a failed run", async () => {
    tauri.answer("detected_run", () => {
      throw new Error("no backend");
    });
    await entries.runDetected(script, "w1", "/work/app");
    expect(lastToast(injector)).toContain("Script: build failed");
  });
});

describe("ProjectEntries skills", () => {
  let entries: ProjectEntries;
  let picker: AgentPicker;
  const skill = {
    source: "skills",
    name: "deliver",
    description: "Ship an issue",
  };
  const harness = (id: string, slashSkills: boolean) => ({
    id,
    command: [id],
    slashSkills,
    available: true,
  });

  async function setup(snap: Snapshot) {
    tauri.reset();
    tauri.answer("snapshot", () => snap);
    tauri.answer("agent_prompt", () => ({
      kind: "started",
      status: "working",
    }));
    tauri.answer("get_harnesses", () => [
      harness("claude", true),
      harness("copilot", true),
      harness("gemini", false),
    ]);
    injector = Injector.create({
      providers: [
        testMessages(),
        { provide: UI_STORAGE, useValue: null },
        TauriBridge,
        SpacesStore,
        UiState,
        SpaceActions,
        AgentPrompts,
        AgentPicker,
        ProjectEntries,
      ],
    });
    await injector.get(SpacesStore).refresh();
    entries = injector.get(ProjectEntries);
    picker = injector.get(AgentPicker);
  }

  it("sends /<skill> to the only agent that takes slash skills, never running a command", async () => {
    await setup(snapshot(pane("w1:p1", "w1", "gemini"), pane("w1:p2", "w1", "claude")));
    await entries.start(skill, "w1", "/work/app");
    expect(tauri.calls("agent_prompt")).toEqual([{ target: "w1:p2", text: "/deliver" }]);
    expect(tauri.calls("detected_run")).toEqual([]);
  });

  it("lets the user pick among several such agents", async () => {
    await setup(snapshot(pane("w1:p1", "w1", "claude"), pane("w1:p2", "w1", "copilot")));
    const run = entries.start(skill, "w1", "/work/app");
    await vi.waitFor(() => expect(picker.choices()).toHaveLength(2));
    picker.answer("w1:p2");
    await run;
    expect(tauri.calls("agent_prompt")).toEqual([{ target: "w1:p2", text: "/deliver" }]);
  });

  it("says so when the space has no agent that runs skills", async () => {
    await setup(snapshot(pane("w1:p1", "w1", "gemini"), pane("w1:p2", "w1")));
    await entries.start(skill, "w1", "/work/app");
    expect(lastToast(injector)).toBe("Skill: deliver: the space has no agent that runs skills");
    expect(tauri.calls("agent_prompt")).toEqual([]);
  });

  it("ignores agents of other spaces and harnesses that cannot be read", async () => {
    await setup(snapshot(pane("w2:p1", "w2", "claude")));
    await entries.start(skill, "w1", "/work/app");
    expect(tauri.calls("agent_prompt")).toEqual([]);
    tauri.answer("get_harnesses", () => {
      throw new Error("x");
    });
    await entries.start(skill, "w2", "/work/app");
    expect(tauri.calls("agent_prompt")).toEqual([]);
  });
});

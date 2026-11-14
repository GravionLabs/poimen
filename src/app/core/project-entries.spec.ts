import { Injector } from "@angular/core";
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
    const injector = Injector.create({
      providers: [
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
      { workspaceId: "w1", dir: "/work/app", entry: command, decision: null },
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
    expect(spaces.warning()).toContain("no agent");
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
    expect(spaces.warning()).toContain("Project: test failed");
  });
});

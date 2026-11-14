import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentPrompts } from "./agent-prompt";
import type { Snapshot } from "./snapshot";
import { SpacesStore } from "./spaces-store";
import { Machines, machineBlock, type MachineStatus } from "./machines";
import { PaneActions } from "./pane-actions";
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

const cfg = { reposDir: "~/Source/{owner}/{repo}" };
const box: MachineStatus = {
  name: "box",
  ssh: "box",
  ...cfg,
  state: "connected",
  serverVersion: "1",
};
const lab: MachineStatus = {
  name: "lab",
  ssh: "lab",
  ...cfg,
  state: "offline",
  error: "timed out",
};

describe("machineBlock", () => {
  it("lets local ids and connected machines through", () => {
    expect(machineBlock([box], "w1:p1")).toBeNull();
    expect(machineBlock([box], "box::7")).toBeNull();
  });

  it("names the reason for a machine that is not connected or not configured", () => {
    expect(machineBlock([lab], "lab::7")).toBe("lab is offline: timed out");
    expect(machineBlock([{ ...lab, state: "connecting" }], "lab::7")).toBe("lab is connecting");
    expect(machineBlock([], "gone::7")).toBe("gone is not a configured machine");
  });
});

describe("actions on remote spaces and terminals (#693)", () => {
  let panes: PaneActions;
  let spaces: SpaceActions;
  let prompts: AgentPrompts;
  const snapshot: Snapshot = {
    spaces: [
      { space_id: "box::1", label: "R", number: 1, agent_status: "idle", machine: "box" },
      { space_id: "lab::1", label: "L", number: 2, agent_status: "unknown", machine: "lab" },
    ],
    tabs: [
      { tab_id: "box::t1", space_id: "box::1", label: "m", number: 1, agent_status: "idle" },
      { tab_id: "lab::t1", space_id: "lab::1", label: "m", number: 1, agent_status: "unknown" },
    ],
    terminals: [
      {
        terminal_id: "box::p1",
        tab_id: "box::t1",
        space_id: "box::1",
        agent_status: "idle",
        cwd: "/r",
        machine: "box",
      },
      {
        terminal_id: "lab::p1",
        tab_id: "lab::t1",
        space_id: "lab::1",
        agent_status: "unknown",
        cwd: "/l",
        machine: "lab",
      },
    ],
  };

  beforeEach(async () => {
    tauri.reset();
    tauri.answer("snapshot", () => snapshot);
    tauri.answer("list_machines", () => [box, lab]);
    tauri.answer("terminal_create", () => "box::p2");
    tauri.answer("pane_split", () => "box::p3");
    const injector = Injector.create({
      providers: [
        { provide: UI_STORAGE, useValue: null },
        UiState,
        TauriBridge,
        Machines,
        SpacesStore,
        SpaceActions,
        PaneActions,
        AgentPrompts,
      ],
    });
    await injector.get(Machines).refresh();
    panes = injector.get(PaneActions);
    spaces = injector.get(SpaceActions);
    prompts = injector.get(AgentPrompts);
    await injector.get(SpacesStore).refresh();
    // Only the store's own snapshot read is ignored; the remote ones are given to the actions.
    vi.spyOn(injector.get(SpacesStore), "pane").mockImplementation((id: string) =>
      snapshot.terminals.find((t) => t.terminal_id === id),
    );
    vi.spyOn(injector.get(SpacesStore), "snapshot").mockReturnValue(snapshot);
  });

  it("sends a split of a remote terminal with its qualified id, which the app routes to the machine", async () => {
    await panes.split("box::p1", "right");
    expect(tauri.calls("pane_split")).toEqual([
      { target: "box::p1", direction: "right", cwd: "/r" },
    ]);
  });

  it("sends new terminal, rename and close of a connected machine with the qualified ids", async () => {
    panes.askNew("box::1");
    await panes.confirmNew("/r");
    panes.askRename("box::p1");
    await panes.confirmRename("x");
    panes.askClose("box::p1");
    await panes.confirmClose();
    expect(tauri.calls("terminal_create")).toEqual([{ spaceId: "box::1", cwd: "/r" }]);
    expect(tauri.calls("pane_rename")).toEqual([{ terminalId: "box::p1", label: "x" }]);
    expect(tauri.calls("pane_close")).toEqual([{ terminalId: "box::p1" }]);
  });

  it("sends rename and close of a remote space with its qualified id", async () => {
    expect(await spaces.rename("box::1", "Renamed")).toBe(true);
    expect(tauri.calls("space_rename")).toEqual([{ spaceId: "box::1", label: "Renamed" }]);
    await spaces.requestClose("box::1");
    expect(tauri.calls("space_close").length).toBe(0);
  });

  it("refuses every pane action on an offline machine with the reason, sending nothing", async () => {
    expect(await panes.split("lab::p1", "down")).toBeUndefined();
    expect(panes.error()).toBe("lab is offline: timed out");
    panes.askNew("lab::1");
    await panes.confirmNew("/l");
    panes.askRename("lab::p1");
    expect(await panes.confirmRename("x")).toBe(false);
    panes.askClose("lab::p1");
    expect(await panes.confirmClose()).toBeUndefined();
    await panes.resizeSplit("lab::t1", "s1", 0.4);
    await panes.focusPane("lab::t1", "lab::p1");
    for (const command of [
      "pane_split",
      "terminal_create",
      "pane_rename",
      "agent_rename",
      "pane_close",
      "tab_resize_split",
      "tab_focus",
    ])
      expect(tauri.calls(command), command).toEqual([]);
  });

  it("refuses rename and close of a space on an offline machine", async () => {
    expect(await spaces.rename("lab::1", "New")).toBe(false);
    expect(spaces.renameError()).toBe("lab is offline: timed out");
    expect(await spaces.requestClose("lab::1")).toBeUndefined();
    expect(tauri.calls("space_rename")).toEqual([]);
    expect(tauri.calls("space_close")).toEqual([]);
  });

  it("does not prompt an agent of an offline machine", async () => {
    expect(await prompts.send("lab::p1", "hi")).toEqual({
      kind: "failed",
      message: "lab is offline: timed out",
    });
    expect(tauri.calls("agent_prompt")).toEqual([]);
  });
});

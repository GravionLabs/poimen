import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HerdrStore } from "./herdr-store";
import { SpaceActions, closeSpaceWarning, dirName, neighbourSpace } from "./space-actions";
import type { Snapshot } from "./herdr";
import { TauriBridge } from "./tauri";
import { UI_STORAGE, UiState } from "./ui-state";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));
const pick = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: pick }));

describe("dirName", () => {
  it("is the last path segment", () => {
    expect(dirName("/work/app")).toBe("app");
    expect(dirName("C:\\work\\app\\")).toBe("app");
    expect(dirName("/work/app/ ")).toBe("app");
    expect(dirName("")).toBe("");
  });
});

describe("SpaceActions", () => {
  let actions: SpaceActions;
  let injector: Injector;

  beforeEach(() => {
    tauri.reset();
    pick.mockReset();
    tauri.answer("snapshot", () => ({ spaces: [], tabs: [], terminals: [] }));
    injector = Injector.create({
      providers: [
        TauriBridge,
        HerdrStore,
        { provide: UI_STORAGE, useValue: null },
        UiState,
        SpaceActions,
      ],
    });
    actions = injector.get(SpaceActions);
  });

  it("opens and cancels the dialog", () => {
    actions.ask();
    expect(actions.open()).toBe(true);
    actions.cancel();
    expect(actions.open()).toBe(false);
  });

  it("creates the space with the trimmed directory and label, then closes and re-reads", async () => {
    tauri.answer("create_space", () => ({ workspace_id: "w2", created: true }));
    actions.ask();
    const outcome = await actions.create(" /work/app ", " App ");
    expect(outcome).toEqual({ workspace_id: "w2", created: true });
    expect(tauri.calls("create_space")).toEqual([
      { cwd: "/work/app", label: "App", decision: null, command: null },
    ]);
    expect(tauri.calls("snapshot")).toHaveLength(1);
    expect(actions.open()).toBe(false);
  });

  it("keeps a layout warning of the created space for the status bar", async () => {
    tauri.answer("create_space", () => ({
      workspace_id: "w2",
      created: true,
      warnings: ["/work/app/.agents/herdr-layout.json: invalid JSON"],
    }));
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    await actions.create("/work/app", "");
    expect(actions.warning()).toBe("/work/app/.agents/herdr-layout.json: invalid JSON");
    tauri.answer("create_space", () => ({ workspace_id: "w3", created: true }));
    await actions.create("/work/lib", "");
    expect(actions.warning()).toBeNull();
  });

  it("selects an existing space and names it in a notice when none was created", async () => {
    tauri.answer("snapshot", () => ({
      spaces: [{ space_id: "w1", label: "app", number: 1, agent_status: "idle" }],
      tabs: [],
      terminals: [],
    }));
    tauri.answer("create_space", () => ({ workspace_id: "w1", created: false }));
    const ui = injector.get(UiState);
    await actions.create("/work/app", "");
    expect(ui.splitWorkspace()).toBe("w1");
    expect(actions.warning()).toBe('Opened the existing space "app".');
  });

  it("holds an untrusted project and creates nothing until answered", async () => {
    const request = { root: "/work/app", files: ["f"], hash: "h1", commands: [] };
    tauri.answer("create_space", (args) =>
      args?.["decision"]
        ? { workspace_id: "w2", created: true }
        : { workspace_id: "", created: false, trust: request },
    );
    expect(await actions.create("/work/app", "App")).toBeUndefined();
    expect(actions.trust()?.request).toEqual(request);
    expect(tauri.calls("snapshot")).toHaveLength(0);
    const outcome = await actions.answerTrust("trust");
    expect(outcome?.workspace_id).toBe("w2");
    expect(tauri.calls("create_space")[1]).toEqual({
      cwd: "/work/app",
      label: "App",
      decision: { kind: "trust", hash: "h1" },
      command: null,
    });
    expect(actions.trust()).toBeNull();
  });

  it("cancelling the trust question sends nothing more", async () => {
    const request = { root: "/work/app", files: ["f"], hash: "h1", commands: [] };
    tauri.answer("create_space", () => ({ workspace_id: "", created: false, trust: request }));
    await actions.create("/work/app", "");
    expect(await actions.answerTrust("cancel")).toBeUndefined();
    expect(tauri.calls("create_space")).toHaveLength(1);
    expect(actions.trust()).toBeNull();
  });

  it("sends no label when it is blank", async () => {
    await actions.create("/work/app", "  ");
    expect(tauri.calls("create_space")).toEqual([
      { cwd: "/work/app", label: null, decision: null, command: null },
    ]);
  });

  it("keeps the dialog open and shows the error when herdr or the directory fails", async () => {
    tauri.answer("create_space", () => {
      throw "/nope is not a directory.";
    });
    actions.ask();
    expect(await actions.create("/nope", "")).toBeUndefined();
    expect(actions.error()).toBe("/nope is not a directory.");
    expect(actions.open()).toBe(true);
    expect(actions.busy()).toBe(false);
  });

  it("lists matching directories for the typed path, empty on failure", async () => {
    tauri.answer("list_dirs", () => ["/work/app", "/work/apps"]);
    await actions.complete("/work/ap");
    expect(tauri.calls("list_dirs")).toEqual([{ input: "/work/ap" }]);
    expect(actions.dirs()).toEqual(["/work/app", "/work/apps"]);
    tauri.answer("list_dirs", () => {
      throw "boom";
    });
    await actions.complete("/x");
    expect(actions.dirs()).toEqual([]);
  });

  it("picks a directory with the native dialog; a cancel yields null", async () => {
    pick.mockResolvedValueOnce("/picked");
    expect(await actions.pick()).toBe("/picked");
    expect(pick).toHaveBeenCalledWith({ directory: true, multiple: false });
    pick.mockResolvedValueOnce(null);
    expect(await actions.pick()).toBeNull();
  });
});

const space = (id: string, number: number) => ({
  space_id: id,
  label: `Space ${number}`,
  number,
  agent_status: "idle" as const,
});
const pane = (id: string, workspace: string, agent?: string) => ({
  terminal_id: id,
  tab_id: `${workspace}:t1`,
  space_id: workspace,
  agent_status: "idle" as const,
  agent,
});
const snap = (): Snapshot => ({
  spaces: [space("w1", 1), space("w2", 2), space("w3", 3)],
  tabs: [],
  terminals: [pane("w1:p1", "w1"), pane("w2:p1", "w2", "claude"), pane("w2:p2", "w2")],
});

describe("neighbourSpace", () => {
  it("is the next space, else the previous, else null", () => {
    expect(neighbourSpace(snap(), "w1")).toBe("w2");
    expect(neighbourSpace(snap(), "w3")).toBe("w2");
    expect(neighbourSpace({ spaces: [space("w1", 1)], tabs: [], terminals: [] }, "w1")).toBeNull();
    expect(neighbourSpace(snap(), "nope")).toBeNull();
  });
});

describe("closeSpaceWarning", () => {
  it("counts panes and agents", () => {
    expect(closeSpaceWarning(snap(), "w1")).toBe("Closing the space ends 1 pane.");
    expect(closeSpaceWarning(snap(), "w2")).toBe(
      "Closing the space ends 2 panes, 1 agent among them.",
    );
  });
});

describe("SpaceActions close", () => {
  let actions: SpaceActions;

  beforeEach(async () => {
    tauri.reset();
    tauri.answer("snapshot", snap);
    const injector = Injector.create({
      providers: [
        TauriBridge,
        HerdrStore,
        { provide: UI_STORAGE, useValue: null },
        UiState,
        SpaceActions,
      ],
    });
    await injector.get(HerdrStore).refresh();
    tauri.answer("space_close", () => undefined);
    actions = injector.get(SpaceActions);
  });

  it("asks before closing a space with panes and closes nothing yet", async () => {
    expect(await actions.requestClose("w2")).toBeUndefined();
    expect(actions.closing()).toMatchObject({ workspaceId: "w2", label: "Space 2", panes: 2 });
    expect(tauri.calls("space_close")).toEqual([]);
    actions.cancelClose();
    expect(actions.closing()).toBeNull();
    expect(tauri.calls("space_close")).toEqual([]);
  });

  it("closes the confirmed space, focuses the neighbour and re-reads the snapshot", async () => {
    await actions.requestClose("w2");
    const before = tauri.calls("snapshot").length;
    expect(await actions.confirmClose()).toEqual(["w2:p1", "w2:p2"]);
    expect(tauri.calls("space_close")).toEqual([{ workspaceId: "w2", focus: "w3" }]);
    expect(tauri.calls("snapshot")).toHaveLength(before + 1);
    expect(actions.closing()).toBeNull();
  });

  it("closes an empty space without a prompt", async () => {
    expect(await actions.requestClose("w3")).toEqual([]);
    expect(actions.closing()).toBeNull();
    expect(tauri.calls("space_close")).toEqual([{ workspaceId: "w3", focus: "w2" }]);
  });

  it("closing the last space sends no neighbour", async () => {
    tauri.answer("snapshot", () => ({ spaces: [space("w1", 1)], tabs: [], terminals: [] }));
    const injector = Injector.create({
      providers: [
        TauriBridge,
        HerdrStore,
        { provide: UI_STORAGE, useValue: null },
        UiState,
        SpaceActions,
      ],
    });
    await injector.get(HerdrStore).refresh();
    const last = injector.get(SpaceActions);
    await last.requestClose("w1");
    expect(tauri.calls("space_close").at(-1)).toEqual({ workspaceId: "w1", focus: null });
  });

  it("shows herdr's error and reports no closed panes when the close fails", async () => {
    tauri.answer("space_close", () => {
      throw "workspace not found";
    });
    await actions.requestClose("w2");
    expect(await actions.confirmClose()).toBeUndefined();
    expect(actions.closeError()).toBe("workspace not found");
    actions.dismissCloseError();
    expect(actions.closeError()).toBeNull();
  });

  it("ignores an unknown space and a confirmation nobody asked for", async () => {
    expect(await actions.requestClose("nope")).toBeUndefined();
    expect(await actions.confirmClose()).toBeUndefined();
    expect(tauri.calls("space_close")).toEqual([]);
  });
});

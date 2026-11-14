// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { EditorTabs, flowTab } from "./editor-tabs";
import type { Snapshot, TabTree, Terminal } from "./snapshot";
import { SpacesStore } from "./spaces-store";
import { PaneActions } from "./pane-actions";
import { UI_STORAGE } from "./ui-state";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const leaf = (terminal_id: string): TabTree => ({ kind: "terminal", terminal_id });
const pane = (terminal_id: string): Terminal => ({
  terminal_id,
  tab_id: "w1:t1",
  space_id: "w1",
  agent_status: "idle",
  cwd: "/work/app",
});

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

/**
 * The real close flow of the pane dialog: `confirmClose` closes the pane, re-reads the snapshot
 * (which no longer has the pane) and only then hands the result to the host, which calls
 * `EditorTabs.closePane` with it (`App.onPaneClosed`).
 */
describe("closing a leaf of a split tab", () => {
  let current: Snapshot;
  let store: SpacesStore;
  let actions: PaneActions;
  let tabs: EditorTabs;

  beforeEach(async () => {
    tauri.reset();
    current = {
      spaces: [{ space_id: "w1", label: "One", number: 1, agent_status: "idle" }],
      tabs: [
        {
          tab_id: "w1:t1",
          space_id: "w1",
          label: "main",
          number: 1,
          agent_status: "idle",
          active_terminal: "w1:p1",
          root: {
            kind: "split",
            split_id: "s1",
            direction: "right",
            ratio: 0.5,
            children: [leaf("w1:p1"), leaf("w1:p2")],
          },
        },
      ],
      terminals: [pane("w1:p1"), pane("w1:p2")],
    };
    tauri.answer("snapshot", () => current);
    // As the server answers after the close: the pane is gone, the tab has one leaf left.
    tauri.answer("pane_close", (args) => {
      const paneId = args?.["paneId"];
      const rest = current.terminals.filter((p) => p.terminal_id !== paneId);
      current = {
        ...current,
        tabs: rest.length
          ? [
              {
                ...current.tabs[0],
                active_terminal: rest[0].terminal_id,
                root: leaf(rest[0].terminal_id),
              },
            ]
          : [],
        terminals: rest,
      };
    });
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: null }] });
    store = TestBed.inject(SpacesStore);
    actions = TestBed.inject(PaneActions);
    tabs = TestBed.inject(EditorTabs);
    await store.refresh();
  });

  async function close(paneId: string): Promise<void> {
    actions.askClose(paneId);
    const closed = await actions.confirmClose();
    expect(store.pane(paneId)).toBeUndefined();
    tabs.closePane(closed!.paneId, closed!.successor);
  }

  it("moves the editor tab of the closed pane to the remaining leaf", async () => {
    tabs.open("w1:p1", true);
    tabs.openFlow("w1:p1");
    tabs.activate("w1:p1");
    await close("w1:p1");
    expect(tabs.tabs().map((t) => t.paneId)).toEqual(["w1:p2"]);
    expect(tabs.activeId()).toBe("w1:p2");
    expect(tabs.groupOf(flowTab("w1:p1"))).toBeUndefined();
  });

  it("closes the editor tab with the last leaf", async () => {
    tabs.open("w1:p1", true);
    await close("w1:p1");
    await close("w1:p2");
    expect(tabs.tabs()).toEqual([]);
  });
});

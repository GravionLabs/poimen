// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { SpaceNode } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import { SpaceActions } from "../core/space-actions";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
import { Sidebar } from "./sidebar";

const node = (id: string, n: number): SpaceNode => ({
  workspace: { space_id: id, label: `Space ${id}`, number: n, agent_status: "idle" },
  rows: [
    {
      pane: { terminal_id: `${id}:p1`, tab_id: `${id}:t1`, space_id: id, agent_status: "idle" },
      tab: "main",
      name: "shell",
      worktree: false,
    },
  ],
});

async function setup() {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      {
        provide: SettingsStore,
        useValue: { settings: signal(DEFAULT_SETTINGS), writeError: signal(null) },
      },
      {
        provide: HerdrStore,
        useValue: {
          spaces: signal([node("w1", 1), node("w2", 2), node("w3", 3)]),
          disconnected: signal<string | null>(null),
        },
      },
    ],
  });
  TestBed.overrideComponent(Sidebar, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/sidebar/sidebar.html", "utf8"),
    },
  });
  TestBed.overrideComponent(Sidebar, {
    remove: { imports: [SplitPane] },
    add: { imports: [SplitPaneStub] },
  });
  const reorder = vi.spyOn(TestBed.inject(SpaceActions), "reorder").mockImplementation(() => {});
  const fixture = TestBed.createComponent(Sidebar);
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      fixture.detectChanges();
      await fixture.whenStable();
    }
  };
  await settle();
  const root = fixture.nativeElement as HTMLElement;
  const rowOf = (id: string) =>
    [...root.querySelectorAll<HTMLElement>(".space")].find((e) =>
      e.textContent?.includes(`Space ${id}`),
    )!;
  const fire = async (el: HTMLElement, type: string, clientY = 0) => {
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clientY", { value: clientY });
    el.dispatchEvent(event);
    await settle();
    return event;
  };
  return { fixture, root, reorder, rowOf, fire, settle };
}

describe("Sidebar space drag and drop", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("makes space rows draggable and shows a marker at the target", async () => {
    const { rowOf, fire } = await setup();
    expect(rowOf("w1").getAttribute("draggable")).toBe("true");
    await fire(rowOf("w1"), "dragstart");
    expect(rowOf("w1").classList).toContain("dragging");
    await fire(rowOf("w3"), "dragover", 0);
    expect(rowOf("w3").classList).toContain("drop-before");
    await fire(rowOf("w3"), "dragover", 5);
    expect(rowOf("w3").classList).toContain("drop-after");
  });

  it("reorders on a drop and clears the marker", async () => {
    const { rowOf, fire, reorder } = await setup();
    await fire(rowOf("w1"), "dragstart");
    await fire(rowOf("w3"), "dragover", 5);
    await fire(rowOf("w3"), "drop");
    expect(reorder).toHaveBeenCalledExactlyOnceWith("w1", "w3", true);
    expect(rowOf("w3").classList).not.toContain("drop-after");
    expect(rowOf("w1").classList).not.toContain("dragging");
  });

  it("changes nothing when the drag ends outside the list or over itself", async () => {
    const { rowOf, fire, reorder } = await setup();
    await fire(rowOf("w1"), "dragstart");
    await fire(rowOf("w1"), "dragover");
    await fire(rowOf("w1"), "drop");
    await fire(rowOf("w2"), "dragstart");
    await fire(rowOf("w2"), "dragend");
    expect(reorder).not.toHaveBeenCalled();
    expect(rowOf("w2").classList).not.toContain("dragging");
  });

  it("does not start a drag while a filter is active", async () => {
    const { root, rowOf, fire, reorder, settle } = await setup();
    const input = root.querySelector<HTMLInputElement>("input.filter")!;
    input.value = "space";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    expect(rowOf("w1").getAttribute("draggable")).toBeNull();
    const start = await fire(rowOf("w1"), "dragstart");
    expect(start.defaultPrevented).toBe(true);
    await fire(rowOf("w2"), "dragover", 5);
    await fire(rowOf("w2"), "drop");
    expect(reorder).not.toHaveBeenCalled();
  });
});

// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import type { Pane, SpaceNode } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { Sidebar } from "./sidebar";

const pane = (id: string): Pane => ({
  pane_id: id,
  tab_id: "w1:t1",
  workspace_id: "w1",
  agent_status: "idle",
});

const spaces: SpaceNode[] = [
  {
    workspace: { workspace_id: "w1", label: "One", number: 1, agent_status: "idle" },
    rows: [
      { pane: pane("w1:p1"), tab: "a", name: "claude", worktree: false },
      { pane: pane("w1:p2"), tab: "b", name: "shell", worktree: false },
    ],
  },
];

async function setup() {
  TestBed.configureTestingModule({
    providers: [
      { provide: HerdrStore, useValue: { spaces: signal(spaces), connectionError: signal(null) } },
    ],
  });
  // JIT cannot fetch templateUrl/styleUrl: inline the real template.
  TestBed.overrideComponent(Sidebar, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/sidebar/sidebar.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(Sidebar);
  const selected: string[] = [];
  const pinned: string[] = [];
  fixture.componentInstance.selected.subscribe((p) => selected.push(p.pane_id));
  fixture.componentInstance.pinned.subscribe((p) => pinned.push(p.pane_id));
  fixture.detectChanges();
  await fixture.whenStable();
  const root = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      fixture.detectChanges();
      await fixture.whenStable();
    }
  };
  /** Clicks the row and lets the tree and the sidebar settle. */
  const click = async (name: string) => {
    const item = [...root.querySelectorAll<HTMLElement>('[role="treeitem"]')].find((e) =>
      e.textContent?.includes(name),
    );
    if (!item) throw new Error(`no row ${name}`);
    (item.querySelector(".row") as HTMLElement).click();
    await settle();
  };
  return { click, selected, pinned };
}

describe("Sidebar with the real hx-tree", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("emits again when the same row is clicked a second time", async () => {
    const { click, selected, pinned } = await setup();
    await click("claude");
    expect(selected).toEqual(["w1:p1"]);
    // The second click within 400 ms is a double click: it pins.
    await click("claude");
    expect(pinned).toEqual(["w1:p1"]);
    // A third click starts over.
    await click("claude");
    expect(selected).toEqual(["w1:p1", "w1:p1"]);
  });
});

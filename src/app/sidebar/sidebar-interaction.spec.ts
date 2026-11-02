// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import type { Pane, SpaceNode } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { Sidebar } from "./sidebar";

const pane = (id: string, status: Pane["agent_status"] = "idle"): Pane => ({
  pane_id: id,
  tab_id: "w1:t1",
  workspace_id: "w1",
  agent_status: status,
});

const spaces: SpaceNode[] = [
  {
    workspace: { workspace_id: "w1", label: "One", number: 1, agent_status: "idle" },
    rows: [
      { pane: pane("w1:p1"), tab: "a", name: "claude", worktree: false },
      { pane: pane("w1:p2", "working"), tab: "b", name: "shell", worktree: false },
    ],
  },
];

async function setup() {
  TestBed.resetTestingModule();
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
  const workspace = () => root.querySelector<HTMLElement>('[role="treeitem"][aria-level="1"]')!;
  const key = async (target: HTMLElement, name: string) => {
    target.focus();
    target.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true }));
    await settle();
  };
  return { click, selected, pinned, root, settle, workspace, key };
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

  it("starts expanded and collapses and expands again with the chevron, without selecting", async () => {
    const { root, settle, workspace, selected } = await setup();
    expect(workspace().getAttribute("aria-expanded")).toBe("true");
    const chevron = workspace().querySelector<HTMLElement>(".chevron")!;
    expect(chevron.getAttribute("aria-hidden")).toBe("true");
    expect(chevron.classList.contains("open")).toBe(true);

    chevron.click();
    await settle();
    expect(workspace().getAttribute("aria-expanded")).toBe("false");
    expect(workspace().querySelector(".chevron")!.classList.contains("open")).toBe(false);
    // The collapsed workspace still shows its panes' aggregate state (#67).
    expect(workspace().querySelector(".space .state")?.textContent).toBe("working");
    expect(root.querySelector(".row")).toBeNull();

    workspace().querySelector<HTMLElement>(".chevron")!.click();
    await settle();
    expect(workspace().getAttribute("aria-expanded")).toBe("true");
    expect(root.querySelectorAll(".row")).toHaveLength(2);
    expect(selected).toEqual([]);
  });

  it("collapses with ArrowLeft and expands with ArrowRight on the workspace row", async () => {
    const { workspace, key } = await setup();
    await key(workspace(), "ArrowLeft");
    expect(workspace().getAttribute("aria-expanded")).toBe("false");
    await key(workspace(), "ArrowRight");
    expect(workspace().getAttribute("aria-expanded")).toBe("true");
  });

  it("hides helix's left toggle and gives panes no chevron", async () => {
    const { root } = await setup();
    expect(root.querySelectorAll(".chevron")).toHaveLength(1);
    expect(readFileSync("src/app/sidebar/sidebar.css", "utf8")).toMatch(
      /hx-tree-node-toggle\s*\{\s*display:\s*none/,
    );
  });
});

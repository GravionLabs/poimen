// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { Pane, SpaceNode } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
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

async function setup(message: string | null = null) {
  const ensureBackend = vi.fn();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      {
        provide: HerdrStore,
        useValue: {
          spaces: signal(spaces),
          disconnected: signal<string | null>(message),
          ensureBackend,
        },
      },
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
  TestBed.overrideComponent(Sidebar, {
    remove: { imports: [SplitPane] },
    add: { imports: [SplitPaneStub] },
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
  return { fixture, ensureBackend, click, selected, pinned, root, settle, workspace, key };
}

describe("Sidebar with the real hx-tree", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("shows one disconnected message with a Retry button that asks the backend again", async () => {
    const { root, ensureBackend } = await setup("herdr not reachable: refused");
    const messages = root.querySelectorAll(".connection");
    expect(messages).toHaveLength(1);
    expect(messages[0].textContent).toContain("herdr not reachable: refused");
    root.querySelector<HTMLButtonElement>(".retry")!.click();
    expect(ensureBackend).toHaveBeenCalledTimes(1);
  });

  it("shows no message or Retry while connected", async () => {
    const { root } = await setup();
    expect(root.querySelector(".connection")).toBeNull();
    expect(root.querySelector(".retry")).toBeNull();
  });

  it("titles the sidebar Spaces", async () => {
    const { root } = await setup();
    expect(root.querySelector(".brand")?.textContent?.trim()).toBe("Spaces");
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

  it("shows a labeled close icon directly left of the chevron", async () => {
    const { workspace } = await setup();
    const close = workspace().querySelector<HTMLButtonElement>(".close")!;
    expect(close.getAttribute("aria-label")).toBe("Close space One");
    expect(close.title).toBe("Close space One");
    expect(close.nextElementSibling?.classList.contains("chevron")).toBe(true);
  });

  it("closes through the menu action without selecting or toggling the row", async () => {
    const { fixture, workspace, settle, selected } = await setup();
    const actions: unknown[] = [];
    fixture.componentInstance.menuAction.subscribe((a) => actions.push(a));
    workspace().querySelector<HTMLButtonElement>(".close")!.click();
    await settle();
    expect(actions).toEqual([{ kind: "workspace", id: "w1", action: "close-space" }]);
    expect(selected).toEqual([]);
    expect(workspace().getAttribute("aria-expanded")).toBe("true");
  });

  it("keeps the close icon calm until hover, focus or the active space", () => {
    const css = readFileSync("src/app/sidebar/sidebar.css", "utf8");
    expect(css).toMatch(/\.close\s*\{[^}]*opacity:\s*0/);
    expect(css).toMatch(/\.space:hover \.close[^{]*\{\s*opacity:\s*1/);
    expect(css).toContain(".space.chosen .close");
    expect(css).toContain(".close:focus-visible");
    // the treeitem is rendered by hx-tree, so only ::ng-deep reaches it from this component
    expect(css).toMatch(
      /\.spaces ::ng-deep \[role="treeitem"\]:focus-visible \.close[^{]*\{\s*opacity:\s*1/,
    );
    expect(css).not.toContain(':host [role="treeitem"]');
  });

  it("makes the close icon a real button for Tab, Enter and Space", async () => {
    const { workspace } = await setup();
    const close = workspace().querySelector<HTMLButtonElement>(".close")!;
    expect(close.tagName).toBe("BUTTON");
    expect(close.tabIndex).toBe(0);
  });

  it("hides helix's left toggle and gives panes no chevron", async () => {
    const { root } = await setup();
    expect(root.querySelectorAll(".chevron")).toHaveLength(1);
    expect(readFileSync("src/app/sidebar/sidebar.css", "utf8")).toMatch(
      /hx-tree-node-toggle\s*\{\s*display:\s*none/,
    );
  });

  it("groups the pane context menu by plugin and moves focus over separators and headings", async () => {
    const { root, settle, fixture } = await setup();
    // JIT does not wire signal inputs for setInput here: replace the input signal itself.
    Object.assign(fixture.componentInstance, {
      pluginActions: signal([
        {
          plugin_id: "review-popup",
          action_id: "open",
          title: "Review",
          plugin_name: "Review Popup",
          description: null,
        },
        {
          plugin_id: "deliver-board",
          action_id: "open",
          title: "Open board",
          plugin_name: "Deliver Board",
          description: null,
        },
      ]),
    });
    const row = [...root.querySelectorAll<HTMLElement>(".row")].find((e) =>
      e.textContent?.includes("claude"),
    )!;
    row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await settle();

    expect(root.querySelectorAll('.menu [role="separator"]')).toHaveLength(2);
    const groups = [...root.querySelectorAll('.menu [role="group"]')];
    expect(groups.map((g) => g.getAttribute("aria-label"))).toEqual([
      "Actions",
      "Deliver Board",
      "Review Popup",
    ]);
    expect(root.querySelectorAll(".menu-heading")).toHaveLength(2);

    const menu = root.querySelector<HTMLElement>(".menu")!;
    const items = [...menu.querySelectorAll<HTMLElement>('[role="menuitem"]')];
    expect(items).toHaveLength(7);
    const press = (name: string) =>
      menu.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true }));
    items[4].focus();
    press("ArrowDown");
    expect(document.activeElement?.textContent?.trim()).toBe("Open board");
    press("ArrowDown");
    expect(document.activeElement?.textContent?.trim()).toBe("Review");
    press("Home");
    expect(document.activeElement?.textContent?.trim()).toBe("Split right");
    press("End");
    expect(document.activeElement?.textContent?.trim()).toBe("Review");
    press("o");
    expect(document.activeElement?.textContent?.trim()).toBe("Open board");

    // Modified keys are no type-ahead and stay unhandled.
    items[0].focus();
    const chord = new KeyboardEvent("keydown", {
      key: "o",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    menu.dispatchEvent(chord);
    expect(chord.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(items[0]);

    // Escape closes the menu even when the container itself has focus.
    menu.focus();
    menu.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await settle();
    expect(root.querySelector(".menu")).toBeNull();
  });
});

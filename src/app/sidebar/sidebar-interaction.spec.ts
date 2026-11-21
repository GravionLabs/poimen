// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { Terminal, SpaceNode } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { SpaceActions } from "../core/space-actions";
import { Sidebar } from "./sidebar";
import { inlineSpaceCard } from "./space-card.mock";

const pane = (id: string, status: Terminal["agent_status"] = "idle"): Terminal => ({
  terminal_id: id,
  tab_id: "w1:t1",
  space_id: "w1",
  agent_status: status,
});

const spaces: SpaceNode[] = [
  {
    workspace: { space_id: "w1", label: "One", number: 1, agent_status: "idle" },
    rows: [
      { pane: pane("w1:p1"), tab: "a", name: "claude", worktree: false },
      { pane: pane("w1:p2", "working"), tab: "b", name: "shell", worktree: false },
    ],
  },
];

async function setup(message: string | null = null, nodes: SpaceNode[] = spaces) {
  const ensureBackend = vi.fn();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      {
        provide: SpacesStore,
        useValue: {
          spaces: signal(nodes),
          disconnected: signal<string | null>(message),
          ensureBackend,
        },
      },
    ],
  });
  // JIT cannot fetch templateUrl/styleUrl: inline the real template.
  inlineSpaceCard();
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
  fixture.componentInstance.selected.subscribe((p) => selected.push(p.terminal_id));
  fixture.componentInstance.pinned.subscribe((p) => pinned.push(p.terminal_id));
  fixture.detectChanges();
  await fixture.whenStable();
  const root = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      fixture.detectChanges();
      await fixture.whenStable();
    }
  };
  /** Clicks the row and lets the cards and the sidebar settle. */
  const click = async (name: string) => {
    const item = [...root.querySelectorAll<HTMLElement>(".row")].find((e) =>
      e.textContent?.includes(name),
    );
    if (!item) throw new Error(`no row ${name}`);
    item.click();
    await settle();
  };
  const workspace = () => root.querySelector<HTMLElement>('[role="treeitem"][aria-level="1"]')!;
  const key = async (target: HTMLElement, name: string) => {
    target.focus();
    target.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true }));
    await settle();
  };
  const spaceActions = TestBed.inject(SpaceActions);
  return {
    fixture,
    spaceActions,
    ensureBackend,
    click,
    selected,
    pinned,
    root,
    settle,
    workspace,
    key,
  };
}

describe("Sidebar cards", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("hints at a restart on the second line of a terminal on an older host, with both versions in the tooltip", async () => {
    const old: Terminal = {
      ...pane("w1:p3"),
      host_version: "0.15.0",
      host_bundled_version: "0.16.0",
    };
    const nodes: SpaceNode[] = [
      {
        workspace: spaces[0].workspace,
        rows: [
          { pane: old, tab: "c", name: "old", worktree: false },
          { pane: pane("w1:p4"), tab: "d", name: "current", worktree: false },
        ],
      },
    ];
    const { root } = await setup(null, nodes);
    const marks = [...root.querySelectorAll<HTMLElement>(".host-hint")];
    expect(marks).toHaveLength(1);
    expect(marks[0].textContent).toContain("restart");
    expect(marks[0].title).toContain("0.15.0");
    expect(marks[0].title).toContain("0.16.0");
    const lines = [...marks[0].closest(".main")!.querySelectorAll(".line")];
    expect(marks[0].closest(".line")).toBe(lines[lines.length - 1]);
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

  it("opens the overview of the space on a click on its row, not on a pane row", async () => {
    const { fixture, workspace, settle, click, selected } = await setup();
    const opened: string[] = [];
    fixture.componentInstance.spaceOpened.subscribe((id) => opened.push(id));
    workspace().click();
    await settle();
    expect(opened).toEqual(["w1"]);
    await click("claude");
    expect(opened).toEqual(["w1"]);
    expect(selected).toEqual(["w1:p1"]);
  });

  describe("inline rename", () => {
    const dbl = async (s: Awaited<ReturnType<typeof setup>>) => {
      const name = s.root.querySelector<HTMLElement>("header .name")!;
      name.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
      await s.settle();
      return s.root.querySelector<HTMLInputElement>("header input.edit")!;
    };

    it("starts an edit prefilled with the label on a double click and confirms with Enter", async () => {
      const s = await setup();
      const rename = vi.spyOn(s.spaceActions, "rename").mockResolvedValue(true);
      const input = await dbl(s);
      expect(input.value).toBe("One");
      input.value = "Uno";
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      await s.settle();
      expect(rename).toHaveBeenCalledWith("w1", "Uno");
      expect(s.root.querySelector("header input.edit")).toBeNull();
    });

    it("focuses and selects the input when the edit starts", async () => {
      const s = await setup();
      const input = await dbl(s);
      expect(document.activeElement).toBe(input);
      expect(input.selectionStart).toBe(0);
      expect(input.selectionEnd).toBe(input.value.length);
    });

    it("focuses the input again after a failed commit", async () => {
      const s = await setup();
      vi.spyOn(s.spaceActions, "rename").mockResolvedValue(false);
      const input = await dbl(s);
      input.value = "";
      input.blur();
      await s.settle();
      expect(s.root.querySelector("header input.edit")).toBe(input);
      expect(document.activeElement).toBe(input);
    });

    it("cancels with Escape without renaming", async () => {
      const s = await setup();
      const rename = vi.spyOn(s.spaceActions, "rename").mockResolvedValue(true);
      const input = await dbl(s);
      input.value = "Uno";
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      await s.settle();
      expect(rename).not.toHaveBeenCalled();
      expect(s.root.querySelector("header input.edit")).toBeNull();
      expect(s.root.querySelector("header .name")?.textContent).toContain("One");
    });

    it("cancels on blur without a change", async () => {
      const s = await setup();
      const rename = vi.spyOn(s.spaceActions, "rename").mockResolvedValue(true);
      const input = await dbl(s);
      input.dispatchEvent(new FocusEvent("blur"));
      await s.settle();
      expect(rename).not.toHaveBeenCalled();
      expect(s.root.querySelector("header input.edit")).toBeNull();
    });

    it("keeps the edit open when the rename fails", async () => {
      const s = await setup();
      vi.spyOn(s.spaceActions, "rename").mockResolvedValue(false);
      const input = await dbl(s);
      input.value = "";
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      await s.settle();
      expect(s.root.querySelector("header input.edit")).not.toBeNull();
    });
  });

  it("moves focus over the entries of the pane context menu", async () => {
    const { root, settle } = await setup();
    const row = [...root.querySelectorAll<HTMLElement>(".row")].find((e) =>
      e.textContent?.includes("claude"),
    )!;
    row.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await settle();

    const groups = [...root.querySelectorAll('.menu [role="group"]')];
    expect(groups.map((g) => g.getAttribute("aria-label"))).toEqual(["Actions"]);

    const menu = root.querySelector<HTMLElement>(".menu")!;
    const items = [...menu.querySelectorAll<HTMLElement>('[role="menuitem"]')];
    expect(items).toHaveLength(5);
    const press = (name: string) =>
      menu.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true }));
    items[4].focus();
    press("ArrowDown");
    expect(document.activeElement?.textContent?.trim()).toBe("Split right");
    press("End");
    expect(document.activeElement?.textContent?.trim()).toBe("Close pane...");
    press("Home");
    expect(document.activeElement?.textContent?.trim()).toBe("Split right");
    press("r");
    expect(document.activeElement?.textContent?.trim()).toBe("Rename...");

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

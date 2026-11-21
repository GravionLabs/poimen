// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { Terminal, SpaceNode } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
import { SpaceActions } from "../core/space-actions";
import { PaneActions } from "../core/pane-actions";
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
          pane: (id: string) =>
            nodes.flatMap((n) => n.rows).find((r) => r.pane.terminal_id === id)?.pane,
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
  TestBed.overrideComponent(Sidebar, {
    remove: { imports: [SplitPane] },
    add: { imports: [SplitPaneStub] },
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
  const spaceActions = TestBed.inject(SpaceActions);
  const paneActions = TestBed.inject(PaneActions);
  return {
    fixture,
    spaceActions,
    paneActions,
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

describe("Sidebar tree: inline terminal rename (#1035)", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  const nameOf = (s: Awaited<ReturnType<typeof setup>>, text: string) =>
    [...s.root.querySelectorAll<HTMLElement>(".row .name")].find((e) => e.textContent === text)!;
  const edit = async (s: Awaited<ReturnType<typeof setup>>) => {
    nameOf(s, "claude").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    await s.settle();
    return s.root.querySelector<HTMLInputElement>("input.pane-edit")!;
  };
  const press = async (
    s: Awaited<ReturnType<typeof setup>>,
    el: HTMLElement,
    init: KeyboardEventInit,
  ) => {
    el.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, ...init }));
    await s.settle();
  };

  it("edits in place on a double click and saves with Enter", async () => {
    const s = await setup();
    const rename = vi.spyOn(s.paneActions, "rename").mockResolvedValue(true);
    const input = await edit(s);
    expect(input.value).toBe("claude");
    expect(document.activeElement).toBe(input);
    input.value = "builder";
    await press(s, input, { key: "Enter" });
    expect(rename).toHaveBeenCalledWith("w1:p1", "builder");
    expect(s.root.querySelector("input.pane-edit")).toBeNull();
    expect(s.selected).toEqual([]);
  });

  it("starts with F2 on the focused row and with Rename from the menu", async () => {
    const s = await setup();
    const item = [
      ...s.root.querySelectorAll<HTMLElement>('[role="treeitem"][aria-level="2"]'),
    ].find((e) => e.textContent?.includes("shell"))!;
    await press(s, item, { key: "F2" });
    expect(s.paneActions.renaming()).toBe("w1:p2");
    s.paneActions.cancelRename();
    s.paneActions.askRename("w1:p1");
    await s.settle();
    expect(s.root.querySelector("input.pane-edit")).not.toBeNull();
  });

  it("does not start F2 on a space row", async () => {
    const s = await setup();
    await press(s, s.workspace(), { key: "F2" });
    expect(s.paneActions.renaming()).toBeNull();
  });

  it("cancels with Escape and on a blur without a change", async () => {
    const s = await setup();
    const rename = vi.spyOn(s.paneActions, "rename").mockResolvedValue(true);
    let input = await edit(s);
    input.value = "x";
    await press(s, input, { key: "Escape" });
    expect(s.root.querySelector("input.pane-edit")).toBeNull();
    input = await edit(s);
    input.dispatchEvent(new FocusEvent("blur"));
    await s.settle();
    expect(s.root.querySelector("input.pane-edit")).toBeNull();
    expect(rename).not.toHaveBeenCalled();
  });

  it("saves on blur with a change", async () => {
    const s = await setup();
    const rename = vi.spyOn(s.paneActions, "rename").mockResolvedValue(true);
    const input = await edit(s);
    input.value = "other";
    input.dispatchEvent(new FocusEvent("blur"));
    await s.settle();
    expect(rename).toHaveBeenCalledWith("w1:p1", "other");
  });

  it("does not save an Enter that ends an IME composition", async () => {
    const s = await setup();
    const rename = vi.spyOn(s.paneActions, "rename").mockResolvedValue(true);
    const input = await edit(s);
    input.value = "にほん";
    await press(s, input, { key: "Enter", isComposing: true });
    await press(s, input, { key: "Enter", keyCode: 229 });
    expect(rename).not.toHaveBeenCalled();
    expect(s.root.querySelector("input.pane-edit")).not.toBeNull();
  });

  it("disables the field while sending and keeps text and error on failure", async () => {
    const s = await setup();
    const input = await edit(s);
    let release!: () => void;
    vi.spyOn(s.paneActions, "run" as never);
    const bridge = (s.paneActions as unknown as { bridge: { invoke: unknown } }).bridge;
    vi.spyOn(bridge as { invoke: () => Promise<unknown> }, "invoke").mockImplementation(
      () => new Promise((_, reject) => (release = () => reject("no luck"))),
    );
    input.value = "kept";
    await press(s, input, { key: "Enter" });
    expect(input.disabled).toBe(true);
    release();
    await s.settle();
    const after = s.root.querySelector<HTMLInputElement>("input.pane-edit")!;
    expect(after.disabled).toBe(false);
    expect(after.value).toBe("kept");
    expect(s.root.querySelector(".pane-edit-error")?.textContent).toContain("no luck");
  });

  it("keeps the row box: the field has the line height and no extra padding", () => {
    const css = readFileSync("src/app/sidebar/sidebar.css", "utf8");
    expect(css).toMatch(/\.pane-edit\s*\{[^}]*height:\s*16px/);
  });
});

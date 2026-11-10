// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { Component, EventEmitter, Input, Output } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import type { Mode, TabTree, Terminal } from "../core/herdr";
import { TerminalView } from "../terminal-view/terminal-view";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
import { TabLayout, cycleFocus, focusDelta } from "./tab-layout";

@Component({ selector: "app-terminal-view", template: "" })
class TerminalViewStub {
  @Input() pane!: Terminal;
  @Input() wanted: Mode = "observe";
  @Output() readonly modeChange = new EventEmitter<{ mode: Mode; blocked: string | null }>();
  @Output() readonly sized = new EventEmitter<{ cols: number; rows: number }>();
}

const leaf = (terminal_id: string): TabTree => ({ kind: "terminal", terminal_id });
// t1 | (t2 over t3): the outer split goes right at 0.3, the inner one down at 0.5.
const tree: TabTree = {
  kind: "split",
  split_id: "split1",
  direction: "right",
  ratio: 0.3,
  children: [
    leaf("t1"),
    {
      kind: "split",
      split_id: "split2",
      direction: "down",
      ratio: 0.5,
      children: [leaf("t2"), leaf("t3")],
    },
  ],
};
const term = (id: string): Terminal => ({
  terminal_id: id,
  tab_id: "tab1",
  space_id: "s1",
  agent_status: "idle",
});
const panes = new Map(["t1", "t2", "t3"].map((id) => [id, term(id)]));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
    errorOnUnknownElements: true,
  });
});

function setup(root: TabTree = tree, focused: string | null = "t1") {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({});
  TestBed.overrideComponent(TabLayout, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/tab-layout/tab-layout.html", "utf8"),
    },
  });
  TestBed.overrideComponent(TabLayout, {
    remove: { imports: [SplitPane, TerminalView] },
    add: { imports: [SplitPaneStub, TerminalViewStub] },
  });
  const fixture = TestBed.createComponent(TabLayout);
  const c = fixture.componentInstance;
  c.tabId = "tab1";
  c.root = root;
  c.panes = panes;
  c.focused = focused;
  c.wanted = "control";
  const log = {
    ratios: [] as { splitId: string; ratio: number }[],
    focus: [] as string[],
    actions: [] as { terminalId: string; action: string }[],
  };
  c.ratioChange.subscribe((r) => log.ratios.push(r));
  c.focusChange.subscribe((f) => log.focus.push(f));
  c.paneAction.subscribe((a) => log.actions.push(a));
  fixture.detectChanges();
  const el = fixture.nativeElement as HTMLElement;
  return { fixture, el, c, log };
}

describe("TabLayout", () => {
  it("renders one terminal view per leaf in the nesting of the tree", () => {
    const { el } = setup();
    const outer = el.querySelector(":scope > app-split-pane")!;
    expect(outer).toBeTruthy();
    expect(el.querySelectorAll("app-terminal-view")).toHaveLength(3);
    expect(outer.querySelectorAll("app-split-pane")).toHaveLength(1);
    // t1 sits directly in the outer split; t2 and t3 in the inner one.
    expect(outer.querySelector(":scope > .pane > .leaf")?.getAttribute("data-terminal")).toBe("t1");
    const inner = outer.querySelector("app-split-pane")!;
    expect(
      [...inner.querySelectorAll(".leaf")].map((l) => l.getAttribute("data-terminal")),
    ).toEqual(["t2", "t3"]);
  });

  it("takes the share of each split from its ratio", () => {
    const { el } = setup();
    const outer = el.querySelector(":scope > app-split-pane")!;
    const [first, second] = [...outer.querySelectorAll<HTMLElement>(":scope > .pane")];
    expect(first.style.flexGrow).toBe("0.3");
    expect(Number(second.style.flexGrow)).toBeCloseTo(0.7);
  });

  it("gives the wanted mode to the focused leaf only", () => {
    const { fixture, el } = setup();
    const modes = () =>
      [...el.querySelectorAll(".leaf")].map((l) => [
        l.getAttribute("data-terminal"),
        l.classList.contains("focused"),
      ]);
    expect(modes()).toEqual([
      ["t1", true],
      ["t2", false],
      ["t3", false],
    ]);
    fixture.componentRef.setInput("focused", "t3");
    fixture.detectChanges();
    expect(modes().find(([, f]) => f)?.[0]).toBe("t3");
  });

  it("reports the ratio of the dragged divider with its split id", () => {
    const { el, log } = setup();
    const dividers = el.querySelectorAll<HTMLElement>(".divider");
    // The outer divider comes first in document order, the inner one after it.
    dividers[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    dividers[1].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    expect(log.ratios[0].splitId).toBe("split1");
    expect(log.ratios[0].ratio).toBeCloseTo(0.32);
    expect(log.ratios[1].splitId).toBe("split2");
    expect(log.ratios[1].ratio).toBeCloseTo(0.52);
  });

  it("moves focus with a click on a leaf", () => {
    const { el, log } = setup();
    el.querySelector('[data-terminal="t2"]')!.dispatchEvent(
      new Event("pointerdown", { bubbles: true }),
    );
    expect(log.focus).toEqual(["t2"]);
  });

  it("moves focus with Alt+arrows in tree order, wrapping, and keeps the key from the terminal", () => {
    const { el, log } = setup(tree, "t3");
    const key = (k: string, extra: KeyboardEventInit = {}) => {
      const event = new KeyboardEvent("keydown", {
        key: k,
        altKey: true,
        bubbles: true,
        cancelable: true,
        ...extra,
      });
      el.querySelector('[data-terminal="t3"]')!.dispatchEvent(event);
      return event;
    };
    expect(key("ArrowRight").defaultPrevented).toBe(true);
    expect(log.focus).toEqual(["t1"]);
    key("ArrowLeft");
    expect(log.focus).toEqual(["t1", "t2"]);
    expect(key("ArrowLeft", { ctrlKey: true }).defaultPrevented).toBe(false);
    expect(log.focus).toHaveLength(2);
  });

  it("offers split right, split down and close pane in the context menu of a leaf", () => {
    const { fixture, el, log } = setup();
    el.querySelector('[data-terminal="t2"]')!.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 20 }),
    );
    fixture.detectChanges();
    const items = [...el.querySelectorAll<HTMLButtonElement>(".menu button")];
    expect(items.map((b) => b.textContent!.trim())).toEqual([
      "Split right",
      "Split down",
      "Close pane",
    ]);
    expect(log.focus).toEqual(["t2"]);
    items[1].click();
    fixture.detectChanges();
    expect(log.actions).toEqual([{ terminalId: "t2", action: "split-down" }]);
    expect(el.querySelector(".menu")).toBeNull();
  });

  it("leaves out a leaf whose terminal is not in the snapshot", () => {
    const { fixture, el } = setup();
    fixture.componentRef.setInput("panes", new Map([["t1", term("t1")]]));
    fixture.detectChanges();
    expect(el.querySelectorAll("app-terminal-view")).toHaveLength(1);
  });
});

describe("cycleFocus", () => {
  it("steps through the leaves and wraps", () => {
    const leaves = ["a", "b", "c"];
    expect(cycleFocus(leaves, "a", 1)).toBe("b");
    expect(cycleFocus(leaves, "c", 1)).toBe("a");
    expect(cycleFocus(leaves, "a", -1)).toBe("c");
  });

  it("starts at the first leaf for an unknown or missing focus, and is null without leaves", () => {
    expect(cycleFocus(["a", "b"], "zz", -1)).toBe("a");
    expect(cycleFocus(["a", "b"], null, 1)).toBe("a");
    expect(cycleFocus([], "a", 1)).toBeNull();
  });
});

describe("focusDelta", () => {
  const e = (key: string, over = {}) => ({
    key,
    altKey: true,
    ctrlKey: false,
    metaKey: false,
    ...over,
  });
  it("maps Alt+arrows and ignores everything else", () => {
    expect(focusDelta(e("ArrowRight"))).toBe(1);
    expect(focusDelta(e("ArrowUp"))).toBe(-1);
    expect(focusDelta(e("ArrowRight", { altKey: false }))).toBeNull();
    expect(focusDelta(e("a"))).toBeNull();
    expect(focusDelta(e("ArrowLeft", { metaKey: true }))).toBeNull();
  });
});

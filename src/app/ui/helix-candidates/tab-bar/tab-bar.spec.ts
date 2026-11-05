// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { Component, signal } from "@angular/core";
import { ComponentFixture, TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { TabBar, TabBarItem, arrowTarget, dropMove, itemsForTab } from "./tab-bar";

describe("dropMove", () => {
  it("moves the dragged tab to the drop index", () => {
    expect(dropMove(0, 2)).toEqual({ from: 0, to: 2 });
    expect(dropMove(3, 1)).toEqual({ from: 3, to: 1 });
  });

  it("is null without a drag or when dropped on itself", () => {
    expect(dropMove(null, 1)).toBeNull();
    expect(dropMove(1, 1)).toBeNull();
  });
});

describe("arrowTarget", () => {
  it("steps right and left, wrapping around", () => {
    expect(arrowTarget("ArrowRight", 1, 3)).toBe(2);
    expect(arrowTarget("ArrowRight", 2, 3)).toBe(0);
    expect(arrowTarget("ArrowLeft", 0, 3)).toBe(2);
  });

  it("ignores other keys and an empty bar", () => {
    expect(arrowTarget("Enter", 0, 3)).toBeNull();
    expect(arrowTarget("ArrowRight", 0, 0)).toBeNull();
  });
});

describe("itemsForTab", () => {
  const items = [
    { action: "close", label: "Close" },
    { action: "only-p5", label: "Only p5", appliesTo: (id: string) => id === "p5" },
  ];

  it("offers a conditional entry only for the tabs it applies to", () => {
    expect(itemsForTab(items, "p5").map((i) => i.action)).toEqual(["close", "only-p5"]);
    expect(itemsForTab(items, "p1").map((i) => i.action)).toEqual(["close"]);
  });
});

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

const items: TabBarItem[] = [
  { id: "a", label: "one" },
  { id: "b", label: "two", attention: true },
  { id: "c", label: "three", group: "ws" },
];

@Component({
  selector: "app-host",
  imports: [TabBar],
  template: `<app-tab-bar
    [tabs]="tabs"
    [activeId]="active()"
    (selected)="picked.push($event)"
    (moved)="moves.push($event)"
    (received)="gets.push($event)"
  />`,
})
class Host {
  tabs = items;
  active = signal<string | null>("a");
  picked: string[] = [];
  moves: { from: number; to: number }[] = [];
  gets: { from: string; id: string; index: number }[] = [];
}

function mount(): { fixture: ComponentFixture<Host>; root: HTMLElement; strip: HTMLElement } {
  TestBed.resetTestingModule();
  TestBed.overrideComponent(TabBar, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/ui/helix-candidates/tab-bar/tab-bar.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(Host);
  fixture.detectChanges();
  const root = fixture.nativeElement as HTMLElement;
  return { fixture, root, strip: root.querySelector(".strip") as HTMLElement };
}

/** jsdom has no layout: pretend the strip is `scroll` wide in a `client` wide box. */
function overflow(strip: HTMLElement, scroll: number, client: number): void {
  Object.defineProperty(strip, "scrollWidth", { configurable: true, value: scroll });
  Object.defineProperty(strip, "clientWidth", { configurable: true, value: client });
  strip.dispatchEvent(new Event("scroll"));
}

describe("TabBar overflow", () => {
  it("shows no scroll or all-tabs buttons while the tabs fit", () => {
    const { fixture, root, strip } = mount();
    overflow(strip, 300, 300);
    fixture.detectChanges();
    expect(root.querySelector(".hx-tab-nav")).toBeNull();
  });

  it("shows them while the tabs overflow and hides them again after a resize", () => {
    const { fixture, root, strip } = mount();
    overflow(strip, 900, 300);
    fixture.detectChanges();
    expect(root.querySelector('[aria-label="Scroll tabs left"]')).not.toBeNull();
    expect(root.querySelector('[aria-label="Scroll tabs right"]')).not.toBeNull();
    expect(root.querySelector('[aria-label="Show all tabs"]')).not.toBeNull();
    overflow(strip, 300, 300);
    fixture.detectChanges();
    expect(root.querySelector(".hx-tab-nav")).toBeNull();
  });

  it("scrolls by 60% of the width on a button click", () => {
    const { fixture, root, strip } = mount();
    overflow(strip, 900, 300);
    fixture.detectChanges();
    const scrollBy = vi.fn();
    strip.scrollBy = scrollBy;
    (root.querySelector('[aria-label="Scroll tabs right"]') as HTMLElement).click();
    (root.querySelector('[aria-label="Scroll tabs left"]') as HTMLElement).click();
    expect(scrollBy.mock.calls.map((c) => c[0].left)).toEqual([180, -180]);
  });

  it("turns the vertical wheel into horizontal scrolling only while overflowing", () => {
    const { fixture, strip } = mount();
    const fits = new WheelEvent("wheel", { deltaY: 40, cancelable: true });
    strip.dispatchEvent(fits);
    expect(fits.defaultPrevented).toBe(false);
    overflow(strip, 900, 300);
    fixture.detectChanges();
    const wheel = new WheelEvent("wheel", { deltaY: 40, cancelable: true });
    strip.dispatchEvent(wheel);
    expect(wheel.defaultPrevented).toBe(true);
    expect(strip.scrollLeft).toBe(40);
  });

  it("lists every tab in the all-tabs menu and activates the chosen one", () => {
    const { fixture, root, strip } = mount();
    overflow(strip, 900, 300);
    fixture.detectChanges();
    (root.querySelector('[aria-label="Show all tabs"]') as HTMLElement).click();
    fixture.detectChanges();
    const entries = [...root.querySelectorAll<HTMLElement>(".all-menu .menu-item")];
    expect(entries.map((e) => e.textContent?.replace(/\s+/g, " ").trim())).toEqual([
      "one",
      "two",
      "ws / three",
    ]);
    expect(entries[1].querySelector(".attention-mark")).not.toBeNull();
    entries[2].click();
    fixture.detectChanges();
    expect(fixture.componentInstance.picked).toEqual(["c"]);
    expect(root.querySelector(".all-menu")).toBeNull();
  });

  it("scrolls the active tab into view when it changes", async () => {
    const { fixture, root } = mount();
    const spies = [...root.querySelectorAll<HTMLElement>('[role="tab"]')].map((tab) => {
      const spy = vi.fn();
      tab.scrollIntoView = spy;
      return spy;
    });
    fixture.componentInstance.active.set("c");
    fixture.detectChanges();
    TestBed.tick();
    expect(spies[2]).toHaveBeenCalledWith({ block: "nearest", inline: "nearest" });
    expect(spies[0]).not.toHaveBeenCalled();
  });

  it("still reorders by drag and drop", () => {
    const { fixture, root } = mount();
    const rows = root.querySelectorAll<HTMLElement>('[role="tab"]');
    const ev = (type: string): DragEvent => {
      const e = new Event(type, { bubbles: true, cancelable: true }) as DragEvent;
      return e;
    };
    rows[0].dispatchEvent(ev("dragstart"));
    rows[2].dispatchEvent(ev("drop"));
    expect(fixture.componentInstance.moves).toEqual([{ from: 0, to: 2 }]);
  });
});

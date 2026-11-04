// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { Component } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import type { PaletteItem } from "../core/command-palette";
import { titleParts } from "./title-parts";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

const item = (key: string, title: string, origin = ""): PaletteItem => ({
  kind: "command",
  key,
  title,
  origin,
  groupId: origin,
  shortcut: origin ? "" : "Ctrl+X",
  command: "refresh",
});

const a = item("a", "Alpha");
const b = item("b", "Beta");
const c = item("c", "Gamma", "plug");

// JIT in vitest does not wire signal inputs, so the real template is rendered by a stand-in host
// with a minimal palette that mirrors the real navigation (selection over entries only).
function makeHost() {
  let selected = 0;
  const entries = [a, b, c];
  const rows = () =>
    [
      { type: "heading", key: "h1", label: "Poimen" },
      { type: "item", key: "a", item: a, index: 0, positions: [0] },
      { type: "item", key: "b", item: b, index: 1, positions: [] },
      { type: "heading", key: "h2", label: "plug" },
      { type: "item", key: "c", item: c, index: 2, positions: [] },
    ] as const;
  const palette = {
    isOpen: () => true,
    query: () => "",
    loadError: () => null,
    rows,
    selected: () => selected,
    hover: (i: number) => (selected = i),
    move: (d: number) => (selected = (selected + d + entries.length) % entries.length),
    current: () => entries[selected],
    close: () => undefined,
    setQuery: () => undefined,
  };
  @Component({
    selector: "app-host",
    template: readFileSync("src/app/command-palette/command-palette.html", "utf8"),
  })
  class Host {
    palette = palette;
    parts = titleParts;
    run = (): void => undefined;
    type = (): void => undefined;
    key(event: KeyboardEvent): void {
      if (event.key === "ArrowDown") palette.move(1);
      else if (event.key === "ArrowUp") palette.move(-1);
    }
  }
  return Host;
}

describe("command palette template", () => {
  it("renders headings outside the options and marks the matched characters", () => {
    TestBed.resetTestingModule();
    const fixture = TestBed.createComponent(makeHost());
    fixture.detectChanges();
    const el = fixture.nativeElement as HTMLElement;
    expect(el.querySelectorAll('[role="option"]')).toHaveLength(3);
    expect([...el.querySelectorAll(".heading")].map((h) => h.textContent?.trim())).toEqual([
      "Poimen",
      "plug",
    ]);
    expect(el.querySelector('[role="option"] mark')?.textContent).toBe("A");
    expect(el.querySelector(".footer")?.textContent).toBe("↑↓ navigate · ↵ run · Esc close");
    expect(el.querySelector("input")?.getAttribute("placeholder")).toBe("Type an action name");
  });

  it("steps over a heading with the arrow keys", () => {
    TestBed.resetTestingModule();
    const fixture = TestBed.createComponent(makeHost());
    fixture.detectChanges();
    const el = fixture.nativeElement as HTMLElement;
    const input = el.querySelector("input")!;
    const selectedTitle = () =>
      el.querySelector('[aria-selected="true"] .title')?.textContent?.trim();
    expect(selectedTitle()).toBe("Alpha");
    for (const [key, expected] of [
      ["ArrowDown", "Beta"],
      ["ArrowDown", "Gamma"],
      ["ArrowDown", "Alpha"],
      ["ArrowUp", "Gamma"],
    ] as const) {
      input.dispatchEvent(new KeyboardEvent("keydown", { key }));
      fixture.detectChanges();
      expect(selectedTitle()).toBe(expected);
    }
    expect(el.querySelectorAll('[aria-selected="true"]')).toHaveLength(1);
    expect(el.querySelector(".heading")?.getAttribute("aria-selected")).toBeNull();
  });
});

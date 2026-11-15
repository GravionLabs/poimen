// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import type { JumpEntry } from "../core/jump-palette";
import { JumpPalette } from "../core/jump-palette";
import { JumpPaletteView } from "./jump-palette";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});
Element.prototype.scrollIntoView = () => undefined;
afterEach(() => TestBed.resetTestingModule());

const entry = (key: string, disabled: string | null = null): JumpEntry => ({
  key,
  kind: "space",
  title: key,
  detail: "",
  spaceId: key,
  disabled,
});

function setup(entries: JumpEntry[]) {
  const selected = signal(0);
  const open = signal(true);
  const palette = {
    isOpen: open,
    query: () => "",
    rows: () =>
      entries.map((e, index) => ({ type: "item", key: e.key, entry: e, index, positions: [] })),
    selected,
    hover: (i: number) => selected.set(i),
    move: (d: number) => selected.set((selected() + d + entries.length) % entries.length),
    current: () => entries[selected()],
    close: () => open.set(false),
    setQuery: () => undefined,
  };
  TestBed.configureTestingModule({ providers: [{ provide: JumpPalette, useValue: palette }] });
  // JIT in vitest does not load template files: give the real template inline.
  TestBed.overrideComponent(JumpPaletteView, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/jump-palette/jump-palette.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(JumpPaletteView);
  const chosen: JumpEntry[] = [];
  fixture.componentInstance.chosen.subscribe((e) => chosen.push(e));
  fixture.detectChanges();
  const input = (fixture.nativeElement as HTMLElement).querySelector("input")!;
  const press = (key: string) => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key, cancelable: true }));
    fixture.detectChanges();
  };
  return { fixture, press, chosen, open };
}

describe("JumpPaletteView", () => {
  it("moves with the arrows and chooses with Enter", () => {
    const { press, chosen } = setup([entry("a"), entry("b")]);
    press("ArrowDown");
    press("Enter");
    expect(chosen.map((e) => e.key)).toEqual(["b"]);
  });

  it("does not choose a disabled entry", () => {
    const { press, chosen, open } = setup([entry("a", "m is offline")]);
    press("Enter");
    expect(chosen).toEqual([]);
    expect(open()).toBe(true);
  });

  it("closes with Escape and returns the focus", () => {
    const outside = document.createElement("button");
    document.body.appendChild(outside);
    outside.focus();
    const { fixture, press, open } = setup([entry("a")]);
    fixture.detectChanges();
    press("Escape");
    expect(open()).toBe(false);
    expect(document.activeElement).toBe(outside);
    outside.remove();
  });
});

// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { ChipPicker } from "./chip-picker";

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

function make(selected: string[], options: string[], disabled = false) {
  TestBed.resetTestingModule();
  TestBed.overrideComponent(ChipPicker, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/github/chip-picker/chip-picker.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(ChipPicker);
  const c = fixture.componentInstance;
  c.label = "Labels";
  c.selected = selected;
  c.options = options;
  c.disabled = disabled;
  const added: string[] = [];
  const removed: string[] = [];
  c.add.subscribe((v) => added.push(v));
  c.remove.subscribe((v) => removed.push(v));
  fixture.detectChanges();
  return { fixture, el: fixture.nativeElement as HTMLElement, added, removed };
}

function type(el: HTMLElement, text: string) {
  const input = el.querySelector<HTMLInputElement>("[data-testid=chip-input]")!;
  input.value = text;
  input.dispatchEvent(new Event("input"));
}

describe("chip picker", () => {
  it("does not point list at a datalist", () => {
    const { el } = make([], ["a", "b"]);
    expect(el.querySelector("[data-testid=chip-input]")!.hasAttribute("list")).toBe(false);
  });

  it("shows chips and emits remove", () => {
    const { el, removed } = make(["a", "b"], ["a", "b", "c"]);
    expect(
      [...el.querySelectorAll("[data-testid=chip]")].map((c) => c.textContent!.trim()),
    ).toEqual(
      ["a ×".replace(" ×", "") + "", "b"].map(
        (t) => expect.stringContaining(t) as unknown as string,
      ),
    );
    el.querySelector<HTMLElement>("[data-testid=chip-remove]")!.click();
    expect(removed).toEqual(["a"]);
  });

  it("offers only unchosen options matching the query and emits add", () => {
    const { fixture, el, added } = make(["bug"], ["bug", "build", "docs"]);
    type(el, "bu");
    fixture.detectChanges();
    const options = [...el.querySelectorAll<HTMLElement>("[data-testid=chip-option]")];
    expect(options.map((o) => o.textContent!.trim())).toEqual(["build"]);
    options[0].click();
    expect(added).toEqual(["build"]);
  });

  it("takes the first match on Enter and ignores a query without match", () => {
    const { fixture, el, added } = make([], ["docs", "bug"]);
    type(el, "zzz");
    el.querySelector("[data-testid=chip-input]")!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter" }),
    );
    fixture.detectChanges();
    expect(added).toEqual([]);
    type(el, "do");
    el.querySelector("[data-testid=chip-input]")!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter" }),
    );
    expect(added).toEqual(["docs"]);
  });

  it("is read-only when disabled and shows None when empty", () => {
    const { el } = make([], ["a"], true);
    expect(el.querySelector("[data-testid=chip-input]")).toBeNull();
    expect(el.textContent).toContain("None");
  });

  it("moves through the options with the arrow keys and back to the field", () => {
    const { fixture, el } = make([], ["bug", "build", "docs"]);
    type(el, "bu");
    fixture.detectChanges();
    const input = el.querySelector<HTMLElement>("[data-testid=chip-input]")!;
    const key = (target: HTMLElement, name: string) =>
      target.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true }));
    const options = [...el.querySelectorAll<HTMLElement>("[data-testid=chip-option]")];
    key(input, "ArrowDown");
    expect(document.activeElement).toBe(options[0]);
    key(options[0], "ArrowDown");
    expect(document.activeElement).toBe(options[1]);
    key(options[1], "ArrowUp");
    key(options[0], "ArrowUp");
    expect(document.activeElement).toBe(input);
  });
});

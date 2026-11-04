// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";
import { Keymap } from "../core/keymap";
import { DEFAULT_SHORTCUTS, formatShortcut, type Shortcuts } from "../core/shortcuts";
import { ShortcutsOverview } from "../core/shortcuts-overview";
import { ShortcutsDialog } from "./shortcuts-dialog";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
  // jsdom has no modal dialog support: model open/close.
  const proto = HTMLDialogElement.prototype;
  proto.showModal = function (this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  proto.close = function (this: HTMLDialogElement) {
    this.removeAttribute("open");
    this.dispatchEvent(new Event("close"));
  };
});

describe("ShortcutsDialog", () => {
  const shortcuts = signal<Shortcuts>({ ...DEFAULT_SHORTCUTS });

  beforeEach(() => {
    shortcuts.set({ ...DEFAULT_SHORTCUTS });
    TestBed.resetTestingModule();
    TestBed.overrideComponent(ShortcutsDialog, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        template: readFileSync("src/app/shortcuts-dialog/shortcuts-dialog.html", "utf8"),
      },
    });
    TestBed.configureTestingModule({
      providers: [
        {
          provide: Keymap,
          useValue: {
            shortcuts,
            mac: false,
            label: (a: keyof Shortcuts) => formatShortcut(shortcuts()[a], false),
          },
        },
      ],
    });
  });

  const mount = async () => {
    const fixture = TestBed.createComponent(ShortcutsDialog);
    fixture.detectChanges();
    await fixture.whenStable();
    const root = fixture.nativeElement as HTMLElement;
    return {
      fixture,
      root,
      dialog: root.querySelector("dialog")!,
      overview: TestBed.inject(ShortcutsOverview),
    };
  };

  it("is a labelled dialog that stays closed until opened", async () => {
    const { dialog } = await mount();
    expect(dialog.hasAttribute("open")).toBe(false);
    expect(dialog.getAttribute("aria-labelledby")).toBe("shortcuts-title");
    expect(dialog.querySelector("#shortcuts-title")?.textContent).toBe("Keyboard shortcuts");
  });

  it("opens modally, lists groups with the palette first, and closes via the button", async () => {
    const { fixture, root, dialog, overview } = await mount();
    overview.open();
    fixture.detectChanges();
    await fixture.whenStable();
    expect(dialog.hasAttribute("open")).toBe(true);
    const headings = [...root.querySelectorAll("h3")].map((h) => h.textContent);
    expect(headings).toEqual(["General", "Tabs and panes", "View", "Agents"]);
    expect(root.querySelector("li")?.textContent).toContain("Command Palette");
    expect(root.querySelector("kbd")?.textContent).toBe("Ctrl+Shift+P");
    expect(root.querySelector(".hint")?.textContent).toContain("Ctrl+Shift+P");

    root.querySelector<HTMLButtonElement>("button.close")!.click();
    fixture.detectChanges();
    await fixture.whenStable();
    expect(overview.isOpen()).toBe(false);
    expect(dialog.hasAttribute("open")).toBe(false);
  });

  it("closes on a backdrop click but not on a click inside", async () => {
    const { fixture, root, dialog, overview } = await mount();
    overview.open();
    fixture.detectChanges();
    await fixture.whenStable();
    root.querySelector<HTMLElement>("h2")!.click();
    expect(overview.isOpen()).toBe(true);
    dialog.click();
    expect(overview.isOpen()).toBe(false);
  });

  it("syncs the state when the dialog closes itself (Escape)", async () => {
    const { fixture, dialog, overview } = await mount();
    overview.open();
    fixture.detectChanges();
    await fixture.whenStable();
    dialog.dispatchEvent(new Event("close"));
    expect(overview.isOpen()).toBe(false);
  });

  it("shows rebound keys and 'not bound' for an unbound action", async () => {
    shortcuts.set({ ...DEFAULT_SHORTCUTS, closeTab: "Ctrl+F4", zoomIn: "" });
    const { fixture, root, overview } = await mount();
    overview.open();
    fixture.detectChanges();
    await fixture.whenStable();
    const row = (title: string) =>
      [...root.querySelectorAll("li")].find((li) => li.textContent?.includes(title))!;
    expect(row("Close Tab").querySelector("kbd")?.textContent).toBe("Ctrl+F4");
    expect(row("Zoom In").textContent).toContain("not bound");
    expect(row("Zoom In").querySelector("kbd")).toBeNull();
  });
});

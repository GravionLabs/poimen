// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import type { SpaceLayout } from "../core/space-layout";
import { tauri } from "../core/tauri.mock";
import { SpaceLayoutEditor } from "./space-layout-editor";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

const DEFAULT_LAYOUT = {
  panes: [
    { name: "agent", command: "claude", split: null },
    { name: "tests", command: null, split: { from: "agent", direction: "right", ratio: 0.6 } },
    { name: "shell", command: null, split: { from: "tests", direction: "down", ratio: 0.5 } },
  ],
};

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

afterEach(() => TestBed.resetTestingModule());

async function setup(spacesLayout: SpaceLayout | null = null) {
  tauri.reset();
  tauri.answer("default_spaces_layout", () => DEFAULT_LAYOUT);
  TestBed.resetTestingModule();
  TestBed.overrideComponent(SpaceLayoutEditor, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/settings-view/space-layout-editor.html", "utf8"),
    },
  });
  const store = TestBed.inject(SettingsStore);
  store["_settings"].set({ ...DEFAULT_SETTINGS, spacesLayout });
  const fixture = TestBed.createComponent(SpaceLayoutEditor);
  fixture.detectChanges();
  // The built-in default arrives through an async backend call.
  await new Promise((resolve) => setTimeout(resolve));
  fixture.detectChanges();
  const host = fixture.nativeElement as HTMLElement;
  const button = (text: string) =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent?.trim() === text || b.getAttribute("aria-label") === text,
    )!;
  const type = (input: HTMLInputElement, value: string) => {
    input.value = value;
    input.dispatchEvent(new Event("input"));
    fixture.detectChanges();
  };
  return { fixture, host, store, button, type };
}

describe("SpaceLayoutEditor", () => {
  beforeEach(() => vi.spyOn(console, "warn").mockImplementation(() => undefined));

  it("shows the built-in default marked as default", async () => {
    const { host, button } = await setup();
    expect(host.querySelector(".badge")?.textContent).toContain("Default");
    expect(host.querySelectorAll(".pane-row")).toHaveLength(3);
    expect(host.querySelector(".root-note")).not.toBeNull();
    expect(button("Save").disabled).toBe(true);
    expect(button("Reset to default").disabled).toBe(true);
    expect(host.querySelector(".hint")?.textContent).toContain(".agents/herdr-layout.json");
  });

  it("shows the configured layout", async () => {
    const { host } = await setup({ panes: [{ name: "solo", command: "nvim", split: null }] });
    expect(host.querySelector(".badge")).toBeNull();
    expect(host.querySelectorAll(".pane-row")).toHaveLength(1);
    expect(host.querySelector<HTMLInputElement>(".pane-row input")!.value).toBe("solo");
  });

  it("edits, validates inline and saves only a valid layout", async () => {
    const { fixture, host, button, type } = await setup();
    const nameOf = (row: number) =>
      host.querySelectorAll(".pane-row")[row].querySelector<HTMLInputElement>("input")!;
    type(nameOf(2), "agent");
    expect(host.querySelector(".field-error")?.textContent).toContain("Duplicate");
    expect(button("Save").disabled).toBe(true);
    type(nameOf(2), "shell");
    expect(host.querySelector(".field-error")).toBeNull();
    type(nameOf(1), "build");
    const shellFrom = host.querySelectorAll(".pane-row")[2].querySelector("select")!;
    expect(shellFrom.value).toBe("build");
    button("Remove pane shell").click();
    fixture.detectChanges();
    button("Save").click();
    await fixture.whenStable();
    expect(tauri.calls("set_spaces_layout")).toEqual([
      {
        layout: {
          panes: [
            { name: "agent", command: "claude" },
            { name: "build", split: { from: "agent", direction: "right", ratio: 0.6 } },
          ],
        },
      },
    ]);
  });

  it("adds and reorders panes and discards edits", async () => {
    const { fixture, host, button } = await setup();
    button("Add pane").click();
    fixture.detectChanges();
    expect(host.querySelectorAll(".pane-row")).toHaveLength(4);
    button("Move pane tests up").click();
    fixture.detectChanges();
    const first = host.querySelector<HTMLInputElement>(".pane-row input")!;
    expect(first.value).toBe("tests");
    button("Discard").click();
    fixture.detectChanges();
    expect(host.querySelectorAll(".pane-row")).toHaveLength(3);
    expect(button("Save").disabled).toBe(true);
  });

  it("marks a layout equal to the default as the default", async () => {
    const { host, button } = await setup(structuredClone(DEFAULT_LAYOUT) as SpaceLayout);
    expect(host.querySelector(".badge")?.textContent).toContain("Default");
    expect(button("Reset to default").disabled).toBe(true);
  });

  it("resets a configured layout to the default by writing it back", async () => {
    const { fixture, host, button } = await setup({
      panes: [{ name: "solo", command: null, split: null }],
    });
    button("Reset to default").click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(tauri.calls("set_spaces_layout")).toHaveLength(1);
    expect(tauri.calls("remove_spaces_layout")).toHaveLength(0);
    expect(host.querySelectorAll(".pane-row")).toHaveLength(3);
    expect(host.querySelector(".badge")).not.toBeNull();
  });
});

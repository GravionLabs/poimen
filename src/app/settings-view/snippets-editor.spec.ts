// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import type { Snippet } from "../core/snippets-model";
import { tauri } from "../core/tauri.mock";
import { SnippetsEditor } from "./snippets-editor";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

const TESTS: Snippet = { label: "tests", text: "cargo test", enter: true, target: "terminal" };
const DELIVER: Snippet = { label: "deliver", text: "/deliver-loop", enter: true, target: "agent" };

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

afterEach(() => TestBed.resetTestingModule());

function setup(snippets: Snippet[] = [TESTS, DELIVER]) {
  tauri.reset();
  tauri.answer("set_snippets", () => null);
  TestBed.resetTestingModule();
  TestBed.overrideComponent(SnippetsEditor, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      styleUrls: undefined,
      template: readFileSync("src/app/settings-view/snippets-editor.html", "utf8"),
    },
  });
  TestBed.inject(SettingsStore)["_settings"].set({ ...DEFAULT_SETTINGS, snippets });
  const fixture = TestBed.createComponent(SnippetsEditor);
  fixture.detectChanges();
  const host = fixture.nativeElement as HTMLElement;
  const button = (label: string) =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent?.trim() === label || b.getAttribute("aria-label") === label,
    )!;
  const field = <T extends HTMLElement>(name: string) => host.querySelector<T>(`[name="${name}"]`)!;
  const type = (name: string, value: string) => {
    const el = field<HTMLInputElement | HTMLTextAreaElement>(name);
    el.value = value;
    el.dispatchEvent(new Event("input"));
    fixture.detectChanges();
  };
  const settle = async () => {
    await fixture.whenStable();
    fixture.detectChanges();
  };
  return { fixture, host, button, field, type, settle };
}

describe("SnippetsEditor", () => {
  it("lists the snippets with where they go", () => {
    const { host } = setup();
    const rows = [...host.querySelectorAll(".row-item")].map((r) => r.textContent ?? "");
    expect(rows[0]).toContain("tests");
    expect(rows[0]).toContain("cargo test");
    expect(rows[0]).toContain("terminal, with Enter");
    expect(rows[1]).toContain("agent prompt");
  });

  it("adds a snippet and writes only snippets", async () => {
    const { host, button, type, fixture, settle } = setup([]);
    expect(host.textContent).toContain("No snippets yet");
    button("Add snippet").click();
    fixture.detectChanges();
    expect(button("Save snippet").disabled).toBe(true);
    type("snippet-label", " build ");
    type("snippet-text", "cargo build");
    button("Save snippet").click();
    await settle();
    expect(tauri.calls("set_snippets")).toEqual([
      { snippets: [{ label: "build", text: "cargo build", enter: true, target: "terminal" }] },
    ]);
    expect(host.textContent).toContain("build");
  });

  it("edits a snippet: target agent and Enter off", async () => {
    const { button, field, type, fixture, settle } = setup([TESTS]);
    button("Edit tests").click();
    fixture.detectChanges();
    const target = field<HTMLSelectElement>("snippet-target");
    target.value = "agent";
    target.dispatchEvent(new Event("change"));
    fixture.detectChanges();
    expect(field<HTMLInputElement>("snippet-enter").disabled).toBe(true);
    type("snippet-text", "run the tests");
    button("Save snippet").click();
    await settle();
    expect(tauri.calls("set_snippets")).toEqual([
      { snippets: [{ label: "tests", text: "run the tests", enter: true, target: "agent" }] },
    ]);
  });

  it("refuses a label another snippet has", () => {
    const { host, button, type, fixture } = setup();
    button("Add snippet").click();
    fixture.detectChanges();
    type("snippet-label", "tests");
    type("snippet-text", "x");
    expect(host.textContent).toContain('Another snippet is "tests"');
    expect(button("Save snippet").disabled).toBe(true);
  });

  it("removes a snippet", async () => {
    const { button, settle } = setup();
    button("Remove tests").click();
    await settle();
    expect(tauri.calls("set_snippets")).toEqual([{ snippets: [DELIVER] }]);
  });
});

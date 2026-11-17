// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NamedLayout } from "../core/named-layouts";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import type { PaneSpec } from "../core/space-layout";
import type { SpaceTemplate } from "../core/space-templates";
import { tauri } from "../core/tauri.mock";
import { LayoutTabsEditor } from "./layout-tabs-editor";
import { LayoutsEditor } from "./layouts-editor";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

const leaf = (command: string | null = null) =>
  ({ kind: "terminal", name: null, command, cwd: null }) as const;

const REVIEW: NamedLayout = {
  name: "review",
  tabs: [
    {
      name: "agent",
      root: leaf("claude"),
      place: null,
      children: [
        {
          name: "tests",
          root: leaf(),
          children: [],
          place: { direction: "right", of: null, ratio: 0.4 },
        },
      ],
    },
  ],
};
const OTHER: NamedLayout = {
  name: "other",
  tabs: [{ name: "x", root: leaf(), children: [], place: null }],
};

const template = (name: string, layout: SpaceTemplate["layout"]): SpaceTemplate => ({
  name,
  dir: null,
  remote: null,
  agent: "claude",
  model: null,
  layout,
});

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

afterEach(() => TestBed.resetTestingModule());

const PANES: PaneSpec[] = [
  { name: "agent", command: "claude", split: null },
  { name: "shell", command: null, split: { from: "agent", direction: "right", ratio: 0.6 } },
];

async function setup(
  layouts: NamedLayout[] = [REVIEW, OTHER],
  templates: SpaceTemplate[] = [],
  spacesLayout: PaneSpec[] | null = null,
) {
  tauri.reset();
  tauri.answer("set_layouts", () => null);
  TestBed.resetTestingModule();
  TestBed.overrideComponent(LayoutTabsEditor, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      styleUrls: undefined,
      template: readFileSync("src/app/settings-view/layout-tabs-editor.html", "utf8"),
    },
  });
  TestBed.overrideComponent(LayoutsEditor, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      styleUrls: undefined,
      template: readFileSync("src/app/settings-view/layouts-editor.html", "utf8"),
    },
  });
  const store = TestBed.inject(SettingsStore);
  store["_settings"].set({
    ...DEFAULT_SETTINGS,
    layouts,
    spaceTemplates: templates,
    spacesLayout: spacesLayout ? { panes: spacesLayout } : null,
  });
  const fixture = TestBed.createComponent(LayoutsEditor);
  fixture.detectChanges();
  const host = fixture.nativeElement as HTMLElement;
  const button = (label: string) =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent?.trim() === label || b.getAttribute("aria-label") === label,
    )!;
  const type = (input: HTMLInputElement, value: string) => {
    input.value = value;
    input.dispatchEvent(new Event("input"));
    fixture.detectChanges();
  };
  const input = (label: string) => host.querySelector<HTMLInputElement>(`[aria-label="${label}"]`)!;
  const settle = async () => {
    await fixture.whenStable();
    fixture.detectChanges();
  };
  return { fixture, host, store, button, type, input, settle };
}

describe("LayoutsEditor", () => {
  beforeEach(() => vi.spyOn(console, "warn").mockImplementation(() => undefined));

  it("lists the layouts with their tabs", async () => {
    const { host } = await setup([REVIEW], [template("t", "review")]);
    const row = host.querySelector(".row")!;
    expect(row.textContent).toContain("review");
    expect(row.textContent).toContain("agent · tests");
    expect(row.textContent).toContain("used by t");
  });

  it("creates a layout and writes only layouts", async () => {
    const { host, button, fixture, settle } = await setup([]);
    expect(host.textContent).toContain("No named layouts yet");
    button("New layout").click();
    fixture.detectChanges();
    expect(host.querySelectorAll(".pane-row")).toHaveLength(1);
    button("Save layout").click();
    await settle();
    expect(tauri.calls("set_layouts")).toEqual([
      { layouts: [{ name: "layout", tabs: [{ name: "main" }] }] },
    ]);
    expect(tauri.calls("set_space_templates")).toEqual([]);
  });

  it("edits tabs: nest, outdent, reorder and delete with children", async () => {
    const { host, button, fixture, settle } = await setup([REVIEW]);
    button("Edit review").click();
    fixture.detectChanges();
    const names = () =>
      [...host.querySelectorAll<HTMLInputElement>("[data-tab]")].map((i) => i.value);
    expect(names()).toEqual(["agent", "tests"]);
    button("Add child tab to agent").click();
    fixture.detectChanges();
    expect(names()).toEqual(["agent", "tests", "tab"]);
    button("Move tab up").click();
    fixture.detectChanges();
    expect(names()).toEqual(["agent", "tab", "tests"]);
    button("Outdent tab").click();
    fixture.detectChanges();
    expect(names()).toEqual(["agent", "tests", "tab"]);
    expect(button("Outdent tab").disabled).toBe(true);
    expect(button("Indent tab").disabled).toBe(false);
    expect(button("Indent agent").disabled).toBe(true);
    button("Delete agent").click();
    fixture.detectChanges();
    // Deleting a tab takes its children along; the only tab left cannot be deleted.
    expect(names()).toEqual(["tab"]);
    expect(button("Delete tab").disabled).toBe(true);
    button("Save layout").click();
    await settle();
    expect(tauri.calls("set_layouts")).toHaveLength(1);
  });

  it("edits a flat tab, a split tree and a place", async () => {
    const { host, button, fixture, input, type, settle } = await setup([REVIEW]);
    button("Edit review").click();
    fixture.detectChanges();
    type(input("Command in agent"), "codex");
    // The shape select turns a tab into a split tree, whose leaves can be split again.
    const shape = host.querySelectorAll<HTMLSelectElement>('[aria-labelledby="tabs-col-shape"]')[1];
    shape.value = "split";
    shape.dispatchEvent(new Event("change"));
    fixture.detectChanges();
    expect(host.querySelectorAll('[aria-label="Command in tests"]')).toHaveLength(2);
    const ratio = host.querySelector<HTMLInputElement>('[aria-label="Place ratio of tests"]')!;
    type(ratio, "0.7");
    button("Save layout").click();
    await settle();
    const written = tauri.calls("set_layouts")[0] as { layouts: Record<string, unknown>[] };
    expect(written.layouts[0]).toEqual({
      name: "review",
      tabs: [
        {
          name: "agent",
          command: "codex",
          children: [
            {
              name: "tests",
              root: {
                kind: "split",
                direction: "right",
                ratio: 0.5,
                children: [{ kind: "terminal" }, { kind: "terminal" }],
              },
              place: { direction: "right", ratio: 0.7 },
            },
          ],
        },
      ],
    });
  });

  it("picks `of` from the other tabs and renames the references of a renamed tab", async () => {
    const { host, button, fixture, input, type } = await setup([REVIEW]);
    button("Edit review").click();
    fixture.detectChanges();
    button("Add child tab to agent").click();
    fixture.detectChanges();
    const of = host.querySelector<HTMLSelectElement>('[aria-label="Place of for tests"]')!;
    expect([...of.options].map((o) => o.value)).toEqual(["", "agent", "tab"]);
    of.value = "tab";
    of.dispatchEvent(new Event("change"));
    fixture.detectChanges();
    type(host.querySelectorAll<HTMLInputElement>("[data-tab]")[2], "docs");
    const again = host.querySelector<HTMLSelectElement>('[aria-label="Place of for tests"]')!;
    expect(again.value).toBe("docs");
    expect(input("Place ratio of tests").value).toBe("0.4");
  });

  it("previews the spaces tree and the editor groups", async () => {
    const { host, button, fixture } = await setup([REVIEW]);
    button("Edit review").click();
    fixture.detectChanges();
    const rows = [...host.querySelectorAll(".preview-tree li")].map((li) =>
      li.textContent?.replace(/\s+/g, " ").trim(),
    );
    expect(rows).toEqual(["New space", "agent claude", "tests shell"]);
    expect(host.querySelectorAll(".preview-groups .preview-group")).toHaveLength(2);
    expect(host.querySelector(".preview-groups")?.textContent).toContain("tests");
  });

  it("keeps Save disabled and names the problem while the draft is invalid", async () => {
    const { host, button, fixture, type } = await setup([REVIEW, OTHER]);
    button("Edit review").click();
    fixture.detectChanges();
    type(host.querySelector<HTMLInputElement>('input[name="layout-name"]')!, "other");
    expect(host.querySelector(".field-error")?.textContent).toContain(
      'Another layout is called "other"',
    );
    expect(button("Save layout").disabled).toBe(true);
    type(host.querySelectorAll<HTMLInputElement>("[data-tab]")[1], "agent");
    expect(host.textContent).toContain('Another tab is called "agent"');
    expect(tauri.calls("set_layouts")).toEqual([]);
  });

  it("duplicates and renames a layout", async () => {
    const { host, button, fixture, type, settle } = await setup([OTHER]);
    button("Duplicate other").click();
    await settle();
    const dup = tauri.calls("set_layouts")[0] as { layouts: { name: string }[] };
    expect(dup.layouts.map((l) => l.name)).toEqual(["other", "other copy"]);

    tauri.reset();
    tauri.answer("set_layouts", () => null);
    button("Rename other").click();
    fixture.detectChanges();
    type(host.querySelector<HTMLInputElement>('input[name="rename"]')!, " ");
    expect(host.textContent).toContain("A name is required");
    type(host.querySelector<HTMLInputElement>('input[name="rename"]')!, "renamed");
    button("Rename").click();
    await settle();
    const renamed = tauri.calls("set_layouts")[0] as { layouts: { name: string }[] };
    expect(renamed.layouts[0].name).toBe("renamed");
  });

  it("refuses to delete or rename a layout a template uses, naming the templates", async () => {
    const { host, button, fixture, type, settle } = await setup(
      [REVIEW, OTHER],
      [template("one", "review"), template("two", "review"), template("three", null)],
    );
    button("Delete review").click();
    await settle();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain(
      'Cannot delete "review": used by templates one, two.',
    );
    expect(tauri.calls("set_layouts")).toEqual([]);

    button("Rename review").click();
    fixture.detectChanges();
    type(host.querySelector<HTMLInputElement>('input[name="rename"]')!, "look");
    button("Rename").click();
    await settle();
    expect(host.textContent).toContain('Cannot rename "review"');
    expect(tauri.calls("set_layouts")).toEqual([]);
  });

  it("deletes a layout no template uses", async () => {
    const { button, settle } = await setup([REVIEW, OTHER], [template("one", "review")]);
    button("Delete other").click();
    await settle();
    expect(tauri.calls("set_layouts")).toEqual([
      {
        layouts: [
          {
            name: "review",
            tabs: [
              {
                name: "agent",
                command: "claude",
                children: [{ name: "tests", place: { direction: "right", ratio: 0.4 } }],
              },
            ],
          },
        ],
      },
    ]);
  });

  it("shows a refused write and keeps the draft", async () => {
    const { host, button, fixture, settle } = await setup([]);
    tauri.answer("set_layouts", () => {
      throw "layouts: nope";
    });
    button("New layout").click();
    fixture.detectChanges();
    button("Save layout").click();
    await settle();
    expect(host.querySelector(".pane-row")).not.toBeNull();
    expect(fixture.debugElement.injector.get(SettingsStore).writeError()).toContain("nope");
  });

  it("lists spaces.layout as the layout default, marked as the one new spaces get", async () => {
    const { host } = await setup([REVIEW], [], PANES);
    const rows = [...host.querySelectorAll(".row")];
    expect(rows[0].textContent).toContain("default");
    expect(rows[0].textContent).toContain("new spaces get this layout");
    expect(rows[0].textContent).toContain("agent");
    expect(rows[1].textContent).toContain("review");
  });

  it("does not write the virtual default when another layout changes", async () => {
    const { button, settle } = await setup([REVIEW], [], PANES);
    button("Duplicate review").click();
    await settle();
    const [call] = tauri.calls("set_layouts") as { layouts: { name: string }[] }[];
    expect(call.layouts.map((l) => l.name)).toEqual(["review", "review copy"]);
  });

  it("writes a named layout default when the default is edited", async () => {
    const { button, fixture, settle, type, input } = await setup([], [], PANES);
    button("Edit default").click();
    await settle();
    type(input("Command in agent"), "codex");
    button("Save layout").click();
    await settle();
    fixture.detectChanges();
    const [call] = tauri.calls("set_layouts") as { layouts: { name: string; tabs: unknown[] }[] }[];
    expect(call.layouts).toHaveLength(1);
    expect(call.layouts[0].name).toBe("default");
    expect(call.layouts[0].tabs).toHaveLength(1);
  });

  it("refuses to delete the default that comes from spaces.layout", async () => {
    const { host, button, settle } = await setup([], [], PANES);
    button("Delete default").click();
    await settle();
    expect(tauri.calls("set_layouts")).toEqual([]);
    expect(host.textContent).toContain("spaces.layout");
  });
});

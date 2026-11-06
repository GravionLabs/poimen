// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import type { SpaceTemplate } from "../core/space-templates";
import { TauriBridge } from "../core/tauri";
import { tauri } from "../core/tauri.mock";
import { SpaceTemplatesEditor } from "./space-templates-editor";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

const DEFAULT_LAYOUT = {
  panes: [
    { name: "agent", command: "claude", split: null },
    { name: "shell", command: null, split: { from: "agent", direction: "down", ratio: 0.5 } },
  ],
};

const HARNESSES = [
  {
    id: "claude",
    command: ["claude"],
    models: ["opus", "sonnet"],
    defaultModel: null,
    available: true,
  },
  { id: "copilot", command: ["copilot"], models: ["auto"], defaultModel: null, available: false },
];

const WORK: SpaceTemplate = {
  name: "Work",
  dir: "~/src",
  remote: null,
  agent: "claude",
  model: "opus",
  layout: null,
};
const AZ: SpaceTemplate = {
  name: "Azure",
  dir: null,
  remote: { provider: "azure-devops", organization: "acme", project: "Flex" },
  agent: "copilot",
  model: null,
  layout: null,
};

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

afterEach(() => TestBed.resetTestingModule());

async function setup(
  spaceTemplates: SpaceTemplate[] = [],
  extra: Partial<typeof DEFAULT_SETTINGS> = {},
) {
  tauri.reset();
  tauri.answer("default_spaces_layout", () => DEFAULT_LAYOUT);
  tauri.answer("get_harnesses", () => HARNESSES);
  tauri.answer("project_config", () => ({
    config: {
      presets: [
        {
          name: "review",
          layout: { panes: [{ name: "diff", command: "git diff", split: null }] },
        },
      ],
    },
    warnings: [],
  }));
  TestBed.resetTestingModule();
  TestBed.overrideComponent(SpaceTemplatesEditor, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/settings-view/space-templates-editor.html", "utf8"),
    },
  });
  const store = TestBed.inject(SettingsStore);
  store["_settings"].set({ ...DEFAULT_SETTINGS, home: "/home/jk", ...extra, spaceTemplates });
  const fixture = TestBed.createComponent(SpaceTemplatesEditor);
  fixture.detectChanges();
  await new Promise((resolve) => setTimeout(resolve));
  fixture.detectChanges();
  const host = fixture.nativeElement as HTMLElement;
  const button = (text: string) =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent?.trim() === text || b.getAttribute("aria-label") === text,
    )!;
  const press = (text: string) => {
    button(text).click();
    fixture.detectChanges();
  };
  const input = (name: string) => host.querySelector<HTMLInputElement>(`[name=${name}]`)!;
  const select = (name: string) => host.querySelector<HTMLSelectElement>(`select[name=${name}]`)!;
  const type = (el: HTMLInputElement, value: string) => {
    el.value = value;
    el.dispatchEvent(new Event("input"));
    fixture.detectChanges();
  };
  const choose = async (el: HTMLSelectElement, value: string) => {
    el.value = value;
    el.dispatchEvent(new Event("change"));
    await new Promise((resolve) => setTimeout(resolve));
    fixture.detectChanges();
  };
  const saved = async () => {
    press("Save");
    await fixture.whenStable();
    await new Promise((resolve) => setTimeout(resolve));
    fixture.detectChanges();
  };
  return { fixture, host, store, button, press, input, select, type, choose, saved };
}

describe("SpaceTemplatesEditor", () => {
  beforeEach(() => vi.spyOn(console, "warn").mockImplementation(() => undefined));

  it("says so when there are no templates", async () => {
    const { host } = await setup();
    expect(host.querySelector("ul[aria-label='Space templates']")).toBeNull();
    expect(host.textContent).toContain("No templates yet");
  });

  it("lists the templates with name, match and agent", async () => {
    const { host } = await setup([WORK, AZ]);
    const rows = [...host.querySelectorAll("li.row-item")].map((r) =>
      r.textContent?.replace(/\s+/g, " ").trim(),
    );
    expect(rows[0]).toContain("Work");
    expect(rows[0]).toContain("~/src");
    expect(rows[0]).toContain("claude · opus");
    expect(rows[1]).toContain("azure acme/Flex");
    expect(rows[1]).toContain("copilot");
  });

  it("adds a template and writes only spaceTemplates", async () => {
    const { host, button, input, select, type, choose, saved, press } = await setup([WORK]);
    press("Add template");
    await choose(select("agent"), "claude");
    expect(button("Save").disabled).toBe(true);
    type(input("name"), "Flex");
    await choose(select("provider"), "github");
    type(input("owner"), "GravionLabs");
    await choose(select("model"), "sonnet");
    expect(button("Save").disabled).toBe(false);
    await saved();
    expect(tauri.calls("set_space_templates")).toEqual([
      {
        templates: [
          { name: "Work", match: { dir: "~/src" }, agent: "claude", model: "opus" },
          {
            name: "Flex",
            match: { remote: { provider: "github", owner: "GravionLabs" } },
            agent: "claude",
            model: "sonnet",
          },
        ],
      },
    ]);
    expect(host.querySelector("form")).toBeNull();
    expect(host.querySelectorAll("li.row-item")).toHaveLength(2);
  });

  it("edits a template in place", async () => {
    const { input, type, saved, press } = await setup([WORK, AZ]);
    press("Edit Work");
    expect(input("name").value).toBe("Work");
    type(input("name"), "Home");
    type(input("dir"), "/srv/app");
    await saved();
    const [call] = tauri.calls("set_space_templates") as { templates: { name: string }[] }[];
    expect(call.templates.map((t) => t.name)).toEqual(["Home", "Azure"]);
    expect(call.templates[0]).toMatchObject({ match: { dir: "/srv/app" } });
  });

  it("removes a template", async () => {
    const { host, fixture, press } = await setup([WORK, AZ]);
    press("Remove Work");
    await fixture.whenStable();
    fixture.detectChanges();
    expect(tauri.calls("set_space_templates")).toEqual([
      {
        templates: [
          {
            name: "Azure",
            match: { remote: { provider: "azure-devops", organization: "acme", project: "Flex" } },
            agent: "copilot",
          },
        ],
      },
    ]);
    expect(host.querySelectorAll("li.row-item")).toHaveLength(1);
  });

  it("shows the write error and keeps the form when the backend refuses", async () => {
    const { host, input, type, saved, fixture, press } = await setup([WORK]);
    tauri.answer("set_space_templates", () => {
      throw "spaceTemplates: duplicate name";
    });
    press("Edit Work");
    type(input("name"), "Other");
    await saved();
    fixture.detectChanges();
    expect(host.querySelector("[role=alert]")?.textContent).toContain("duplicate name");
    expect(host.querySelector("form")).not.toBeNull();
    expect(host.querySelector("li.row-item")?.textContent).toContain("Work");
  });

  describe("validation", () => {
    it("requires a unique name", async () => {
      const { host, button, input, type, press } = await setup([WORK, AZ]);
      press("Edit Work");
      type(input("name"), "Azure");
      expect(host.querySelector(".field-error")?.textContent).toContain('"Azure"');
      expect(button("Save").disabled).toBe(true);
      type(input("name"), "  ");
      expect(host.querySelector(".field-error")?.textContent).toContain("name is required");
      type(input("name"), "Work");
      expect(button("Save").disabled).toBe(false);
    });

    it("requires a directory or a remote, and an absolute directory", async () => {
      const { host, button, input, type, choose, select, press } = await setup([WORK]);
      press("Edit Work");
      type(input("dir"), "");
      expect(host.textContent).toContain("Set a directory, a remote, or both");
      expect(button("Save").disabled).toBe(true);
      type(input("dir"), "relative/path");
      expect(host.textContent).toContain("absolute directory");
      type(input("dir"), "C:\\src");
      expect(button("Save").disabled).toBe(false);
      await choose(select("provider"), "azure-devops");
      expect(host.textContent).toContain("organization and the project are required");
      expect(button("Save").disabled).toBe(true);
      type(input("organization"), "acme");
      type(input("project"), "Flex");
      expect(button("Save").disabled).toBe(false);
    });

    it("rejects an unknown harness", async () => {
      const { host, button, type, input, press } = await setup([
        { ...WORK, agent: "gone" } as SpaceTemplate,
      ]);
      press("Edit Work");
      type(input("name"), "Work");
      expect(host.textContent).toContain('Unknown harness "gone"');
      expect(button("Save").disabled).toBe(true);
    });
  });

  it("marks unavailable agents and offers the models of the chosen harness", async () => {
    const { host, select, choose, press } = await setup([WORK]);
    press("Edit Work");
    const agents = [...select("agent").options].map((o) => o.textContent?.trim());
    expect(agents).toEqual(["claude", "copilot (not found)"]);
    expect([...select("model").options].map((o) => o.value)).toEqual(["", "opus", "sonnet"]);
    await choose(select("agent"), "copilot");
    expect([...select("model").options].map((o) => o.value)).toEqual(["", "auto"]);
    expect(host.querySelector<HTMLSelectElement>("[name=model]")!.value).toBe("");
  });

  it("fills the directory from the folder picker", async () => {
    const { fixture, input, press } = await setup([WORK]);
    vi.spyOn(TestBed.inject(TauriBridge), "pickDirectory").mockResolvedValue("/picked/dir");
    press("Edit Work");
    press("Browse…");
    await fixture.whenStable();
    await new Promise((resolve) => setTimeout(resolve));
    fixture.detectChanges();
    expect(input("dir").value).toBe("/picked/dir");
  });

  describe("layout", () => {
    it("starts from the built-in default and edits name and command", async () => {
      const { host, select, choose, type, saved, press } = await setup([WORK]);
      press("Edit Work");
      expect(host.querySelector("ol[aria-label='Panes of the template']")).toBeNull();
      await choose(select("layout"), "default");
      const rows = host.querySelectorAll(".pane-row");
      expect(rows).toHaveLength(2);
      type(rows[1].querySelector("input")!, "term");
      type(rows[1].querySelectorAll("input")[1], "htop");
      expect(select("layout").value).toBe("custom");
      await saved();
      const [call] = tauri.calls("set_space_templates") as {
        templates: { layout: unknown }[];
      }[];
      expect(call.templates[0].layout).toEqual({
        panes: [
          { name: "agent", command: "claude" },
          {
            name: "term",
            command: "htop",
            split: { from: "agent", direction: "down", ratio: 0.5 },
          },
        ],
      });
    });

    it("offers spaces.layout and the project presets of the directory", async () => {
      const { fixture, host, select, choose, press } = await setup([WORK], {
        spacesLayout: { panes: [{ name: "mine", command: null, split: null }] },
      });
      press("Edit Work");
      await vi.waitFor(() => expect(tauri.calls("project_config")).toHaveLength(1));
      expect(tauri.calls("project_config")[0]).toEqual({ dir: "/home/jk/src" });
      await new Promise((resolve) => setTimeout(resolve));
      fixture.detectChanges();
      const values = [...select("layout").options].map((o) => o.value);
      expect(values).toEqual(["none", "default", "user", "preset:review"]);
      await choose(select("layout"), "preset:review");
      expect(host.querySelector<HTMLInputElement>(".pane-row input")!.value).toBe("diff");
    });

    it("adds and removes panes, validates them and can go back to none", async () => {
      const { host, button, select, choose, type, press } = await setup([WORK]);
      press("Edit Work");
      await choose(select("layout"), "default");
      press("Add pane");
      expect(host.querySelectorAll(".pane-row")).toHaveLength(3);
      type(host.querySelectorAll(".pane-row")[2].querySelector("input")!, "agent");
      expect(host.textContent).toContain("Duplicate");
      expect(button("Save").disabled).toBe(true);
      press("Remove pane agent");
      expect(button("Save").disabled).toBe(false);
      await choose(select("layout"), "none");
      expect(host.querySelector(".pane-row")).toBeNull();
    });
  });

  it("shows the backend warnings about spaceTemplates only", async () => {
    const { host } = await setup([], {
      warnings: ["spaceTemplates[x].match needs dir or remote; ignored", "tasks: other"],
    });
    const alerts = [...host.querySelectorAll("[role=alert]")].map((a) => a.textContent?.trim());
    expect(alerts).toEqual(["spaceTemplates[x].match needs dir or remote; ignored"]);
  });
});

import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  SettingsTransfer,
  exportFileName,
  hasChanges,
  parseImportPlan,
  planSummary,
  shortValue,
  type ImportPlan,
} from "./settings-transfer";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

const EMPTY: ImportPlan = { added: [], changed: [], removed: [], invalid: [], skipped: [] };

const PLAN = {
  added: [{ path: "ui", after: { fontSize: 14 } }],
  changed: [{ path: "theme", before: "dark", after: "light" }],
  removed: [],
  invalid: [{ path: "sidebar.layout", reason: "sidebar.layout must be tree or split" }],
  skipped: [{ path: "trustedProjects", reason: "per machine, never imported" }],
};

describe("parseImportPlan", () => {
  it("reads the plan of the backend", () => {
    expect(parseImportPlan(PLAN)).toEqual(PLAN);
  });

  it("keeps a change without values and drops malformed entries", () => {
    expect(
      parseImportPlan({
        added: [{ path: "a" }, { nope: 1 }, "x"],
        invalid: [{ path: "b" }, { path: "c", reason: "bad" }],
      }),
    ).toEqual({ ...EMPTY, added: [{ path: "a" }], invalid: [{ path: "c", reason: "bad" }] });
  });

  it("is empty for anything else", () => {
    expect(parseImportPlan(null)).toEqual(EMPTY);
    expect(parseImportPlan("plan")).toEqual(EMPTY);
  });
});

describe("plan helpers", () => {
  it("counts the changes", () => {
    expect(planSummary(parseImportPlan(PLAN))).toBe("1 added, 1 changed, 0 removed, 1 invalid");
    expect(planSummary(EMPTY)).toBe("0 added, 0 changed, 0 removed");
  });

  it("has changes only when something is added, changed or removed", () => {
    expect(hasChanges(parseImportPlan(PLAN))).toBe(true);
    expect(hasChanges({ ...EMPTY, invalid: PLAN.invalid })).toBe(false);
    expect(hasChanges({ ...EMPTY, removed: [{ path: "theme", before: "dark" }] })).toBe(true);
  });

  it("shows values as short JSON", () => {
    expect(shortValue("light")).toBe('"light"');
    expect(shortValue(undefined)).toBe("");
    expect(shortValue({ a: "x".repeat(100) }, 10)).toBe('{"a":"xxx…');
  });

  it("suggests a file name", () => {
    expect(exportFileName(null)).toBe("poimen-settings.json");
    expect(exportFileName("My work/repo")).toBe("poimen-template-My-work-repo.json");
    expect(exportFileName("///")).toBe("poimen-template-template.json");
  });
});

describe("SettingsTransfer", () => {
  let transfer: SettingsTransfer;
  let bridge: TauriBridge;

  beforeEach(() => {
    tauri.reset();
    const injector = Injector.create({ providers: [TauriBridge, SettingsTransfer] });
    bridge = injector.get(TauriBridge);
    transfer = injector.get(SettingsTransfer);
  });

  it("exports nothing when the save dialog is cancelled", async () => {
    vi.spyOn(bridge, "pickSaveFile").mockResolvedValue(null);
    await transfer.export();
    expect(tauri.calls("settings_export")).toEqual([]);
  });

  it("exports the settings or one template to the chosen file", async () => {
    const pick = vi.spyOn(bridge, "pickSaveFile").mockResolvedValue("/tmp/out.json");
    await transfer.export();
    await transfer.export("Work");
    expect(pick.mock.calls).toEqual([["poimen-settings.json"], ["poimen-template-Work.json"]]);
    expect(tauri.calls("settings_export")).toEqual([
      { path: "/tmp/out.json", template: null },
      { path: "/tmp/out.json", template: "Work" },
    ]);
    expect(transfer.message()).toBe("Template exported to /tmp/out.json");
  });

  it("shows the plan of a chosen file and writes nothing before apply", async () => {
    vi.spyOn(bridge, "pickJsonFile").mockResolvedValue("/tmp/in.json");
    tauri.answer("settings_import_plan", () => PLAN);
    await transfer.choose("all");
    expect(tauri.calls("settings_import_plan")).toEqual([
      { path: "/tmp/in.json", mode: "merge", scope: "all" },
    ]);
    expect(transfer.pending()).toEqual({
      path: "/tmp/in.json",
      scope: "all",
      mode: "merge",
      plan: PLAN,
    });
    expect(tauri.calls("settings_import_apply")).toEqual([]);
  });

  it("plans again for replace and applies with that mode", async () => {
    vi.spyOn(bridge, "pickJsonFile").mockResolvedValue("/tmp/in.json");
    tauri.answer("settings_import_plan", () => PLAN);
    tauri.answer("settings_import_apply", () => ({ ...EMPTY, removed: [{ path: "theme" }] }));
    await transfer.choose("all");
    await transfer.setMode("replace");
    expect(tauri.calls("settings_import_plan")).toHaveLength(2);
    expect(transfer.pending()?.mode).toBe("replace");
    await transfer.apply();
    expect(tauri.calls("settings_import_apply")).toEqual([
      { path: "/tmp/in.json", mode: "replace", scope: "all" },
    ]);
    expect(transfer.pending()).toBeNull();
    expect(transfer.message()).toBe("Imported: 0 added, 0 changed, 1 removed");
  });

  it("does nothing when the file dialog is cancelled", async () => {
    vi.spyOn(bridge, "pickJsonFile").mockResolvedValue(null);
    await transfer.choose("template");
    expect(tauri.calls("settings_import_plan")).toEqual([]);
    expect(transfer.pending()).toBeNull();
  });

  it("reports a file that cannot be read and keeps nothing pending", async () => {
    vi.spyOn(bridge, "pickJsonFile").mockResolvedValue("/tmp/in.json");
    tauri.answer("settings_import_plan", () => {
      throw "the file is not valid JSON";
    });
    await transfer.choose("all");
    expect(transfer.error()).toBe("the file is not valid JSON");
    expect(transfer.pending()).toBeNull();
    expect(transfer.busy()).toBe(false);
  });

  it("keeps the import pending when the write fails", async () => {
    vi.spyOn(bridge, "pickJsonFile").mockResolvedValue("/tmp/in.json");
    tauri.answer("settings_import_plan", () => PLAN);
    tauri.answer("settings_import_apply", () => {
      throw "cannot write";
    });
    await transfer.choose("all");
    await transfer.apply();
    expect(transfer.error()).toBe("cannot write");
    expect(transfer.pending()?.path).toBe("/tmp/in.json");
  });
});

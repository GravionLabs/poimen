import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProjectTeardown, teardownSummary, type TeardownReport } from "./project-teardown";
import { ResultDialog } from "./result-dialog";
import { SpaceActions } from "./space-actions";
import { HerdrStore } from "./herdr-store";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";
import { UI_STORAGE, UiState } from "./ui-state";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const ran = (over: Partial<NonNullable<TeardownReport["result"]>>): TeardownReport => ({
  plan: "ran",
  result: { exitCode: 0, timedOut: false, output: "bye", truncated: false, ...over },
});

describe("teardownSummary", () => {
  it("names each outcome", () => {
    expect(teardownSummary({ plan: "none", result: null })).toContain("no teardown");
    expect(teardownSummary({ plan: "untrusted", result: null })).toContain("not trusted");
    expect(teardownSummary(ran({}))).toBe("The teardown finished successfully.");
    expect(teardownSummary(ran({ exitCode: 2 }))).toBe("The teardown failed (exit 2).");
    expect(teardownSummary(ran({ exitCode: null, timedOut: true }))).toContain("timed out");
  });
});

describe("ProjectTeardown", () => {
  let teardown: ProjectTeardown;
  let dialog: ResultDialog;
  let spaces: SpaceActions;

  beforeEach(() => {
    tauri.reset();
    tauri.answer("snapshot", () => ({ spaces: [], tabs: [], terminals: [] }));
    const injector = Injector.create({
      providers: [
        { provide: UI_STORAGE, useValue: null },
        TauriBridge,
        HerdrStore,
        UiState,
        SpaceActions,
        ResultDialog,
        ProjectTeardown,
      ],
    });
    teardown = injector.get(ProjectTeardown);
    dialog = injector.get(ResultDialog);
    spaces = injector.get(SpaceActions);
  });

  it("runs in the directory and shows the output in the dialog", async () => {
    tauri.answer("project_teardown", () => ran({ exitCode: 1, output: "boom" }));
    await teardown.run("/work/app-7");
    expect(tauri.calls("project_teardown")).toEqual([{ dir: "/work/app-7" }]);
    expect(dialog.content()).toMatchObject({
      summary: "The teardown failed (exit 1).",
      text: "boom",
    });
  });

  it("marks truncated output", async () => {
    tauri.answer("project_teardown", () => ran({ output: "x", truncated: true }));
    await teardown.run("/w");
    expect(dialog.content()?.text).toContain("[output truncated at 1 MB]");
  });

  it("an untrusted project shows the reason and no output", async () => {
    tauri.answer("project_teardown", () => ({ plan: "untrusted", result: null }));
    await teardown.run("/w");
    expect(dialog.content()?.text).toBe("");
    expect(dialog.content()?.summary).toContain("not trusted");
  });

  it("an error is a notice", async () => {
    tauri.answer("project_teardown", () => {
      throw new Error("not a repo");
    });
    expect(await teardown.run("/w")).toBeNull();
    expect(spaces.warning()).toContain("not a repo");
    expect(dialog.content()).toBeNull();
  });
});

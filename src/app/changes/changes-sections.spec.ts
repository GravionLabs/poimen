// @vitest-environment jsdom
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { Attention } from "../core/attention";
import type { Terminal } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { tauri } from "../core/tauri.mock";
import { UI_STORAGE } from "../core/ui-state";
import { rangeBetween, sectionsOf, splitPath } from "./change-sections";
import type { ChangedFile, ChangeList } from "./changes";
import { ChangesView } from "./changes-view";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

function file(path: string, over: Partial<ChangedFile> = {}): ChangedFile {
  return {
    path,
    old_path: null,
    status: "modified",
    additions: 1,
    deletions: 0,
    binary: false,
    staged: false,
    conflicted: false,
    ...over,
  };
}

const FILES = [
  file("c.ts", { conflicted: true }),
  file("src/deep/dir/s1.ts", { staged: true }),
  file("s2.ts", { staged: true }),
  file("src/m1.ts"),
  file("m2.ts"),
  file("m3.ts"),
];

describe("sections", () => {
  it("splits into Conflicts, Staged and Changes and leaves empty ones out", () => {
    expect(sectionsOf(FILES).map((s) => [s.id, s.files.length])).toEqual([
      ["conflicts", 1],
      ["staged", 2],
      ["changes", 3],
    ]);
    expect(sectionsOf([file("a")]).map((s) => s.id)).toEqual(["changes"]);
    expect(sectionsOf([])).toEqual([]);
  });

  it("splits a path and picks a range in either direction", () => {
    expect(splitPath("a/b/c.ts")).toEqual({ name: "c.ts", dir: "a/b/" });
    expect(splitPath("c.ts")).toEqual({ name: "c.ts", dir: "" });
    expect(rangeBetween(["a", "b", "c", "d"], "c", "a")).toEqual(["a", "b", "c"]);
    expect(rangeBetween(["a", "b"], "x", "b")).toEqual(["b"]);
  });
});

describe("Changes file list sections", () => {
  let current: ChangedFile[];

  beforeEach(() => {
    tauri.reset();
    current = FILES;
  });
  afterEach(() => TestBed.resetTestingModule());

  async function settle(f: { detectChanges(): void; whenStable(): Promise<unknown> }) {
    for (let i = 0; i < 3; i++) {
      await new Promise((r) => setTimeout(r, 0));
      f.detectChanges();
      await f.whenStable();
    }
  }

  async function render(scope: "base" | "uncommitted" = "uncommitted") {
    tauri.answer("changes_list", (args): ChangeList => ({
      base: "origin/main",
      scope: args?.["scope"] as "base" | "uncommitted",
      from: "abc",
      files: current,
    }));
    tauri.answer("changes_file", () => ({ state: "binary" }));
    TestBed.configureTestingModule({
      providers: [
        { provide: UI_STORAGE, useValue: { getItem: () => null, setItem: () => undefined } },
        { provide: SpacesStore, useValue: { panes: signal(new Map<string, Terminal>()) } },
        { provide: Attention, useValue: new Attention() },
      ],
    });
    const f = TestBed.createComponent(ChangesView);
    f.componentRef.setInput("dir", "/repo");
    f.detectChanges();
    await settle(f);
    if (scope === "uncommitted") {
      pressed(f, "Uncommitted").click();
      await settle(f);
    }
    return f;
  }

  const root = (f: { nativeElement: HTMLElement }) => f.nativeElement;
  const pressed = (f: { nativeElement: HTMLElement }, text: string) =>
    [...root(f).querySelectorAll("button")].find((b) => b.textContent!.trim() === text)!;
  const row = (f: { nativeElement: HTMLElement }, path: string) =>
    root(f).querySelector<HTMLElement>(`.file[data-path="${path}"]`)!;
  const heads = (f: { nativeElement: HTMLElement }) =>
    [...root(f).querySelectorAll(".section-head")].map((h) => ({
      id: h.getAttribute("data-section"),
      count: h.querySelector(".count")!.textContent,
      all: h.querySelector(".all")!.textContent!.trim(),
    }));
  const shiftClick = (e: HTMLElement) =>
    e.dispatchEvent(new MouseEvent("click", { bubbles: true, shiftKey: true }));

  it("shows the sections with counts and Stage all or Unstage all", async () => {
    const f = await render();
    expect(heads(f)).toEqual([
      { id: "conflicts", count: "1", all: "Stage all" },
      { id: "staged", count: "2", all: "Unstage all" },
      { id: "changes", count: "3", all: "Stage all" },
    ]);
  });

  it("hides empty sections", async () => {
    current = [file("a.ts")];
    const f = await render();
    expect(heads(f).map((h) => h.id)).toEqual(["changes"]);
  });

  it("keeps the flat list in the base scope", async () => {
    const f = await render("base");
    expect(root(f).querySelector(".section-head")).toBeNull();
    expect(root(f).querySelectorAll(".file").length).toBe(6);
    expect(root(f).querySelector(".file .stage")).toBeNull();
  });

  it("collapses a section", async () => {
    const f = await render();
    root(f).querySelector<HTMLElement>('[data-section="changes"] .toggle')!.click();
    f.detectChanges();
    expect(root(f).querySelectorAll(".file").length).toBe(3);
    expect(row(f, "m2.ts")).toBeNull();
    expect(heads(f).at(-1)!.count).toBe("3");
  });

  it("stages all of a section in one call", async () => {
    const f = await render();
    root(f).querySelector<HTMLElement>('[data-section="changes"] .all')!.click();
    await settle(f);
    expect(tauri.calls("changes_stage")).toEqual([
      { dir: "/repo", paths: ["src/m1.ts", "m2.ts", "m3.ts"], stage: true },
    ]);
  });

  it("unstages all of the staged section", async () => {
    const f = await render();
    root(f).querySelector<HTMLElement>('[data-section="staged"] .all')!.click();
    await settle(f);
    expect(tauri.calls("changes_stage")[0]).toMatchObject({ stage: false });
  });

  it("shows the name, then the muted directory", async () => {
    const f = await render();
    const r = row(f, "src/deep/dir/s1.ts");
    expect(r.querySelector(".name")!.textContent).toBe("s1.ts");
    expect(r.querySelector(".dir")!.textContent).toBe("src/deep/dir/");
    expect(row(f, "s2.ts").querySelector(".dir")).toBeNull();
    expect(r.querySelector(".status")!.textContent).toBe("M");
    expect(r.querySelector(".stat")!.textContent).toContain("+1");
  });

  it("has Stage or Unstage and Discard on each row", async () => {
    const f = await render();
    expect(row(f, "s2.ts").querySelector(".stage")!.textContent!.trim()).toBe("Unstage");
    expect(row(f, "m2.ts").querySelector(".stage")!.textContent!.trim()).toBe("Stage");
    expect(row(f, "m2.ts").querySelector(".discard")).not.toBeNull();
  });

  it("asks before it discards and names the file", async () => {
    const f = await render();
    row(f, "m2.ts").querySelector<HTMLElement>(".discard")!.click();
    f.detectChanges();
    const dialog = root(f).querySelector("app-dialog-shell")!;
    expect(dialog.textContent).toContain("m2.ts");
    expect(tauri.calls("changes_discard")).toEqual([]);
    (dialog.querySelector(".cancel") as HTMLElement).click();
    f.detectChanges();
    expect(root(f).querySelector("app-dialog-shell")).toBeNull();
    expect(tauri.calls("changes_discard")).toEqual([]);

    row(f, "m2.ts").querySelector<HTMLElement>(".discard")!.click();
    f.detectChanges();
    root(f).querySelector<HTMLElement>(".confirm-discard")!.click();
    await settle(f);
    expect(tauri.calls("changes_discard")).toEqual([{ dir: "/repo", paths: ["m2.ts"] }]);
  });

  it("Shift-click picks a range, the actions apply to it and Escape clears it", async () => {
    const f = await render();
    row(f, "src/m1.ts").click();
    shiftClick(row(f, "m3.ts"));
    f.detectChanges();
    expect(root(f).querySelectorAll(".file.picked").length).toBe(3);

    row(f, "m2.ts").querySelector<HTMLElement>(".stage")!.click();
    await settle(f);
    expect(tauri.calls("changes_stage")).toEqual([
      { dir: "/repo", paths: ["src/m1.ts", "m2.ts", "m3.ts"], stage: true },
    ]);

    row(f, "src/m1.ts").click();
    shiftClick(row(f, "m3.ts"));
    f.detectChanges();
    row(f, "m3.ts").querySelector<HTMLElement>(".discard")!.click();
    f.detectChanges();
    expect(root(f).querySelector("app-dialog-shell")!.textContent).toContain("3 files");
    root(f).querySelector<HTMLElement>(".confirm-discard")!.click();
    await settle(f);
    expect(tauri.calls("changes_discard")).toEqual([
      { dir: "/repo", paths: ["src/m1.ts", "m2.ts", "m3.ts"] },
    ]);

    row(f, "src/m1.ts").click();
    shiftClick(row(f, "m3.ts"));
    f.detectChanges();
    root(f)
      .querySelector(".files")!
      .dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    f.detectChanges();
    expect(root(f).querySelectorAll(".file.picked").length).toBe(0);
  });
});

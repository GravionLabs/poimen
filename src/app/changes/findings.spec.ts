// @vitest-environment jsdom
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { Attention } from "../core/attention";
import { HerdrStore } from "../core/herdr-store";
import { tauri } from "../core/tauri.mock";
import { UI_STATE_KEY, UI_STORAGE, type UiStorage } from "../core/ui-state";
import type { ChangeList, ChangedFile } from "./changes";
import { ChangesView } from "./changes-view";
import {
  countLines,
  findingComment,
  findingLocation,
  lineCounts,
  mapFindings,
  type Finding,
  type FindingsResult,
} from "./findings";

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

function file(path: string, status: ChangedFile["status"] = "modified"): ChangedFile {
  return { path, old_path: null, status, additions: 1, deletions: 0, binary: false };
}

const f = (over: Partial<Finding>): Finding => ({
  severity: "note",
  title: "Title",
  why: "Why.",
  ...over,
});

describe("mapFindings", () => {
  const files = [file("src/a.ts"), file("gone.ts", "deleted"), file("src/b.ts")];

  it("places a finding with a line inside a changed file", () => {
    const counts = lineCounts(files, { path: "src/a.ts", lines: 10 });
    const m = mapFindings([f({ file: "src/a.ts", line: 4, endLine: 6 })], counts);
    expect(m.located.get("src/a.ts")).toEqual([
      { id: "0", finding: expect.objectContaining({ line: 4 }) },
    ]);
    expect(m.other).toEqual([]);
  });

  it("lists a line beyond the file under other findings", () => {
    const counts = lineCounts(files, { path: "src/a.ts", lines: 10 });
    const m = mapFindings([f({ file: "src/a.ts", line: 11 })], counts);
    expect(m.located.size).toBe(0);
    expect(m.other.map((o) => o.id)).toEqual(["0"]);
  });

  it("places a line of a file whose length is not known yet", () => {
    const m = mapFindings([f({ file: "src/b.ts", line: 99 })], lineCounts(files, null));
    expect(m.located.get("src/b.ts")).toHaveLength(1);
  });

  it("lists findings of unchanged files, deleted files and without location as other", () => {
    const m = mapFindings(
      [
        f({ file: "docs/x.md", line: 1 }),
        f({ file: "gone.ts", line: 1 }),
        f({ file: "src/a.ts" }),
        f({}),
      ],
      lineCounts(files, null),
    );
    expect(m.located.size).toBe(0);
    expect(m.other.map((o) => o.id)).toEqual(["0", "1", "2", "3"]);
  });

  it("counts lines without counting a final newline", () => {
    expect(countLines("")).toBe(0);
    expect(countLines("a\nb")).toBe(2);
    expect(countLines("a\nb\n")).toBe(2);
  });
});

describe("finding helpers", () => {
  it("writes the location", () => {
    expect(findingLocation(f({ file: "a.ts", line: 3 }))).toBe("a.ts:3");
    expect(findingLocation(f({ file: "a.ts", line: 3, endLine: 5 }))).toBe("a.ts:3-5");
    expect(findingLocation(f({ file: "a.ts" }))).toBe("a.ts");
    expect(findingLocation(f({}))).toBe("no location");
  });

  it("turns a finding into a pending comment", () => {
    expect(
      findingComment(f({ file: "a.ts", line: 3, endLine: 5, severity: "blocking" }), "x"),
    ).toEqual({
      path: "a.ts",
      side: "new",
      start: 3,
      end: 5,
      quote: "x",
      text: "[blocking] Title\n\nWhy.",
    });
    expect(findingComment(f({})).path).toBe("(general)");
    expect(findingComment(f({ file: "a.ts" }))).toMatchObject({ start: 0, end: 0 });
  });
});

describe("findings in the Changes tab", () => {
  const LIST: ChangeList = {
    base: "origin/release/next",
    scope: "base",
    from: "abc123",
    files: [file("src/a.ts")],
  };
  const FOUND: Finding[] = [
    f({ file: "src/a.ts", line: 1, severity: "blocking", title: "Inside", why: "In the diff." }),
    f({ file: "src/a.ts", line: 40, title: "Beyond", why: "After the end." }),
    f({ file: "docs/x.md", line: 2, title: "Elsewhere", why: "No diff." }),
    f({ title: "General", why: "No location." }),
  ];
  const ok = (stale: boolean): FindingsResult => ({
    state: "ok",
    head: "1111111aaaa",
    stale,
    findings: { version: 1, sha: "2222222bbbb", source: "deliver", findings: FOUND },
  });
  const panes = signal(new Map());
  let data = new Map<string, string>();

  async function render(result: FindingsResult | Error) {
    tauri.answer("changes_list", () => LIST);
    tauri.answer("changes_file", () => ({ state: "text", old: "one", new: "two" }));
    tauri.answer("changes_findings", () => {
      if (result instanceof Error) throw result;
      return result;
    });
    const storage: UiStorage = {
      getItem: (k) => data.get(k) ?? null,
      setItem: (k, v) => void data.set(k, v),
    };
    TestBed.configureTestingModule({
      providers: [
        { provide: UI_STORAGE, useValue: storage },
        { provide: HerdrStore, useValue: { panes } },
        { provide: Attention, useValue: new Attention() },
      ],
    });
    const fixture = TestBed.createComponent(ChangesView);
    fixture.componentRef.setInput("dir", "/repo");
    for (let i = 0; i < 4; i++) {
      fixture.detectChanges();
      await new Promise((r) => setTimeout(r, 0));
      await fixture.whenStable();
    }
    fixture.detectChanges();
    return fixture;
  }

  beforeEach(() => {
    tauri.reset();
    data = new Map();
  });
  afterEach(() => TestBed.resetTestingModule());

  it("marks a finding at its line and lists the rest under other findings", async () => {
    const fx = await render(ok(false));
    const root: HTMLElement = fx.nativeElement;
    const box = root.querySelector(".cm-finding-box")!;
    expect(box.textContent).toContain("blocking · Line 1");
    expect(box.textContent).toContain("Inside");
    expect(box.textContent).toContain("In the diff.");
    expect(root.querySelectorAll(".cm-finding-box")).toHaveLength(1);
    const other = [...root.querySelectorAll("app-findings-panel li .title")].map(
      (n) => n.textContent,
    );
    expect(other).toEqual(["Beyond", "Elsewhere", "General"]);
    expect(root.querySelector(".findings-note")).toBeNull();
    expect(tauri.calls("changes_findings")).toContainEqual({ dir: "/repo" });
  });

  it("sends a finding to the pending comments", async () => {
    const fx = await render(ok(false));
    const root: HTMLElement = fx.nativeElement;
    root.querySelector<HTMLElement>(".cm-finding-send")!.click();
    fx.detectChanges();
    const stored = JSON.parse(data.get(UI_STATE_KEY)!);
    const pending = stored.lineComments["/repo"].pending;
    expect(pending).toHaveLength(1);
    expect(pending[0]).toMatchObject({
      path: "src/a.ts",
      start: 1,
      end: 1,
      text: "[blocking] Inside\n\nIn the diff.",
    });
    expect(root.querySelector<HTMLButtonElement>(".cm-finding-send")!.disabled).toBe(true);

    root.querySelector<HTMLElement>("app-findings-panel li button")!.click();
    fx.detectChanges();
    const next = JSON.parse(data.get(UI_STATE_KEY)!).lineComments["/repo"].pending;
    expect(next).toHaveLength(2);
    expect(next[1]).toMatchObject({ path: "src/a.ts", start: 40, text: expect.any(String) });
  });

  it("notes stale findings", async () => {
    const fx = await render(ok(true));
    const note = (fx.nativeElement as HTMLElement).querySelector(".findings-note.stale")!;
    expect(note.textContent).toContain("2222222");
    expect(note.textContent).toContain("1111111");
  });

  it("shows a note for an invalid file and still shows the diff", async () => {
    const fx = await render({ state: "invalid", error: "findings[0]: line must be at least 1" });
    const root: HTMLElement = fx.nativeElement;
    expect(root.querySelector(".findings-note.invalid")!.textContent).toContain("line must be");
    expect(root.querySelector(".cm-mergeView")).not.toBeNull();
    expect(root.querySelector("app-findings-panel")).toBeNull();
  });

  it("treats a failed read as an invalid file and keeps the diff", async () => {
    const fx = await render(new Error("boom"));
    const root: HTMLElement = fx.nativeElement;
    expect(root.querySelector(".findings-note.invalid")!.textContent).toContain("boom");
    expect(root.querySelector(".cm-mergeView")).not.toBeNull();
  });

  it("shows nothing without a findings file", async () => {
    const fx = await render({ state: "none" });
    const root: HTMLElement = fx.nativeElement;
    expect(root.querySelector(".findings-note")).toBeNull();
    expect(root.querySelector("app-findings-panel")).toBeNull();
  });
});

// @vitest-environment jsdom
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { goToNextChunk, goToPreviousChunk } from "@codemirror/merge";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { Attention } from "../core/attention";
import type { AgentStatus, Terminal } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { tauri } from "../core/tauri.mock";
import { UI_STATE_KEY, UI_STORAGE, type UiStorage } from "../core/ui-state";
import {
  Changes,
  NARROW_WIDTH,
  effectiveLayout,
  isWithin,
  statusLetter,
  totals,
  type ChangeList,
  type FileContent,
} from "./changes";
import { ChangesView } from "./changes-view";
import { DiffEditor, diffThemeRules } from "./diff-editor";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@codemirror/merge", async (original) => {
  const real = await original<typeof import("@codemirror/merge")>();
  return {
    ...real,
    goToNextChunk: vi.fn(real.goToNextChunk),
    goToPreviousChunk: vi.fn(real.goToPreviousChunk),
  };
});
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

const LIST: ChangeList = {
  base: "origin/release/next",
  scope: "base",
  from: "abc123",
  files: [
    {
      path: "src/a.ts",
      old_path: null,
      status: "modified",
      additions: 3,
      deletions: 1,
      binary: false,
    },
    { path: "img.png", old_path: null, status: "added", additions: 0, deletions: 0, binary: true },
    {
      path: "big.txt",
      old_path: null,
      status: "modified",
      additions: 9,
      deletions: 9,
      binary: false,
    },
  ],
};

const CONTENT: Record<string, FileContent> = {
  "src/a.ts": { state: "text", old: "one", new: "two" },
  "img.png": { state: "binary" },
  "big.txt": { state: "too_large" },
};

const panes = signal(new Map<string, Terminal>());
const attention = new Attention();

function pane(id: string, cwd: string, status: AgentStatus): Terminal {
  return { terminal_id: id, tab_id: "t", space_id: "s", cwd, agent_status: status };
}

function memory(): UiStorage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => void data.set(k, v),
  };
}

let storage = memory();

async function render(dir = "/repo") {
  tauri.answer("changes_list", (args) => ({ ...LIST, scope: args?.["scope"] }));
  tauri.answer("changes_file", (args) => CONTENT[args?.["path"] as string]);
  TestBed.configureTestingModule({
    providers: [
      { provide: UI_STORAGE, useValue: storage },
      { provide: HerdrStore, useValue: { panes } },
      { provide: Attention, useValue: attention },
    ],
  });
  const fixture = TestBed.createComponent(ChangesView);
  fixture.componentRef.setInput("dir", dir);
  fixture.detectChanges();
  await settle(fixture);
  fixture.detectChanges();
  return fixture;
}

/** Lets the pending invokes and the deferred diff finish, then renders. */
async function settle(f: { detectChanges(): void; whenStable(): Promise<unknown> }): Promise<void> {
  for (let i = 0; i < 3; i++) {
    await new Promise((r) => setTimeout(r, 0));
    f.detectChanges();
    await f.whenStable();
  }
}

const el = (f: { nativeElement: HTMLElement }) => f.nativeElement;
const names = (f: { nativeElement: HTMLElement }) =>
  [...el(f).querySelectorAll(".file .path")].map((n) => n.textContent);
const click = (e: Element | null) => (e as HTMLElement).click();
const button = (f: { nativeElement: HTMLElement }, text: string) =>
  [...el(f).querySelectorAll("button")].find((b) => b.textContent!.trim() === text)!;
const listCalls = () => tauri.calls("changes_list");

beforeEach(() => {
  tauri.reset();
  storage = memory();
  vi.mocked(goToNextChunk).mockClear();
  vi.mocked(goToPreviousChunk).mockClear();
  panes.set(new Map());
});

afterEach(() => TestBed.resetTestingModule());

describe("Changes tab", () => {
  it("lists the changed files, selects the first and shows its diff", async () => {
    const f = await render();
    expect(names(f)).toEqual(["src/a.ts", "img.png", "big.txt"]);
    expect(el(f).querySelector(".ref")!.textContent).toBe("origin/release/next");
    expect(el(f).querySelector(".totals .add")!.textContent).toBe("+12");
    expect(el(f).querySelector(".totals .del")!.textContent).toBe("−10");
    expect(el(f).querySelector(".totals .count")!.textContent).toBe("3 files");
    expect(el(f).querySelector(".file.selected .path")!.textContent).toBe("src/a.ts");
    expect(el(f).querySelector(".cm-mergeView")).not.toBeNull();
    expect(el(f).querySelector(".cm-mergeViewEditors")!.textContent).toContain("one");
    expect(el(f).querySelector(".cm-mergeViewEditors")!.textContent).toContain("two");
    expect(tauri.calls("changes_file")).toEqual([
      { dir: "/repo", scope: "base", path: "src/a.ts" },
    ]);
  });

  it("selects the clicked file and moves with Up and Down", async () => {
    const f = await render();
    click(el(f).querySelectorAll(".file")[1]);
    await settle(f);
    f.detectChanges();
    expect(el(f).querySelector(".file.selected .path")!.textContent).toBe("img.png");

    const list = el(f).querySelector(".files")!;
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await settle(f);
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await settle(f);
    list.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
    await settle(f);
    f.detectChanges();
    expect(el(f).querySelector(".file.selected .path")!.textContent).toBe("img.png");
  });

  it("reads the other scope when the toggle is used", async () => {
    const f = await render();
    click(button(f, "Uncommitted"));
    await settle(f);
    expect(listCalls().at(-1)).toEqual({ dir: "/repo", scope: "uncommitted" });
    f.detectChanges();
    expect(button(f, "Uncommitted").getAttribute("aria-pressed")).toBe("true");
    expect(button(f, "Base").getAttribute("aria-pressed")).toBe("false");
  });

  it("shows a note for a binary and a too large file instead of a diff", async () => {
    const f = await render();
    click(el(f).querySelectorAll(".file")[1]);
    await settle(f);
    f.detectChanges();
    expect(el(f).querySelector(".note")!.textContent).toContain("Binary");
    expect(el(f).querySelector(".cm-editor")).toBeNull();

    click(el(f).querySelectorAll(".file")[2]);
    await settle(f);
    f.detectChanges();
    expect(el(f).querySelector(".note")!.textContent).toContain("too large");
    expect(el(f).querySelector(".cm-editor")).toBeNull();
  });

  it("reads again on the refresh button", async () => {
    const f = await render();
    const before = listCalls().length;
    click(button(f, "Refresh"));
    await settle(f);
    expect(listCalls().length).toBe(before + 1);
  });

  it("reads again when an agent of its directory turns done, not for another directory", async () => {
    const f = await render("/repo");
    const before = listCalls().length;
    const status = (cwd: string, s: AgentStatus) => {
      const map = new Map([["p1", pane("p1", cwd, s)]]);
      panes.set(map);
      attention.observe(new Map([["p1", s]]), new Set());
    };
    status("/repo/sub", "working");
    status("/repo/sub", "done");
    f.detectChanges();
    await settle(f);
    expect(listCalls().length).toBe(before + 1);

    status("/elsewhere", "working");
    status("/elsewhere", "done");
    f.detectChanges();
    await settle(f);
    expect(listCalls().length).toBe(before + 1);
  });

  it("does not poll", async () => {
    const timer = vi.spyOn(globalThis, "setInterval");
    try {
      await render();
      // Only the editor's frame timers (16 ms) run; nothing that would poll git.
      expect(timer.mock.calls.filter(([, ms]) => (ms ?? 0) >= 100)).toEqual([]);
    } finally {
      timer.mockRestore();
    }
  });

  it("jumps to the next and previous change with n and p", async () => {
    const f = await render();
    el(f).dispatchEvent(new KeyboardEvent("keydown", { key: "n", bubbles: true }));
    f.detectChanges();
    el(f).dispatchEvent(new KeyboardEvent("keydown", { key: "p", bubbles: true }));
    f.detectChanges();
    el(f).dispatchEvent(new KeyboardEvent("keydown", { key: "n", ctrlKey: true, bubbles: true }));
    f.detectChanges();
    expect(goToNextChunk).toHaveBeenCalledTimes(1);
    expect(goToPreviousChunk).toHaveBeenCalledTimes(1);
  });

  it("remembers the inline layout in the UI state", async () => {
    const f = await render();
    click(button(f, "Inline"));
    f.detectChanges();
    expect(el(f).querySelector(".cm-mergeView")).toBeNull();
    expect(el(f).querySelector(".cm-editor")).not.toBeNull();
    expect(JSON.parse(storage.data.get(UI_STATE_KEY)!).changesLayout).toBe("inline");
  });

  it("shows the error of a failed read", async () => {
    tauri.answer("changes_list", () => {
      throw "/x is not in a git repository";
    });
    TestBed.configureTestingModule({
      providers: [
        { provide: UI_STORAGE, useValue: storage },
        { provide: HerdrStore, useValue: { panes } },
        { provide: Attention, useValue: attention },
      ],
    });
    const f = TestBed.createComponent(ChangesView);
    f.componentRef.setInput("dir", "/x");
    f.detectChanges();
    await settle(f);
    f.detectChanges();
    expect(el(f).querySelector(".problem")!.textContent).toContain("not in a git repository");
  });

  it("says so when nothing changed", async () => {
    const f = await render();
    tauri.answer("changes_list", () => ({ ...LIST, files: [] }));
    click(button(f, "Refresh"));
    await settle(f);
    f.detectChanges();
    expect(el(f).querySelector(".empty")!.textContent).toContain("No changes");
  });
});

describe("Changes service", () => {
  it("keeps the selection across a refresh while the file is still changed", async () => {
    tauri.answer("changes_list", () => LIST);
    tauri.answer("changes_file", (args) => CONTENT[args?.["path"] as string]);
    TestBed.configureTestingModule({
      providers: [
        { provide: HerdrStore, useValue: { panes } },
        { provide: Attention, useValue: attention },
        Changes,
      ],
    });
    const changes = TestBed.inject(Changes);
    await changes.follow("/repo");
    await changes.select("big.txt");
    await changes.refresh();
    expect(changes.selected()).toBe("big.txt");
    tauri.answer("changes_list", () => ({ ...LIST, files: [LIST.files[0]] }));
    await changes.refresh();
    expect(changes.selected()).toBe("src/a.ts");
  });
});

describe("helpers", () => {
  it("falls back to inline under 900 px", () => {
    expect(effectiveLayout("split", NARROW_WIDTH - 1)).toBe("inline");
    expect(effectiveLayout("split", NARROW_WIDTH)).toBe("split");
    expect(effectiveLayout("inline", 2000)).toBe("inline");
  });

  it("marks, totals and containment", () => {
    expect(
      ["added", "deleted", "renamed", "untracked", "modified"].map((s) => statusLetter(s as never)),
    ).toEqual(["A", "D", "R", "U", "M"]);
    expect(totals(LIST.files)).toEqual({ files: 3, additions: 12, deletions: 10 });
    expect(isWithin("/a/b", "/a")).toBe(true);
    expect(isWithin("/ab", "/a")).toBe(false);
    expect(isWithin("/a", "/a/")).toBe(true);
  });

  it("themes the diff with the color tokens, green for added and red for removed", () => {
    const rules = diffThemeRules();
    expect(rules["&"]["backgroundColor"]).toBe("var(--term-bg)");
    expect(rules["&"]["color"]).toBe("var(--term-fg)");
    expect(JSON.stringify(rules[".cm-insertedLine, .cm-changedLine"])).toContain("--term-ansi-2");
    expect(JSON.stringify(rules[".cm-deletedLine, .cm-deletedChunk"])).toContain("--term-ansi-1");
  });

  it("exports the editor component", () => {
    expect(DiffEditor).toBeDefined();
  });
});

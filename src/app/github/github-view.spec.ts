// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { ACTIVITY_VIEWS } from "../activity-bar/activity-bar";
import { POIMEN_COMMANDS } from "../core/command-palette";
import { EditorTabs, GITHUB_TAB, tabKind } from "../core/editor-tabs";
import { UI_STORAGE } from "../core/ui-state";
import { tauri } from "../core/tauri.mock";
import { GithubView } from "./github-view";
import { IssuesSection } from "./issues-section";
import { MilestonesSection } from "./milestones-section";
import {
  GithubPulls,
  ageText,
  errorText,
  filterPulls,
  flattenPulls,
  keyRange,
  reviewText,
  worktreeOf,
  type Pull,
  type PullsResult,
} from "./pulls";

const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-opener", () => opener);
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

function pull(number: number, over: Partial<Pull> = {}): Pull {
  return {
    number,
    title: `title ${number}`,
    url: `https://github.com/o/n/pull/${number}`,
    author: "jk",
    isDraft: false,
    additions: 10,
    deletions: 2,
    createdAt: "2026-11-01T00:00:00Z",
    headRef: "b",
    baseRef: "main",
    reviewDecision: null,
    reviewRequests: [],
    checks: { state: "none", passed: 0, failed: 0, pending: 0 },
    ...over,
  };
}

const RESULT: PullsResult = {
  viewer: "jk",
  repos: [
    {
      root: "/src/n",
      repo: "o/n",
      status: "ok",
      pulls: [
        pull(1, { title: "Fix parser", author: "other", reviewRequests: ["jk"] }),
        pull(2, {
          title: "Add view",
          reviewDecision: "APPROVED",
          checks: { state: "failing", passed: 3, failed: 1, pending: 0 },
        }),
        pull(3, { title: "Draft idea", author: "other", isDraft: true }),
      ],
    },
    { root: "/src/m", repo: "o/m", status: "error", error: { kind: "not-logged-in" } },
    {
      root: "/src/k",
      repo: "o/k",
      status: "ok",
      pulls: [pull(1, { title: "Same number elsewhere", url: "https://evil.example/x" })],
    },
  ],
};

async function render(result: PullsResult = RESULT) {
  tauri.answer("github_pulls", () => result);
  TestBed.overrideComponent(GithubView, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/github/github-view.html", "utf8"),
    },
  });
  TestBed.overrideComponent(IssuesSection, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/github/issues-section.html", "utf8"),
    },
  });
  TestBed.overrideComponent(MilestonesSection, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/github/milestones-section.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(GithubView);
  await fixture.whenStable();
  fixture.detectChanges();
  return fixture;
}

const rows = (el: HTMLElement) => [...el.querySelectorAll("tbody tr")].map((r) => r.textContent!);

describe("pull request helpers", () => {
  const all = flattenPulls(RESULT.repos);

  it("flattens only the repositories that could be read, with unique keys", () => {
    expect(all.map((r) => r.key)).toEqual(["o/n#1", "o/n#2", "o/n#3", "o/k#1"]);
  });

  it("filters by scope for the viewer", () => {
    const f = { viewer: "jk", repo: null, text: "" };
    expect(filterPulls(all, { ...f, scope: "review" }).map((r) => r.key)).toEqual(["o/n#1"]);
    expect(filterPulls(all, { ...f, scope: "mine" }).map((r) => r.key)).toEqual(["o/n#2", "o/k#1"]);
    expect(filterPulls(all, { ...f, scope: "all" })).toHaveLength(4);
  });

  it("shows nothing for review and mine while the viewer is unknown", () => {
    const f = { viewer: null, repo: null, text: "" };
    expect(filterPulls(all, { ...f, scope: "mine" })).toEqual([]);
    expect(filterPulls(all, { ...f, scope: "review" })).toEqual([]);
    expect(filterPulls(all, { ...f, scope: "all" })).toHaveLength(4);
  });

  it("filters by repository and by title or number", () => {
    const f = { scope: "all" as const, viewer: "jk" };
    expect(filterPulls(all, { ...f, repo: "o/k", text: "" })).toHaveLength(1);
    expect(filterPulls(all, { ...f, repo: null, text: "PARSER" }).map((r) => r.number)).toEqual([
      1,
    ]);
    expect(filterPulls(all, { ...f, repo: null, text: "#3" }).map((r) => r.number)).toEqual([3]);
    expect(filterPulls(all, { ...f, repo: null, text: "nothing" })).toEqual([]);
  });

  it("describes review state, errors and age", () => {
    expect(reviewText(pull(1, { isDraft: true }))).toBe("Draft");
    expect(reviewText(pull(1, { reviewDecision: "CHANGES_REQUESTED" }))).toBe("Changes requested");
    expect(reviewText(pull(1, { reviewRequests: ["x"] }))).toBe("Requested");
    expect(reviewText(pull(1))).toBe("None");
    expect(errorText({ kind: "not-logged-in" })).toContain("gh auth login");
    const now = Date.parse("2026-11-04T00:00:00Z");
    expect(ageText("2026-11-03T22:00:00Z", now)).toBe("2h");
    expect(ageText("2026-11-01T00:00:00Z", now)).toBe("3d");
    expect(ageText("2026-09-01T00:00:00Z", now)).toBe("2mo");
    expect(ageText("garbage", now)).toBe("");
  });

  it("builds ranges in either direction", () => {
    const keys = ["a", "b", "c", "d"];
    expect(keyRange(keys, "b", "d")).toEqual(["b", "c", "d"]);
    expect(keyRange(keys, "c", "a")).toEqual(["a", "b", "c"]);
    expect(keyRange(keys, null, "c")).toEqual(["c"]);
    expect(keyRange(keys, "a", "zz")).toEqual([]);
  });
});

describe("GithubView", () => {
  beforeEach(() => {
    tauri.reset();
    opener.openUrl.mockClear();
    TestBed.resetTestingModule();
  });
  afterEach(() => TestBed.resetTestingModule());

  it("asks the backend once and lists the pull requests of Review requested first", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    expect(tauri.calls("github_pulls")).toHaveLength(1);
    expect(tauri.calls("github_pulls")[0]).toEqual({ dirs: expect.any(Array) });
    expect(rows(el)).toHaveLength(1);
    expect(rows(el)[0]).toContain("Fix parser");
  });

  it("shows the error of one repository and still lists the others", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    fixture.componentInstance["pulls"].scope.set("all");
    fixture.detectChanges();
    expect(el.querySelector("[data-testid=repo-error]")!.textContent).toContain("gh auth login");
    expect(el.querySelector("[data-testid=repo-error]")!.textContent).toContain("o/m");
    expect(rows(el)).toHaveLength(4);
  });

  it("shows the columns of a pull request", async () => {
    const fixture = await render();
    const pulls = TestBed.inject(GithubPulls);
    pulls.scope.set("mine");
    fixture.detectChanges();
    const text = rows(fixture.nativeElement)[0];
    expect(text).toContain("o/n");
    expect(text).toContain("#2");
    expect(text).toContain("Add view");
    expect(text).toContain("jk");
    expect(text).toContain("3 / 1 / 0");
    expect(text).toContain("Approved");
    expect(text).toContain("+10");
    expect(text).toContain("-2");
  });

  it("filters by repository and text", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    const pulls = TestBed.inject(GithubPulls);
    pulls.scope.set("all");
    const select = el.querySelector<HTMLSelectElement>("select")!;
    fixture.detectChanges();
    select.value = "o/k";
    select.dispatchEvent(new Event("change"));
    fixture.detectChanges();
    expect(rows(el)).toHaveLength(1);
    select.value = "";
    select.dispatchEvent(new Event("change"));
    const input = el.querySelector<HTMLInputElement>("input[type=search]")!;
    input.value = "draft";
    input.dispatchEvent(new Event("input"));
    fixture.detectChanges();
    expect(rows(el)).toEqual([expect.stringContaining("Draft idea")]);
    expect(rows(el)[0]).toContain("Draft");
  });

  it("selects with the checkbox, with Shift for a range, and with the keyboard", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    const pulls = TestBed.inject(GithubPulls);
    pulls.scope.set("all");
    fixture.detectChanges();
    const boxes = () => [...el.querySelectorAll<HTMLInputElement>("tbody input[type=checkbox]")];

    boxes()[0].click();
    fixture.detectChanges();
    expect([...pulls.selected()]).toEqual(["o/n#1"]);

    boxes()[2].dispatchEvent(new MouseEvent("click", { shiftKey: true, bubbles: true }));
    fixture.detectChanges();
    expect(pulls.selectedPulls().map((r) => r.key)).toEqual(["o/n#1", "o/n#2", "o/n#3"]);

    boxes()[1].click();
    fixture.detectChanges();
    expect(pulls.selectedPulls().map((r) => r.key)).toEqual(["o/n#1", "o/n#3"]);

    pulls.clearSelection();
    const body = el.querySelector("tbody")!;
    const key = (k: string) =>
      body.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
    key("ArrowDown");
    key("ArrowDown");
    key(" ");
    fixture.detectChanges();
    expect([...pulls.selected()]).toEqual(["o/n#2"]);
    key("ArrowUp");
    key("ArrowUp");
    key("ArrowUp");
    key(" ");
    fixture.detectChanges();
    expect([...pulls.selected()].sort()).toEqual(["o/n#1", "o/n#2"]);
    expect(el.querySelector("[data-testid=count]")!.textContent).toContain("2 selected");
  });

  it("drops selected pull requests that disappear on reload", async () => {
    const fixture = await render();
    const pulls = TestBed.inject(GithubPulls);
    pulls.scope.set("all");
    pulls.select("o/n#2");
    pulls.select("o/k#1");
    tauri.answer("github_pulls", () => ({ ...RESULT, repos: [RESULT.repos[0]] }));
    await pulls.load();
    fixture.detectChanges();
    expect([...pulls.selected()]).toEqual(["o/n#2"]);
  });

  it("opens only GitHub links in the browser", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    const pulls = TestBed.inject(GithubPulls);
    pulls.scope.set("all");
    fixture.detectChanges();
    const buttons = [...el.querySelectorAll<HTMLButtonElement>("tbody button")].filter((b) =>
      b.textContent!.includes("Open in browser"),
    );
    expect(buttons[0].textContent).toContain("Open in browser");
    expect(buttons[0].disabled).toBe(false);
    expect(buttons[3].disabled).toBe(true);
    buttons[0].click();
    await fixture.whenStable();
    expect(opener.openUrl).toHaveBeenCalledWith("https://github.com/o/n/pull/1");
    await pulls.openUrl("https://evil.example/x");
    expect(opener.openUrl).toHaveBeenCalledTimes(1);
  });

  it("does not read again within 30 seconds unless asked with the button", async () => {
    const fixture = await render();
    const pulls = TestBed.inject(GithubPulls);
    await pulls.load(false);
    window.dispatchEvent(new Event("focus"));
    await fixture.whenStable();
    expect(tauri.calls("github_pulls")).toHaveLength(1);
    const refresh = fixture.nativeElement.querySelector("header button") as HTMLButtonElement;
    refresh.click();
    await fixture.whenStable();
    expect(tauri.calls("github_pulls")).toHaveLength(2);
  });

  it("reads again on focus after 30 seconds", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      const fixture = await render();
      vi.advanceTimersByTime(31_000);
      window.dispatchEvent(new Event("focus"));
      await fixture.whenStable();
      expect(tauri.calls("github_pulls")).toHaveLength(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("says so when no repository is found and when the command fails", async () => {
    const fixture = await render({ viewer: null, repos: [] });
    expect(
      (fixture.nativeElement as HTMLElement).querySelector("[data-testid=no-repos]"),
    ).not.toBeNull();
    tauri.answer("github_pulls", () => {
      throw new Error("boom");
    });
    await TestBed.inject(GithubPulls).load();
    fixture.detectChanges();
    expect((fixture.nativeElement as HTMLElement).textContent).toContain("boom");
  });
});

describe("GitHub entry points", () => {
  beforeEach(() => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: null }] });
  });

  it("opens one pinned github tab and only activates it afterwards", () => {
    const tabs = TestBed.inject(EditorTabs);
    expect(tabKind(GITHUB_TAB)).toBe("github");
    tabs.open("a", true);
    tabs.openGithub();
    tabs.open("b", true);
    tabs.openGithub();
    expect(tabs.tabs().filter((t) => t.paneId === GITHUB_TAB)).toEqual([
      { paneId: GITHUB_TAB, pinned: true },
    ]);
    expect(tabs.activeId()).toBe(GITHUB_TAB);
  });

  it("is offered by the activity bar (as a tab) and the command palette", () => {
    expect(ACTIVITY_VIEWS.find((v) => v.id === "github")).toMatchObject({
      label: "GitHub",
      tab: "github",
    });
    expect(POIMEN_COMMANDS).toContainEqual({ id: "openGithub", title: "Open GitHub" });
  });
});

describe("pull request worktrees", () => {
  beforeEach(() => {
    tauri.reset();
    TestBed.resetTestingModule();
  });
  afterEach(() => TestBed.resetTestingModule());

  it("finds the worktree of the head branch or of pr/<number>", () => {
    const wts = [
      { path: "/src/n", branch: "main" },
      { path: "/src/n-pr1", branch: "pr/1" },
      { path: "/src/n-b", branch: "feat/b" },
      { path: "/src/n-x", branch: null },
    ];
    expect(worktreeOf(pull(1), wts)?.path).toBe("/src/n-pr1");
    expect(worktreeOf(pull(2, { headRef: "feat/b" }), wts)?.path).toBe("/src/n-b");
    expect(worktreeOf(pull(3), wts)).toBeNull();
  });

  it("shows Open space for a pull request whose head is checked out and focuses its space", async () => {
    tauri.answer("worktree_branches", (args) =>
      args?.["dir"] === "/src/n" ? [{ path: "/src/n-pr1", branch: "pr/1" }] : [],
    );
    tauri.answer("create_space", () => ({ workspace_id: "w1" }));
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector("[data-testid=check-out]")).toBeNull();
    (el.querySelector("[data-testid=open-space]") as HTMLButtonElement).click();
    await fixture.whenStable();
    expect(tauri.calls("create_space")[0]).toMatchObject({ cwd: "/src/n-pr1" });
  });

  it("checks out a pull request without a worktree through pr_worktree", async () => {
    tauri.answer("worktree_branches", () => []);
    tauri.answer("pr_worktree", () => ({ path: "/src/n-pr1", branch: "pr/1", created: true }));
    tauri.answer("create_space", () => ({ workspace_id: "w1" }));
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector("[data-testid=open-space]")).toBeNull();
    (el.querySelector("[data-testid=check-out]") as HTMLButtonElement).click();
    await fixture.whenStable();
    expect(tauri.calls("pr_worktree")[0]).toEqual({ repoDir: "/src/n", nr: 1, headRef: "b" });
    expect(tauri.calls("create_space")[0]).toMatchObject({ cwd: "/src/n-pr1" });
  });
});

// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { ACTIVITY_VIEWS } from "../activity-bar/activity-bar";
import { POIMEN_COMMANDS } from "../core/command-palette";
import { EditorTabs, GITHUB_TAB, tabKind } from "../core/editor-tabs";
import { UI_STORAGE, UiState, type GithubPage } from "../core/ui-state";
import { tauri } from "../core/tauri.mock";
import { GithubView } from "./github-view";
import { IssuesSection } from "./issues-section";
import { IssueDetailPane } from "./issue-detail/issue-detail";
import { PullDetailPane } from "./pull-detail/pull-detail";
import { GithubIssues } from "./issues";
import { MilestonesSection } from "./milestones-section";
import { InboxSection } from "./inbox-section";
import { deriveInbox, inboxCount } from "./inbox";
import {
  GithubPulls,
  ageText,
  errorText,
  filterPulls,
  flattenPulls,
  keyRange,
  reviewBadge,
  checkBadges,
  groupPulls,
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
    isCrossRepository: false,
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

async function render(result: PullsResult = RESULT, page: GithubPage | null = "pulls") {
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
  TestBed.overrideComponent(InboxSection, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/github/inbox-section.html", "utf8"),
    },
  });
  TestBed.overrideComponent(MilestonesSection, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/github/milestones-section.html", "utf8"),
    },
  });
  TestBed.overrideComponent(PullDetailPane, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/github/pull-detail/pull-detail.html", "utf8"),
    },
  });
  TestBed.overrideComponent(IssueDetailPane, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/github/issue-detail/issue-detail.html", "utf8"),
    },
  });
  if (page) TestBed.inject(UiState).selectGithubPage(page);
  const fixture = TestBed.createComponent(GithubView);
  await fixture.whenStable();
  fixture.detectChanges();
  return fixture;
}

const rows = (el: HTMLElement) =>
  [...el.querySelectorAll("tbody tr.row")].map((r) => r.textContent!);

describe("deriveInbox", () => {
  const mk = (n: number, over: Partial<Pull>) => pull(n, over);
  const pulls: PullsResult = {
    viewer: "JK",
    repos: [
      {
        root: "/a",
        repo: "o/a",
        status: "ok",
        pulls: [
          mk(1, { author: "x", reviewRequests: ["jk"] }),
          mk(2, { checks: { state: "failing", passed: 0, failed: 1, pending: 0 } }),
          mk(3, { reviewDecision: "CHANGES_REQUESTED" }),
          mk(4, { checks: { state: "passing", passed: 1, failed: 0, pending: 0 } }),
          mk(5, { author: "x", checks: { state: "failing", passed: 0, failed: 1, pending: 0 } }),
        ],
      },
      { root: "/b", repo: "o/b", status: "error", error: { kind: "offline" } },
    ],
  };
  const issue = (number: number, assignees: string[], state: "OPEN" | "CLOSED" = "OPEN") => ({
    number,
    title: `i${number}`,
    url: "u",
    state,
    labels: [],
    assignees,
    milestone: null,
    parent: null,
    subTotal: 0,
    subCompleted: 0,
    prs: [],
  });
  const issues = [
    {
      root: "/a",
      repo: "o/a",
      status: "ok" as const,
      truncated: false,
      issues: [issue(7, ["jk"]), issue(8, ["x"]), issue(9, ["jk"], "CLOSED")],
    },
  ];

  it("collects the three sections", () => {
    const s = deriveInbox(pulls, issues);
    expect(s.map((x) => [x.id, x.entries.map((e) => e.number)])).toEqual([
      ["review", [1]],
      ["mine", [2, 3]],
      ["issues", [7]],
    ]);
    expect(s[1].entries.map((e) => e.reason)).toEqual(["Checks failed", "Changes requested"]);
    expect(inboxCount(s)).toBe(4);
  });

  it("hides empty sections", () => {
    const s = deriveInbox(pulls, []);
    expect(s.map((x) => x.id)).toEqual(["review", "mine"]);
  });

  it("is empty without a viewer or without data", () => {
    expect(deriveInbox({ ...pulls, viewer: null }, issues)).toEqual([]);
    expect(deriveInbox(null, null)).toEqual([]);
    expect(inboxCount([])).toBe(0);
  });
});

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

  it("maps the review decision to a badge, nothing for none or a draft", () => {
    expect(reviewBadge(pull(1, { reviewDecision: "APPROVED" }))).toEqual({
      label: "Approved",
      tone: "success",
    });
    expect(reviewBadge(pull(1, { reviewDecision: "CHANGES_REQUESTED" }))).toEqual({
      label: "Changes requested",
      tone: "danger",
    });
    expect(reviewBadge(pull(1, { reviewDecision: "REVIEW_REQUIRED" }))).toEqual({
      label: "Review required",
      tone: "warning",
    });
    expect(reviewBadge(pull(1))).toBeNull();
    expect(reviewBadge(pull(1, { reviewRequests: ["x"] }))).toBeNull();
    expect(reviewBadge(pull(1, { isDraft: true, reviewDecision: "APPROVED" }))).toBeNull();
  });

  it("maps checks to icons with counts, leaving out empty states", () => {
    const c = (
      state: "passing" | "failing" | "pending" | "none",
      p: number,
      f: number,
      n: number,
    ) => checkBadges({ state, passed: p, failed: f, pending: n }).map((b) => [b.kind, b.count]);
    expect(c("failing", 8, 0, 2)).toEqual([
      ["passed", 8],
      ["pending", 2],
    ]);
    expect(c("failing", 3, 1, 0)).toEqual([
      ["passed", 3],
      ["failed", 1],
    ]);
    expect(c("none", 0, 0, 0)).toEqual([]);
  });

  it("groups rows by repository in first-seen order", () => {
    expect(groupPulls(all).map((g) => [g.repo, g.rows.length])).toEqual([
      ["o/n", 3],
      ["o/k", 1],
    ]);
    expect(groupPulls([])).toEqual([]);
  });

  it("describes errors and age", () => {
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
    expect(text).toContain("#2");
    expect(text).toContain("Add view");
    expect(text).toContain("jk");
    expect(text).not.toContain("3 / 1 / 0");
    const checks = fixture.nativeElement.querySelector("tr.row [data-testid=checks]");
    expect(
      [...checks.querySelectorAll(".check")].map((c: Element) => c.getAttribute("aria-label")),
    ).toEqual(["3 passed", "1 failed"]);
    expect(checks.getAttribute("title")).toContain("3 passed");
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
    expect(el.querySelector("[data-testid=draft]")!.textContent).toBe("Draft");
  });

  it("groups pull requests under a header per repository instead of a column", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    TestBed.inject(GithubPulls).scope.set("all");
    fixture.detectChanges();
    expect(
      [...el.querySelectorAll("tr.group")].map((g) => g.textContent!.replace(/\s+/g, " ").trim()),
    ).toEqual(["o/n 3", "o/k 1"]);
    expect([...el.querySelectorAll("thead th")].map((h) => h.textContent)).not.toContain(
      "Repository",
    );
    expect(el.querySelector("[data-testid=review]")!.textContent).toBe("Approved");
  });

  it("hides repositories without matches unless Show empty repositories is on", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    const groups = () => [...el.querySelectorAll("tr.group")].length;
    fixture.detectChanges();
    expect(groups()).toBe(1);
    const toggle = el.querySelector<HTMLInputElement>("[data-testid=show-empty]")!;
    expect(toggle.checked).toBe(false);
    toggle.click();
    fixture.detectChanges();
    expect(TestBed.inject(UiState).githubShowEmpty()).toBe(true);
    expect(groups()).toBe(2);
    expect(el.querySelector("tr.empty-group")!.textContent).toContain("No pull requests match");
    toggle.click();
    fixture.detectChanges();
    expect(groups()).toBe(1);
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
    const buttons = [...el.querySelectorAll<HTMLButtonElement>("tbody .icon-btn")];
    expect(buttons[0].title).toBe("Open in browser");
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

  it("does not match a local branch to the head branch of a fork pull request", () => {
    const wts = [{ path: "/src/n", branch: "main" }];
    expect(worktreeOf(pull(4, { headRef: "main", isCrossRepository: true }), wts)).toBeNull();
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

describe("GithubView sub-pages", () => {
  const issueOf = (number: number, milestone: string | null) => ({
    number,
    title: `issue ${number}`,
    url: `https://github.com/o/n/issues/${number}`,
    state: "OPEN" as const,
    labels: [],
    assignees: [],
    milestone,
    parent: null,
    subTotal: 0,
    subCompleted: 0,
    prs: [],
  });
  const ISSUES = [
    {
      root: "/src/n",
      repo: "o/n",
      status: "ok",
      truncated: false,
      issues: [issueOf(10, "1.2.0"), issueOf(11, null)],
    },
    {
      root: "/src/k",
      repo: "o/k",
      status: "ok",
      truncated: false,
      issues: [issueOf(20, null)],
    },
  ];
  const ms = (number: number, title: string) => ({
    number,
    title,
    url: `https://github.com/o/n/milestone/${number}`,
    dueOn: null,
    open: 1,
    closed: 0,
    current: false,
  });
  const MILESTONES = [
    { root: "/src/n", repo: "o/n", status: "ok", milestones: [ms(1, "1.2.0"), ms(2, "1.3.0")] },
    { root: "/src/k", repo: "o/k", status: "ok", milestones: [ms(3, "2.0.0")] },
  ];
  let stored: Record<string, string>;

  beforeEach(() => {
    tauri.reset();
    TestBed.resetTestingModule();
    stored = {};
    TestBed.configureTestingModule({
      providers: [
        {
          provide: UI_STORAGE,
          useValue: {
            getItem: (k: string) => stored[k] ?? null,
            setItem: (k: string, v: string) => (stored[k] = v),
          },
        },
      ],
    });
    tauri.answer("github_issues", () => ISSUES);
    tauri.answer("github_milestones", () => MILESTONES);
  });
  afterEach(() => TestBed.resetTestingModule());

  const navAll = (el: HTMLElement) => [...el.querySelectorAll<HTMLElement>("nav button")];
  const nav = (el: HTMLElement) => navAll(el).filter((b) => b.dataset["page"] !== "inbox");
  const press = (b: HTMLElement, key: string) =>
    b.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));

  it("shows a count per page and marks the active one with aria-current", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    expect(nav(el).map((b) => b.textContent!.replace(/\s+/g, " ").trim())).toEqual([
      "Pull requests 1",
      "Issues 3",
      "Milestones 3",
    ]);
    expect(nav(el).map((b) => b.getAttribute("aria-current"))).toEqual(["page", null, null]);
  });

  it("renders only the selected page and moves with ArrowDown, ArrowUp and Enter", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector("app-github-issues")).toBeNull();
    expect(el.querySelector("app-github-milestones")).toBeNull();
    press(nav(el)[0], "ArrowDown");
    fixture.detectChanges();
    expect(nav(el)[1].getAttribute("aria-current")).toBe("page");
    expect(el.querySelector("app-github-issues")).not.toBeNull();
    expect(el.querySelector("table")).toBeNull();
    press(nav(el)[1], "ArrowDown");
    press(nav(el)[2], "ArrowDown");
    fixture.detectChanges();
    expect(nav(el)[2].getAttribute("aria-current")).toBe("page");
    press(nav(el)[2], "ArrowUp");
    fixture.detectChanges();
    expect(nav(el)[1].getAttribute("aria-current")).toBe("page");
    nav(el)[0].focus();
    press(nav(el)[0], "Enter");
    fixture.detectChanges();
    expect(nav(el)[0].getAttribute("aria-current")).toBe("page");
    expect(el.querySelector("table")).not.toBeNull();
  });

  it("keeps the selected page across a reopen", async () => {
    const first = await render(RESULT, null);
    nav(first.nativeElement)[2].click();
    first.detectChanges();
    expect(JSON.parse(stored["poimen.ui"] ?? Object.values(stored)[0]).githubPage).toBe(
      "milestones",
    );
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        {
          provide: UI_STORAGE,
          useValue: { getItem: (k: string) => stored[k] ?? null, setItem: () => undefined },
        },
      ],
    });
    const second = await render(RESULT, null);
    expect(nav(second.nativeElement)[2].getAttribute("aria-current")).toBe("page");
    expect(second.nativeElement.querySelector("app-github-milestones")).not.toBeNull();
  });

  it("applies the shared repository filter to pull requests, issues and milestones", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    const select = el.querySelector<HTMLSelectElement>("aside select")!;
    expect([...select.options].map((o) => o.value)).toEqual(["", "o/k", "o/m", "o/n"]);
    expect(el.querySelectorAll(".toolbar select")).toHaveLength(0);
    select.value = "o/k";
    select.dispatchEvent(new Event("change"));
    fixture.detectChanges();
    expect(nav(el).map((b) => b.querySelector(".nav-count")!.textContent)).toEqual(["0", "1", "1"]);
    nav(el)[1].click();
    fixture.detectChanges();
    expect(el.querySelectorAll("app-github-issues h3").length).toBe(1);
    expect(el.querySelector("app-github-issues h3")!.textContent).toContain("o/k");
    nav(el)[2].click();
    fixture.detectChanges();
    expect(el.querySelector("app-github-milestones h3")!.textContent).toContain("o/k");
    expect(el.querySelectorAll("app-github-milestones li.item")).toHaveLength(1);
  });

  it("switches to Issues, filtered by the milestone, when a milestone is clicked", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    nav(el)[2].click();
    fixture.detectChanges();
    el.querySelector<HTMLElement>("li[data-title='1.2.0'] button.pick")!.click();
    fixture.detectChanges();
    expect(nav(el)[1].getAttribute("aria-current")).toBe("page");
    expect(TestBed.inject(UiState).githubPage()).toBe("issues");
    expect(el.querySelector("app-github-issues")!.textContent).toContain("issue 10");
    expect(el.querySelector("app-github-issues")!.textContent).not.toContain("issue 11");
  });

  it("has a single Refresh that reloads the active page only", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    nav(el)[1].click();
    fixture.detectChanges();
    expect(
      [...el.querySelectorAll("button")].filter((b) => /Refresh/.test(b.textContent!)),
    ).toHaveLength(1);
    const before = {
      pulls: tauri.calls("github_pulls").length,
      issues: tauri.calls("github_issues").length,
      milestones: tauri.calls("github_milestones").length,
    };
    [...el.querySelectorAll("button")].find((b) => b.textContent!.trim() === "Refresh")!.click();
    await fixture.whenStable();
    expect(tauri.calls("github_issues")).toHaveLength(before.issues + 1);
    expect(tauri.calls("github_pulls")).toHaveLength(before.pulls);
    expect(tauri.calls("github_milestones")).toHaveLength(before.milestones);
  });

  it("starts on the Inbox when nothing is persisted and counts its entries", async () => {
    const fixture = await render(RESULT, null);
    const el: HTMLElement = fixture.nativeElement;
    expect(TestBed.inject(UiState).githubPage()).toBe("inbox");
    expect(navAll(el)[0].getAttribute("aria-current")).toBe("page");
    expect(el.querySelector("[data-testid=count-inbox]")!.textContent).toBe("2");
    expect(el.querySelector("[data-testid=inbox-review]")!.textContent).toContain(
      "Review requested",
    );
    expect(el.querySelector("[data-testid=inbox-mine]")).not.toBeNull();
    expect(el.querySelector("[data-testid=inbox-issues]")).toBeNull();
    expect(stored["poimen.ui-state"]).toBeUndefined();
  });

  it("opens the pull request of an inbox entry on its page with the pane open", async () => {
    const fixture = await render(RESULT, null);
    const el: HTMLElement = fixture.nativeElement;
    el.querySelector<HTMLElement>("app-github-inbox button.pick")!.click();
    fixture.detectChanges();
    expect(TestBed.inject(UiState).githubPage()).toBe("pulls");
    expect(TestBed.inject(GithubPulls).detailKey()).toBe("o/n#1");
  });

  it("opens the issue of an inbox entry on the Issues page", async () => {
    tauri.answer("github_issues", () => [
      {
        root: "/src/n",
        repo: "o/n",
        status: "ok",
        truncated: false,
        issues: [{ ...issueOf(10, null), assignees: ["JK"] }],
      },
    ]);
    const fixture = await render(RESULT, null);
    const el: HTMLElement = fixture.nativeElement;
    const entries = [...el.querySelectorAll<HTMLElement>("app-github-inbox button.pick")];
    expect(entries).toHaveLength(3);
    entries[2].click();
    fixture.detectChanges();
    expect(TestBed.inject(UiState).githubPage()).toBe("issues");
    expect(TestBed.inject(GithubIssues).detailKey()).toBe("o/n#10");
  });

  it("offers the sub-pages in the command palette", () => {
    expect(POIMEN_COMMANDS).toContainEqual({ id: "githubInbox", title: "GitHub: Inbox" });
    expect(POIMEN_COMMANDS).toContainEqual({ id: "githubPulls", title: "GitHub: Pull requests" });
    expect(POIMEN_COMMANDS).toContainEqual({ id: "githubIssues", title: "GitHub: Issues" });
    expect(POIMEN_COMMANDS).toContainEqual({ id: "githubMilestones", title: "GitHub: Milestones" });
  });
});

// @vitest-environment jsdom
import { IssueFormPane } from "./issue-form/issue-form";
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { UI_STATE_KEY, UI_STORAGE, UiState, parseLayout } from "../core/ui-state";
import { tauri } from "../core/tauri.mock";
import {
  GithubIssues,
  bulkSummary,
  NO_ISSUE_FILTER,
  buildTree,
  childLevel,
  formReady,
  levelPrefix,
  newIssueForm,
  parentCandidates,
  filterTree,
  flattenTree,
  issueLevel,
  issueProgress,
  labelChips,
  levelBadge,
  type Issue,
  type RepoIssues,
} from "./issues";
import { IssuesSection } from "./issues-section";
import { SpaceActions } from "../core/space-actions";
import { choose, shown } from "../../testing/select";

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

function issue(number: number, over: Partial<Issue> = {}): Issue {
  return {
    number,
    title: `title ${number}`,
    url: `https://github.com/o/n/issues/${number}`,
    state: "OPEN",
    labels: [],
    assignees: [],
    milestone: null,
    parent: null,
    subTotal: 0,
    subCompleted: 0,
    prs: [],
    ...over,
  };
}
const parent = (number: number, repo = "o/n") => ({ number, repo });
const ids = (nodes: { issue: Issue; children: unknown[] }[]): number[] =>
  nodes.map((n) => n.issue.number);

describe("issueLevel", () => {
  it("takes the level from the label, ahead of the title", () => {
    expect(issueLevel(issue(1, { labels: ["bug", "x"], title: "[Epic] x" }))).toBe("bug");
    expect(issueLevel(issue(1, { labels: ["Epic"] }))).toBe("epic");
    expect(issueLevel(issue(1, { labels: ["feature"] }))).toBe("feature");
    expect(issueLevel(issue(1, { labels: ["pbi"] }))).toBe("pbi");
  });

  it("falls back to the title prefix, else other", () => {
    expect(issueLevel(issue(1, { title: "[Epic] Views" }))).toBe("epic");
    expect(issueLevel(issue(1, { title: "[PBI] Tree" }))).toBe("pbi");
    expect(issueLevel(issue(1, { title: "[bug] crash" }))).toBe("bug");
    expect(issueLevel(issue(1, { title: "Epic without brackets" }))).toBe("other");
    expect(issueLevel(issue(1, { title: "x [Epic]" }))).toBe("other");
  });
});

describe("buildTree", () => {
  it("nests children under their parent, sorted by number", () => {
    const tree = buildTree("o/n", [
      issue(3, { parent: parent(1) }),
      issue(1),
      issue(2, { parent: parent(1) }),
      issue(4, { parent: parent(2) }),
    ]);
    expect(ids(tree)).toEqual([1]);
    expect(ids(tree[0].children as never)).toEqual([2, 3]);
    expect(ids((tree[0].children[0] as never as { children: never[] }).children)).toEqual([4]);
  });

  it("makes issues with a missing (closed) parent roots", () => {
    const tree = buildTree("o/n", [issue(5, { parent: parent(99) }), issue(2)]);
    expect(ids(tree)).toEqual([2, 5]);
  });

  it("makes issues whose parent lives in another repository roots", () => {
    const tree = buildTree("o/n", [issue(1), issue(2, { parent: parent(1, "o/other") })]);
    expect(ids(tree)).toEqual([1, 2]);
  });

  it("ignores a cycle and a self parent: every issue appears once", () => {
    const tree = buildTree("o/n", [
      issue(1, { parent: parent(2) }),
      issue(2, { parent: parent(1) }),
      issue(3, { parent: parent(3) }),
      issue(4, { parent: parent(1) }),
    ]);
    expect(ids(tree)).toEqual([1, 2, 3]);
    expect(flattenTree(tree, () => false).map((r) => r.node.issue.number)).toEqual([1, 4, 2, 3]);
  });

  it("is empty for no issues", () => {
    expect(buildTree("o/n", [])).toEqual([]);
  });
});

describe("filterTree and flattenTree", () => {
  const issues = [
    issue(1, { labels: ["epic"], title: "Views", subTotal: 4, subCompleted: 1 }),
    issue(2, { labels: ["pbi", "needs-refinement"], parent: parent(1), assignees: ["jk"] }),
    issue(3, { labels: ["bug"], parent: parent(1), title: "Crash" }),
    issue(4, { labels: ["pbi"], title: "Loose" }),
  ];
  const tree = buildTree("o/n", issues);
  const rows = (filter: Partial<typeof NO_ISSUE_FILTER>) =>
    flattenTree(filterTree(tree, { ...NO_ISSUE_FILTER, ...filter }), () => false).map(
      (r) => r.node.issue.number,
    );

  it("keeps everything without a filter", () => {
    expect(rows({})).toEqual([1, 2, 3, 4]);
  });

  it("keeps the ancestors of a match", () => {
    expect(rows({ assignee: "JK" })).toEqual([1, 2]);
    expect(rows({ level: "bug" })).toEqual([1, 3]);
    expect(rows({ label: "needs-refinement" })).toEqual([1, 2]);
  });

  it("filters by text on title or number", () => {
    expect(rows({ text: "crash" })).toEqual([1, 3]);
    expect(rows({ text: "#4" })).toEqual([4]);
    expect(rows({ text: "nothing" })).toEqual([]);
  });

  it("filters by milestone title and keeps the ancestors", () => {
    const withMilestone = buildTree("o/n", [
      issue(1, { labels: ["epic"] }),
      issue(2, { parent: parent(1), milestone: "1.2.0" }),
      issue(3, { parent: parent(1), milestone: "1.3.0" }),
      issue(4),
    ]);
    const numbers = (milestone: string | null) =>
      flattenTree(filterTree(withMilestone, { ...NO_ISSUE_FILTER, milestone }), () => false).map(
        (r) => r.node.issue.number,
      );
    expect(numbers("1.2.0")).toEqual([1, 2]);
    expect(numbers("9.9.9")).toEqual([]);
    expect(numbers(null)).toEqual([1, 2, 3, 4]);
  });

  it("leaves out the children of collapsed nodes", () => {
    const flat = flattenTree(tree, (n) => n === 1);
    expect(flat.map((r) => [r.node.issue.number, r.depth, r.collapsed])).toEqual([
      [1, 0, true],
      [4, 0, false],
    ]);
    // A leaf is never collapsed, whatever the stored state says.
    expect(flattenTree(tree, () => true).find((r) => r.node.issue.number === 4)!.collapsed).toBe(
      false,
    );
  });

  it("shows labels as chips without the level label", () => {
    expect(labelChips(issue(1, { labels: ["pbi", "needs-decision", "Epic", "x"] }))).toEqual([
      "needs-decision",
      "x",
    ]);
    expect(labelChips(issue(1))).toEqual([]);
  });

  it("maps every level to a badge", () => {
    expect(levelBadge("epic")).toEqual({ label: "epic", tone: "epic" });
    expect(levelBadge("bug")).toEqual({ label: "bug", tone: "bug" });
    expect(levelBadge("other").tone).toBe("other");
  });

  it("reports sub-issue progress for epics and features only", () => {
    expect(issueProgress("epic", { subTotal: 4, subCompleted: 1 })).toEqual({
      done: 1,
      total: 4,
      percent: 25,
    });
    expect(issueProgress("feature", { subTotal: 3, subCompleted: 3 })?.percent).toBe(100);
    expect(issueProgress("feature", { subTotal: 0, subCompleted: 0 })).toBeNull();
    expect(issueProgress("pbi", { subTotal: 4, subCompleted: 1 })).toBeNull();
    expect(issueProgress("epic", { subTotal: 2, subCompleted: 5 })?.percent).toBe(100);
  });
});

describe("collapsed issues in the UI state", () => {
  it("is parsed per repository and tolerates garbage", () => {
    expect(
      parseLayout(JSON.stringify({ collapsedIssues: { "o/n": [1, "x", 2.5, 3], "o/m": [] } })),
    ).toMatchObject({ collapsedIssues: { "o/n": [1, 3] } });
    expect(parseLayout(JSON.stringify({ collapsedIssues: [1] })).collapsedIssues).toBeUndefined();
    expect(parseLayout(null).collapsedIssues).toBeUndefined();
  });

  it("is remembered per repository across restarts", () => {
    const data: Record<string, string> = {};
    const storage = {
      getItem: (k: string) => data[k] ?? null,
      setItem: (k: string, v: string) => void (data[k] = v),
    };
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: storage }] });
    const ui = TestBed.inject(UiState);
    ui.setIssueCollapsed("o/n", 1, true);
    ui.setIssueCollapsed("o/n", 2, true);
    ui.setIssueCollapsed("o/m", 1, true);
    ui.setIssueCollapsed("o/n", 2, false);
    ui.setIssueCollapsed("o/m", 1, false);
    expect(parseLayout(data[UI_STATE_KEY]).collapsedIssues).toEqual({ "o/n": [1] });
  });
});

const RESULT: RepoIssues[] = [
  {
    root: "/src/n",
    repo: "o/n",
    status: "ok",
    truncated: false,
    issues: [
      issue(1, { labels: ["epic"], title: "Views", subTotal: 4, subCompleted: 1 }),
      issue(2, {
        labels: ["pbi", "needs-refinement", "needs-decision"],
        parent: parent(1),
        assignees: ["jk"],
        prs: [
          {
            number: 40,
            url: "https://github.com/o/n/pull/40",
            state: "OPEN",
            checks: { state: "failing", passed: 3, failed: 1, pending: 0 },
          },
          {
            number: 41,
            url: "https://evil.example/41",
            state: "OPEN",
            checks: { state: "none", passed: 0, failed: 0, pending: 0 },
          },
        ],
      }),
    ],
  },
  { root: "/src/m", repo: "o/m", status: "error", error: { kind: "not-logged-in" } },
];

async function render(result: RepoIssues[] = RESULT) {
  tauri.answer("github_issues", () => result);
  TestBed.overrideComponent(IssueFormPane, {
    set: { templateUrl: undefined, styleUrl: undefined, template: "", imports: [] },
  });
  TestBed.overrideComponent(IssuesSection, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/github/issues-section.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(IssuesSection);
  await fixture.whenStable();
  fixture.detectChanges();
  return fixture;
}

const items = (el: HTMLElement) => [...el.querySelectorAll("li.row")];

describe("IssuesSection", () => {
  beforeEach(() => {
    tauri.reset();
    opener.openUrl.mockClear();
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: null }] });
  });
  afterEach(() => TestBed.resetTestingModule());

  it("reads the open issues once and shows the row details", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    expect(tauri.calls("github_issues")).toEqual([{ dirs: expect.any(Array), closed: false }]);
    expect(items(el)).toHaveLength(2);
    const child = items(el)[1].textContent!;
    expect(
      [...items(el)[1].querySelectorAll("[data-testid=chip]")].map((c) => c.textContent),
    ).toEqual(["needs-refinement", "needs-decision"]);
    expect(items(el)[1].querySelector("[data-testid=level]")!.textContent).toBe("pbi");
    expect(items(el)[1].querySelector("[data-testid=progress]")).toBeNull();
    expect(child).toContain("@jk");
    expect(child).toContain("PR #40");
    expect(child).not.toContain("3 / 1 / 0");
    expect(
      [...items(el)[1].querySelectorAll(".check")].map((c) => c.getAttribute("aria-label")),
    ).toEqual(["3 passed", "1 failed"]);
    const bar = items(el)[0].querySelector("[data-testid=progress]")!;
    expect(bar.textContent).toContain("1/4");
    expect(bar.querySelector("hx-progress-bar")!.getAttribute("aria-valuenow")).toBe("25");
    expect(el.querySelector("[data-testid=issue-error]")!.textContent).toContain("o/m");
  });

  it("collapses and expands a node and remembers it", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    el.querySelector<HTMLButtonElement>("[data-testid=twisty]")!.click();
    fixture.detectChanges();
    expect(items(el)).toHaveLength(1);
    expect(TestBed.inject(UiState).collapsedIssues()).toEqual({ "o/n": [1] });
    el.querySelector<HTMLButtonElement>("[data-testid=twisty]")!.click();
    fixture.detectChanges();
    expect(items(el)).toHaveLength(2);
  });

  it("filters by level, assignee and text", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    const issues = TestBed.inject(GithubIssues);
    issues.level.set("epic");
    fixture.detectChanges();
    expect(items(el)).toHaveLength(1);
    issues.level.set(null);
    issues.assignee.set("nobody");
    fixture.detectChanges();
    expect(items(el)).toHaveLength(0);
    expect(el.querySelector("[data-testid=issues-empty]")).toBeNull();
    TestBed.inject(UiState).setGithubShowEmpty(true);
    fixture.detectChanges();
    expect(el.querySelector("[data-testid=issues-empty]")).not.toBeNull();
    issues.assignee.set(null);
    const input = el.querySelector<HTMLInputElement>("input[type=search]")!;
    input.value = "title 2";
    input.dispatchEvent(new Event("input"));
    fixture.detectChanges();
    expect(items(el).map((i) => i.getAttribute("data-number"))).toEqual(["1", "2"]);
  });

  it("reads closed issues when the state changes", async () => {
    const fixture = await render();
    const state = fixture.nativeElement.querySelector('hx-select[ariaLabel="State"]')!;
    await choose(state, "Closed", () => fixture.detectChanges());
    await fixture.whenStable();
    expect(shown(state)).toBe("Closed");
    expect(tauri.calls("github_issues")[1]).toEqual({ dirs: expect.any(Array), closed: true });
  });

  it("runs a bulk pick once and shows the placeholder again", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    const issues = TestBed.inject(GithubIssues);
    const edit = vi.spyOn(issues, "bulkEdit").mockResolvedValue({ done: [], failed: [] });
    issues.mark("o/n#1");
    issues.bulkLabels.set(["bug"]);
    fixture.detectChanges();
    const add = el.querySelector("[data-testid=bulk-add-label]")!;
    await choose(add, "bug", () => fixture.detectChanges());
    expect(edit.mock.calls).toEqual([[{ addLabels: ["bug"] }]]);
    expect(shown(add)).toBe("Add label");
  });

  it("opens issues and pull requests in the browser, GitHub links only", async () => {
    const fixture = await render();
    const el: HTMLElement = fixture.nativeElement;
    el.querySelector<HTMLButtonElement>("[aria-label='Open issue #2 in browser']")!.click();
    el.querySelector<HTMLButtonElement>("[aria-label='Open pull request #40 in browser']")!.click();
    const evil = el.querySelector<HTMLButtonElement>(
      "[aria-label='Open pull request #41 in browser']",
    )!;
    expect(evil.disabled).toBe(true);
    await TestBed.inject(GithubIssues).openUrl("https://evil.example/x");
    expect(opener.openUrl.mock.calls.map((c) => c[0])).toEqual([
      "https://github.com/o/n/issues/2",
      "https://github.com/o/n/pull/40",
    ]);
  });

  it("shows a failed command", async () => {
    tauri.answer("github_issues", () => {
      throw new Error("boom");
    });
    TestBed.overrideComponent(IssueFormPane, {
      set: { templateUrl: undefined, styleUrl: undefined, template: "", imports: [] },
    });
    TestBed.overrideComponent(IssuesSection, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        template: readFileSync("src/app/github/issues-section.html", "utf8"),
      },
    });
    const fixture = TestBed.createComponent(IssuesSection);
    await fixture.whenStable();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector("[role=alert]").textContent).toContain("boom");
  });
});

describe("new issue form (#341)", () => {
  it("derives the child level: Epic gives Feature, Feature gives PBI, the rest none", () => {
    expect(childLevel("epic")).toBe("feature");
    expect(childLevel("feature")).toBe("pbi");
    expect(childLevel("pbi")).toBeNull();
    expect(childLevel("bug")).toBeNull();
    expect(childLevel("other")).toBeNull();
    expect(levelPrefix("feature")).toBe("[Feature] ");
    expect(levelPrefix("pbi")).toBe("[Pbi] ");
    expect(levelPrefix("other")).toBe("");
    expect(levelPrefix(null)).toBe("");
  });

  it("presets title prefix and level label from the parent", () => {
    const epic = newIssueForm("o/n", "/r", issue(1, { labels: ["epic"], title: "Big" }));
    expect(epic).toMatchObject({ title: "[Feature] ", labels: ["feature"], parent: { number: 1 } });
    const byPrefix = newIssueForm("o/n", "/r", issue(2, { title: "[Feature] F" }));
    expect(byPrefix).toMatchObject({ title: "[Pbi] ", labels: ["pbi"] });
    const top = newIssueForm("o/n", "/r", null);
    expect(top).toMatchObject({ title: "", labels: [], parent: null });
    expect(newIssueForm("o/n", "/r", issue(3, { labels: ["bug"] })).labels).toEqual([]);
  });

  it("is ready only with a title beyond the prefix", () => {
    expect(formReady({ title: "" })).toBe(false);
    expect(formReady({ title: "[Feature] " })).toBe(false);
    expect(formReady({ title: "[Feature] Do it" })).toBe(true);
    expect(formReady({ title: "Plain" })).toBe(true);
  });

  it("offers parents by number or title, never itself, its parent or its descendants", () => {
    const all = [
      issue(1),
      issue(2, { parent: parent(1), title: "Child" }),
      issue(3, { parent: parent(2), title: "Grandchild" }),
      issue(4, { title: "Other thing" }),
      issue(5, { title: "Another" }),
    ];
    const nums = (q: string) => parentCandidates(all, 1, null, q).map((i) => i.number);
    expect(nums("")).toEqual([4, 5]);
    expect(parentCandidates(all, 3, 2, "").map((i) => i.number)).toEqual([1, 4, 5]);
    expect(nums("#4")).toEqual([4]);
    expect(nums("another")).toEqual([5]);
    expect(nums("child")).toEqual([]);
    expect(parentCandidates(all, 1, null, "", 1)).toHaveLength(1);
  });
});

describe("bulk selection and actions (#342)", () => {
  const TWO: RepoIssues[] = [
    {
      root: "/src/n",
      repo: "o/n",
      status: "ok",
      truncated: false,
      issues: [issue(1), issue(2), issue(3), issue(4)],
    },
    {
      root: "/src/m",
      repo: "o/m",
      status: "ok",
      truncated: false,
      issues: [issue(9)],
    },
  ];

  async function setup() {
    tauri.reset();
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: null }] });
    tauri.answer("github_issues", () => TWO);
    const issues = TestBed.inject(GithubIssues);
    const notify = vi.fn<(text: string) => void>();
    vi.spyOn(TestBed.inject(SpaceActions), "notify").mockImplementation(notify as never);
    await issues.load();
    return { issues, notify };
  }
  afterEach(() => TestBed.resetTestingModule());

  it("toggles single issues and marks a range since the anchor", async () => {
    const { issues } = await setup();
    issues.mark("o/n#1");
    issues.mark("o/n#3", "range");
    expect(issues.markedIssues().map((m) => m.issue.number)).toEqual([1, 2, 3]);
    issues.mark("o/n#2");
    expect(issues.markedIssues().map((m) => m.issue.number)).toEqual([1, 3]);
  });

  it("limits the selection to one repository", async () => {
    const { issues } = await setup();
    issues.mark("o/n#1");
    expect(issues.markDisabled("o/m")).toBe(true);
    expect(issues.markDisabled("o/n")).toBe(false);
    issues.mark("o/m#9");
    expect([...issues.marked()]).toEqual(["o/n#1"]);
    issues.clearMarked();
    expect(issues.markDisabled("o/m")).toBe(false);
  });

  it("runs one request per issue in order and keeps going after a failure", async () => {
    const { issues, notify } = await setup();
    tauri.answer("github_issue_edit", (a) => {
      if (a?.["number"] === 2) throw "boom";
      return { status: "saved" };
    });
    issues.mark("o/n#1");
    issues.mark("o/n#3", "range");
    const r = await issues.bulkEdit({ addLabels: ["bug"] });
    expect(r.done).toEqual([1, 3]);
    expect(r.failed).toEqual([{ number: 2, error: "boom" }]);
    expect(tauri.calls("github_issue_edit").map((c) => (c as { number: number }).number)).toEqual([
      1, 2, 3,
    ]);
    expect(notify).toHaveBeenCalledTimes(1);
    expect(notify.mock.calls[0][0]).toBe(
      "Bulk add label bug: 2 done (#1, #3), 1 failed (#2: boom)",
    );
    expect(issues.marked().size).toBe(0);
  });

  it("closes with the reason", async () => {
    const { issues } = await setup();
    tauri.answer("github_issue_action", () => undefined);
    issues.mark("o/n#4");
    await issues.bulkClose("not_planned");
    expect(tauri.calls("github_issue_action")).toEqual([
      { root: "/src/n", number: 4, action: "not_planned" },
    ]);
  });

  it("formats the summary", () => {
    expect(bulkSummary("x", { done: [1], failed: [] })).toBe("Bulk x: 1 done (#1)");
    expect(bulkSummary("x", { done: [], failed: [{ number: 5, error: "e" }] })).toBe(
      "Bulk x: 0 done, 1 failed (#5: e)",
    );
  });
});

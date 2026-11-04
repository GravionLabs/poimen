// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { UI_STATE_KEY, UI_STORAGE, UiState, parseLayout } from "../core/ui-state";
import { tauri } from "../core/tauri.mock";
import {
  GithubIssues,
  NO_ISSUE_FILTER,
  buildTree,
  filterTree,
  flattenTree,
  issueLevel,
  markers,
  type Issue,
  type RepoIssues,
} from "./issues";
import { IssuesSection } from "./issues-section";

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
    issue(1, { labels: ["epic"], title: "Views" }),
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

  it("reports the marker labels", () => {
    expect(markers(issue(1, { labels: ["Needs-Decision", "x", "needs-refinement"] }))).toEqual([
      "needs-refinement",
      "needs-decision",
    ]);
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
      issue(1, { labels: ["epic"], title: "Views" }),
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
    expect(child).toContain("[needs-refinement]");
    expect(child).toContain("[needs-decision]");
    expect(child).toContain("[@jk]");
    expect(child).toContain("PR #40");
    expect(child).toContain("3 / 1 / 0");
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
    const select = fixture.nativeElement.querySelector("select[aria-label=State]")!;
    select.value = "closed";
    select.dispatchEvent(new Event("change"));
    await fixture.whenStable();
    expect(tauri.calls("github_issues")[1]).toEqual({ dirs: expect.any(Array), closed: true });
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

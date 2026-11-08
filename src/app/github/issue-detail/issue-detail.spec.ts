// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { UI_STORAGE, UiState } from "../../core/ui-state";
import { tauri } from "../../core/tauri.mock";
import { AgentActionMenu } from "../agent-action-menu/agent-action-menu";
import { GithubView } from "../github-view";
import { GithubIssues, type Issue, type IssueDetail, type RepoIssues } from "../issues";
import { IssuesSection } from "../issues-section";
import { InboxSection } from "../inbox-section";
import { MilestonesSection } from "../milestones-section";
import { GithubPulls } from "../pulls";
import { PullDetailPane } from "../pull-detail/pull-detail";
import { IssueDetailPane } from "./issue-detail";

const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-opener", () => opener);
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

function issue(number: number, labels: string[] = ["pbi"]): Issue {
  return {
    number,
    title: `title ${number}`,
    url: `https://github.com/o/n/issues/${number}`,
    state: "OPEN",
    labels,
    assignees: [],
    milestone: null,
    parent: null,
    subTotal: 0,
    subCompleted: 0,
    prs: [],
  };
}

const RESULT: RepoIssues[] = [
  { root: "/src/n", repo: "o/n", status: "ok", issues: [issue(1), issue(2)], truncated: false },
];

const PULLS = {
  viewer: "jk",
  repos: [
    {
      root: "/src/n",
      repo: "o/n",
      status: "ok",
      pulls: [
        {
          number: 40,
          title: "The PR",
          url: "https://github.com/o/n/pull/40",
          author: "jk",
          isDraft: false,
          additions: 1,
          deletions: 1,
          createdAt: "2026-11-01T00:00:00Z",
          headRef: "feat/b",
          isCrossRepository: false,
          baseRef: "main",
          reviewDecision: null,
          reviewRequests: [],
          checks: { state: "none", passed: 0, failed: 0, pending: 0 },
        },
      ],
    },
  ],
};

const DETAIL: IssueDetail = {
  number: 1,
  title: "title 1",
  url: "https://github.com/o/n/issues/1",
  body: "Hello **world** <script>alert(1)</script> [x](javascript:alert(2))",
  state: "CLOSED",
  stateReason: "NOT_PLANNED",
  author: "jk",
  labels: ["pbi", "frontend"],
  assignees: ["octocat"],
  milestone: "M2",
  parent: { number: 9, title: "The parent", state: "", repo: "o/n" },
  subIssues: [
    { number: 2, title: "Sub", state: "OPEN", repo: "o/n" },
    { number: 3, title: "Elsewhere", state: "CLOSED", repo: "x/other" },
  ],
  pullRequests: [
    {
      number: 40,
      title: "The PR",
      url: "https://github.com/o/n/pull/40",
      state: "MERGED",
      repo: "o/n",
    },
  ],
  viewerCanUpdate: true,
};

function withTemplate(component: unknown, file: string) {
  TestBed.overrideComponent(component as never, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync(`src/app/github/${file}`, "utf8"),
    },
  });
}

async function render(detail: () => unknown = () => DETAIL) {
  tauri.answer("github_pulls", () => PULLS);
  tauri.answer("github_pull_detail", () => ({}));
  tauri.answer("github_issues", () => RESULT);
  tauri.answer("github_issue_detail", detail);
  withTemplate(GithubView, "github-view.html");
  withTemplate(AgentActionMenu, "agent-action-menu/agent-action-menu.html");
  withTemplate(IssuesSection, "issues-section.html");
  withTemplate(InboxSection, "inbox-section.html");
  withTemplate(MilestonesSection, "milestones-section.html");
  withTemplate(PullDetailPane, "pull-detail/pull-detail.html");
  withTemplate(IssueDetailPane, "issue-detail/issue-detail.html");
  TestBed.inject(UiState).selectGithubPage("issues");
  const fixture = TestBed.createComponent(GithubView);
  await fixture.whenStable();
  TestBed.inject(GithubPulls).scope.set("all");
  fixture.detectChanges();
  return fixture;
}

const q = (el: HTMLElement, sel: string) => el.querySelector<HTMLElement>(sel);
const key = (el: HTMLElement, k: string) =>
  q(el, "section.issues")!.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));

async function settle(fixture: { whenStable(): Promise<unknown>; detectChanges(): void }) {
  await fixture.whenStable();
  fixture.detectChanges();
}

beforeEach(() => {
  localStorage.clear();
  tauri.reset();
  opener.openUrl.mockReset();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [{ provide: UI_STORAGE, useValue: localStorage }],
  });
});

describe("issue detail pane", () => {
  it("moves the cursor with the arrow keys, clamped, and opens with Enter", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    expect(q(el, "[data-testid=issue-detail]")).toBeNull();
    key(el, "ArrowUp");
    fixture.detectChanges();
    expect(q(el, ".row.cursor")!.dataset["number"]).toBe("2");
    key(el, "ArrowDown");
    fixture.detectChanges();
    expect(q(el, ".row.cursor")!.dataset["number"]).toBe("2");
    key(el, "ArrowUp");
    key(el, "Enter");
    await settle(fixture);
    expect(tauri.calls("github_issue_detail")).toEqual([{ root: "/src/n", number: 1 }]);
    expect(q(el, "[data-testid=detail-number]")!.textContent).toBe("#1");
    expect(q(el, "[data-testid=detail-level]")!.textContent).toBe("pbi");
  });

  it("opens on a row click and closes with Escape and the close button", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    q(el, '.row[data-number="2"]')!.click();
    await settle(fixture);
    expect(q(el, "[data-testid=issue-detail]")).not.toBeNull();
    key(el, "Escape");
    fixture.detectChanges();
    expect(q(el, "[data-testid=issue-detail]")).toBeNull();
    q(el, '.row[data-number="2"]')!.click();
    await settle(fixture);
    q(el, "[data-testid=detail-close]")!.click();
    fixture.detectChanges();
    expect(q(el, "[data-testid=issue-detail]")).toBeNull();
  });

  it("renders metadata, a sanitized body and sub-issues", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    q(el, '.row[data-number="1"]')!.click();
    await settle(fixture);
    const body = q(el, "[data-testid=detail-body]")!;
    expect(body.innerHTML).toContain("<strong>world</strong>");
    expect(body.innerHTML).not.toContain("<script");
    expect(body.innerHTML).not.toContain("javascript:");
    expect(q(el, "[data-testid=detail-state]")!.textContent!.replace(/\s+/g, " ").trim()).toBe(
      "closed (not planned)",
    );
    expect(q(el, "[data-testid=detail-labels]")!.textContent).toContain("frontend");
    expect(q(el, "[data-testid=detail-assignees]")!.textContent).toContain("octocat");
    expect(q(el, "[data-testid=detail-milestone]")!.textContent).toContain("M2");
    expect(q(el, "[data-testid=actions]")!.children).toHaveLength(0);
    const subs = el.querySelectorAll<HTMLButtonElement>("[data-testid=detail-sub]");
    expect(subs).toHaveLength(2);
    expect(subs[0].disabled).toBe(false);
    expect(subs[1].disabled).toBe(true);
  });

  it("selects a sub-issue in the pane", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    q(el, '.row[data-number="1"]')!.click();
    await settle(fixture);
    q(el, "[data-testid=detail-sub]")!.click();
    await settle(fixture);
    expect(tauri.calls("github_issue_detail")).toEqual([
      { root: "/src/n", number: 1 },
      { root: "/src/n", number: 2 },
    ]);
    expect(TestBed.inject(GithubIssues).detailKey()).toBe("o/n#2");
  });

  it("selects a linked pull request on the Pull requests sub-page", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    q(el, '.row[data-number="1"]')!.click();
    await settle(fixture);
    expect(q(el, "[data-testid=detail-pr]")!.textContent).toContain("#40 The PR");
    q(el, "[data-testid=detail-pr]")!.click();
    expect(TestBed.inject(UiState).githubPage()).toBe("pulls");
    expect(TestBed.inject(GithubPulls).detailKey()).toBe("o/n#40");
  });

  it("shows why the detail could not be read", async () => {
    const fixture = await render(() => {
      throw { kind: "not-found" };
    });
    const el = fixture.nativeElement as HTMLElement;
    q(el, '.row[data-number="1"]')!.click();
    await settle(fixture);
    expect(q(el, "[data-testid=detail-error]")!.textContent).toContain("Could not read");
    expect(q(el, "[data-testid=detail-body]")).toBeNull();
  });

  it("does not open from a row button", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    q(el, '.row[data-number="1"] button.open')!.click();
    fixture.detectChanges();
    expect(tauri.calls("github_issue_detail")).toEqual([]);
  });

  it("overlays the list under 1100 px through the shared stylesheet", () => {
    expect(readFileSync("src/app/github/issue-detail/issue-detail.css", "utf8")).toContain(
      "detail-pane.css",
    );
    expect(readFileSync("src/app/github/detail-pane.css", "utf8")).toMatch(
      /@media \(max-width: 1099\.98px\)[\s\S]*position: absolute/,
    );
  });
});

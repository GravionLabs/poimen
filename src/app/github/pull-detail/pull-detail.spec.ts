// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { UI_STORAGE, UiState } from "../../core/ui-state";
import { tauri } from "../../core/tauri.mock";
import { GithubView } from "../github-view";
import { IssuesSection } from "../issues-section";
import { GithubIssues } from "../issues";
import { InboxSection } from "../inbox-section";
import { MilestonesSection } from "../milestones-section";
import { GithubPulls, type Pull, type PullDetail, type PullsResult } from "../pulls";
import { IssueDetailPane } from "../issue-detail/issue-detail";
import { PullDetailPane } from "./pull-detail";

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

function pull(number: number): Pull {
  return {
    number,
    title: `title ${number}`,
    url: `https://github.com/o/n/pull/${number}`,
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
  };
}

const RESULT: PullsResult = {
  viewer: "jk",
  repos: [{ root: "/src/n", repo: "o/n", status: "ok", pulls: [pull(1), pull(2)] }],
};

const DETAIL: PullDetail = {
  number: 1,
  title: "title 1",
  url: "https://github.com/o/n/pull/1",
  body: "Hello **world** <script>alert(1)</script>",
  state: "OPEN",
  isDraft: false,
  author: "jk",
  headRef: "feat/b",
  baseRef: "main",
  mergeable: "MERGEABLE",
  mergeStateStatus: "CLEAN",
  checks: [
    { name: "frontend", state: "passing", url: "https://github.com/o/n/actions/runs/1" },
    { name: "rust", state: "failing", url: null },
  ],
  reviews: [{ author: "octocat", state: "CHANGES_REQUESTED", submittedAt: "2026-11-05T10:00:00Z" }],
  linkedIssues: [
    {
      number: 7,
      title: "The issue",
      url: "https://github.com/o/n/issues/7",
      state: "OPEN",
      repo: "o/n",
    },
  ],
  viewerCanUpdate: true,
  viewerDidAuthor: false,
  viewerCanMergeAsAdmin: false,
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
  tauri.answer("github_pulls", () => RESULT);
  tauri.answer("github_pull_detail", detail);
  withTemplate(GithubView, "github-view.html");
  withTemplate(IssuesSection, "issues-section.html");
  withTemplate(InboxSection, "inbox-section.html");
  withTemplate(MilestonesSection, "milestones-section.html");
  withTemplate(PullDetailPane, "pull-detail/pull-detail.html");
  withTemplate(IssueDetailPane, "issue-detail/issue-detail.html");
  TestBed.inject(UiState).selectGithubPage("pulls");
  const fixture = TestBed.createComponent(GithubView);
  await fixture.whenStable();
  TestBed.inject(GithubPulls).scope.set("all");
  fixture.detectChanges();
  return fixture;
}

const q = (el: HTMLElement, sel: string) => el.querySelector<HTMLElement>(sel);

beforeEach(() => {
  localStorage.clear();
  tauri.reset();
  opener.openUrl.mockReset();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [{ provide: UI_STORAGE, useValue: localStorage }],
  });
});

describe("pull request detail pane", () => {
  it("opens on a row click and asks for that pull request", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    expect(q(el, "[data-testid=pull-detail]")).toBeNull();
    q(el, 'tr.row[data-key="o/n#1"]')!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(tauri.calls("github_pull_detail")).toEqual([{ root: "/src/n", number: 1 }]);
    expect(q(el, "[data-testid=detail-number]")!.textContent).toBe("#1");
    expect(q(el, "[data-testid=detail-branches]")!.textContent).toBe("feat/b → main");
  });

  it("opens with Enter on the cursor row and closes with Escape", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    const body = q(el, "tbody")!;
    body.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    body.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await fixture.whenStable();
    fixture.detectChanges();
    expect(q(el, "[data-testid=pull-detail]")).not.toBeNull();
    body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    fixture.detectChanges();
    expect(q(el, "[data-testid=pull-detail]")).toBeNull();
  });

  it("closes with the close button", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    q(el, 'tr.row[data-key="o/n#1"]')!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    q(el, "[data-testid=detail-close]")!.click();
    fixture.detectChanges();
    expect(q(el, "[data-testid=pull-detail]")).toBeNull();
  });

  it("renders the sections, the sanitized body, and an empty action bar", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    q(el, 'tr.row[data-key="o/n#1"]')!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    const body = q(el, "[data-testid=detail-body]")!;
    expect(body.innerHTML).toContain("<strong>world</strong>");
    expect(body.innerHTML).not.toContain("<script");
    expect(q(el, "[data-testid=detail-checks]")!.textContent).toContain("frontend");
    expect(q(el, "[data-testid=detail-checks]")!.textContent).toContain("rust");
    expect(q(el, "[data-testid=detail-checks] a")!.textContent).toBe("frontend");
    expect(q(el, "[data-testid=detail-reviews]")!.textContent).toContain("octocat");
    expect(q(el, "[data-testid=detail-reviews]")!.textContent).toContain("changes requested");
    expect(q(el, "[data-testid=detail-issue]")!.textContent).toContain("o/n#7 The issue");
    expect(q(el, "[data-testid=actions]")!.children).toHaveLength(0);
    q(el, "[data-testid=detail-checks] a")!.click();
    expect(opener.openUrl).toHaveBeenCalledWith("https://github.com/o/n/actions/runs/1");
  });

  it("selects a linked issue on the Issues sub-page", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    q(el, 'tr.row[data-key="o/n#1"]')!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    q(el, "[data-testid=detail-issue]")!.click();
    expect(TestBed.inject(UiState).githubPage()).toBe("issues");
    expect(TestBed.inject(GithubIssues).selected()).toEqual({ repo: "o/n", number: 7 });
  });

  it("shows why the detail could not be read", async () => {
    const fixture = await render(() => {
      throw { kind: "not-found" };
    });
    const el = fixture.nativeElement as HTMLElement;
    q(el, 'tr.row[data-key="o/n#1"]')!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(q(el, "[data-testid=detail-error]")!.textContent).toContain("Could not read");
    expect(q(el, "[data-testid=detail-body]")).toBeNull();
  });

  it("does not open from the row checkbox", async () => {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    q(el, 'tr.row[data-key="o/n#1"] input[type=checkbox]')!.click();
    fixture.detectChanges();
    expect(tauri.calls("github_pull_detail")).toEqual([]);
  });

  it("overlays the list under 1100 px through its stylesheet", () => {
    const css = readFileSync("src/app/github/detail-pane.css", "utf8");
    expect(css).toMatch(/@media \(max-width: 1099\.98px\)[\s\S]*position: absolute/);
  });
});

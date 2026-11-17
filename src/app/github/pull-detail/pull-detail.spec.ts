// @vitest-environment jsdom
import { IssueFormPane } from "../issue-form/issue-form";
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { SpaceActions } from "../../core/space-actions";
import { UI_STORAGE, UiState } from "../../core/ui-state";
import { tauri } from "../../core/tauri.mock";
import { AgentActionMenu } from "../agent-action-menu/agent-action-menu";
import { GithubView } from "../github-view";
import { IssuesSection } from "../issues-section";
import { GithubIssues } from "../issues";
import { InboxSection } from "../inbox-section";
import { MilestonesSection } from "../milestones-section";
import {
  GithubPulls,
  availableActions,
  canEditPeople,
  canMerge,
  canRerun,
  defaultMergeMethod,
  type Pull,
  type PullDetail,
  type PullsResult,
} from "../pulls";
import { IssueDetailPane } from "../issue-detail/issue-detail";
import { PullDetailPane } from "./pull-detail";
import { ChipPicker } from "../chip-picker/chip-picker";

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
  headSha: "0123456789abcdef",
  mergeMethods: ["squash", "rebase"],
  mergeBlock: null,
  reviewers: ["octocat"],
  labels: ["bug"],
  assignees: [],
  assignableUsers: ["jk", "octocat", "hubot"],
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
  withTemplate(AgentActionMenu, "agent-action-menu/agent-action-menu.html");
  withTemplate(IssuesSection, "issues-section.html");
  withTemplate(InboxSection, "inbox-section.html");
  withTemplate(MilestonesSection, "milestones-section.html");
  withTemplate(PullDetailPane, "pull-detail/pull-detail.html");
  withTemplate(ChipPicker, "chip-picker/chip-picker.html");
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
    expect(q(el, "[data-testid=detail-checks] button")!.textContent!.trim()).toBe("frontend");
    expect(q(el, "[data-testid=detail-reviews]")!.textContent).toContain("octocat");
    expect(q(el, "[data-testid=detail-reviews]")!.textContent).toContain("changes requested");
    expect(q(el, "[data-testid=detail-issue]")!.textContent).toContain("o/n#7 The issue");
    expect(q(el, "[data-testid=actions]")!.children).toHaveLength(0);
    expect(q(el, "[data-testid=pr-actions]")).not.toBeNull();
    q(el, "[data-testid=detail-checks] button")!.click();
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

describe("pull request write actions", () => {
  const base = { state: "OPEN", isDraft: false, viewerCanUpdate: true, viewerDidAuthor: false };

  it("offers the actions the viewer may use", () => {
    expect(availableActions(base)).toEqual([
      "approve",
      "request_changes",
      "comment",
      "draft",
      "close",
    ]);
    expect(availableActions({ ...base, isDraft: true })).toContain("ready");
    expect(availableActions({ ...base, state: "CLOSED" })).toEqual(["reopen"]);
  });

  it("hides review on own pull requests, writes without viewerCanUpdate and on merged ones", () => {
    expect(availableActions({ ...base, viewerDidAuthor: true })).toEqual([
      "comment",
      "draft",
      "close",
    ]);
    expect(availableActions({ ...base, viewerCanUpdate: false })).toEqual([]);
    expect(availableActions({ ...base, state: "MERGED" })).toEqual([]);
  });

  async function open() {
    const fixture = await render();
    const el = fixture.nativeElement as HTMLElement;
    q(el, 'tr.row[data-key="o/n#1"]')!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    return { fixture, el };
  }

  it("approves at once, reloads the list and the pane", async () => {
    tauri.answer("github_pr_action", () => null);
    const { fixture, el } = await open();
    q(el, "[data-testid=pr-approve]")!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(tauri.calls("github_pr_action")).toEqual([
      { root: "/src/n", number: 1, action: "approve", body: null },
    ]);
    expect(tauri.calls("github_pull_detail")).toHaveLength(2);
    expect(tauri.calls("github_pulls").length).toBeGreaterThan(1);
  });

  it("needs a body for request changes", async () => {
    tauri.answer("github_pr_action", () => null);
    const { fixture, el } = await open();
    q(el, "[data-testid=pr-request_changes]")!.click();
    fixture.detectChanges();
    const submit = q(el, "[data-testid=pr-submit]") as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    const field = q(el, "[data-testid=pr-body]") as HTMLTextAreaElement;
    field.value = "please fix";
    field.dispatchEvent(new Event("input"));
    fixture.detectChanges();
    expect(submit.disabled).toBe(false);
    submit.click();
    await fixture.whenStable();
    expect(tauri.calls("github_pr_action")).toEqual([
      { root: "/src/n", number: 1, action: "request_changes", body: "please fix" },
    ]);
  });

  it("asks before closing and runs nothing on cancel", async () => {
    tauri.answer("github_pr_action", () => null);
    const { fixture, el } = await open();
    q(el, "[data-testid=pr-close]")!.click();
    fixture.detectChanges();
    expect(tauri.calls("github_pr_action")).toHaveLength(0);
    q(el, "[data-testid=pr-cancel]")!.click();
    fixture.detectChanges();
    expect(q(el, "[data-testid=pr-compose]")).toBeNull();
    q(el, "[data-testid=pr-close]")!.click();
    fixture.detectChanges();
    q(el, "[data-testid=pr-submit]")!.click();
    await fixture.whenStable();
    expect(tauri.calls("github_pr_action")).toEqual([
      { root: "/src/n", number: 1, action: "close", body: null },
    ]);
  });

  it("drops the confirmation and the typed text when another pull request is selected", async () => {
    tauri.answer("github_pr_action", () => null);
    const { fixture, el } = await open();
    q(el, "[data-testid=pr-request_changes]")!.click();
    fixture.detectChanges();
    const field = q(el, "[data-testid=pr-body]") as HTMLTextAreaElement;
    field.value = "please fix";
    field.dispatchEvent(new Event("input"));
    fixture.detectChanges();
    q(el, 'tr.row[data-key="o/n#2"]')!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(q(el, "[data-testid=pr-compose]")).toBeNull();
    q(el, "[data-testid=pr-request_changes]")!.click();
    fixture.detectChanges();
    expect((q(el, "[data-testid=pr-body]") as HTMLTextAreaElement).value).toBe("");
    expect(tauri.calls("github_pr_action")).toHaveLength(0);
  });

  it("notifies with the action and the gh message on failure", async () => {
    tauri.answer("github_pr_action", () => {
      throw { kind: "other", message: "not allowed" };
    });
    const { fixture, el } = await open();
    q(el, "[data-testid=pr-approve]")!.click();
    await fixture.whenStable();
    expect(TestBed.inject(SpaceActions).warning()).toBe("Approve #1: not allowed");
  });
});

describe("merge, re-run and people", () => {
  const view = { state: "OPEN", viewerCanUpdate: true };

  it("gates the controls by state and viewerCanUpdate", () => {
    expect(canMerge({ ...view, mergeMethods: ["merge"] })).toBe(true);
    expect(canMerge({ ...view, mergeMethods: [] })).toBe(false);
    expect(canMerge({ ...view, viewerCanUpdate: false, mergeMethods: ["merge"] })).toBe(false);
    expect(canMerge({ ...view, state: "MERGED", mergeMethods: ["merge"] })).toBe(false);
    const failing = [{ name: "x", state: "failing" as const, url: null }];
    expect(canRerun({ ...view, checks: failing })).toBe(true);
    expect(
      canRerun({ ...view, checks: [{ name: "x", state: "passing" as const, url: null }] }),
    ).toBe(false);
    expect(canRerun({ ...view, viewerCanUpdate: false, checks: failing })).toBe(false);
    expect(canEditPeople(view)).toBe(true);
    expect(canEditPeople({ ...view, state: "CLOSED" })).toBe(false);
    expect(defaultMergeMethod(["rebase", "squash"])).toBe("squash");
    expect(defaultMergeMethod(["rebase", "merge"])).toBe("rebase");
    expect(defaultMergeMethod([])).toBeNull();
  });

  async function open(detail: Partial<PullDetail> = {}) {
    tauri.answer("github_pr_merge", () => null);
    tauri.answer("github_pr_rerun", () => 2);
    tauri.answer("github_pr_edit", () => null);
    tauri.answer("github_labels", () => ["bug", "github"]);
    const fixture = await render(() => ({ ...DETAIL, ...detail }));
    const el = fixture.nativeElement as HTMLElement;
    q(el, 'tr.row[data-key="o/n#1"]')!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    return { fixture, el };
  }

  it("confirms the method and target branch, then merges with delete branch on", async () => {
    const { fixture, el } = await open();
    q(el, "[data-testid=pr-merge]")!.click();
    fixture.detectChanges();
    expect(tauri.calls("github_pr_merge")).toHaveLength(0);
    const text = q(el, "[data-testid=merge-confirm]")!.textContent!;
    expect(text).toContain("main");
    expect(q(el, "[data-testid=merge-method-label]")!.textContent).toBe("Squash and merge");
    const options = [
      ...el.querySelectorAll<HTMLOptionElement>("[data-testid=merge-method] option"),
    ];
    expect(options.map((o) => o.value)).toEqual(["squash", "rebase"]);
    expect((q(el, "[data-testid=merge-delete]") as HTMLInputElement).checked).toBe(true);
    const select = q(el, "[data-testid=merge-method]") as HTMLSelectElement;
    select.value = "rebase";
    select.dispatchEvent(new Event("change"));
    fixture.detectChanges();
    q(el, "[data-testid=merge-submit]")!.click();
    await fixture.whenStable();
    expect(tauri.calls("github_pr_merge")).toEqual([
      { root: "/src/n", number: 1, method: "rebase", deleteBranch: true },
    ]);
  });

  it("can keep the branch and cancel without sending", async () => {
    const { fixture, el } = await open();
    q(el, "[data-testid=pr-merge]")!.click();
    fixture.detectChanges();
    const box = q(el, "[data-testid=merge-delete]") as HTMLInputElement;
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    fixture.detectChanges();
    q(el, "[data-testid=merge-submit]")!.click();
    await fixture.whenStable();
    expect(tauri.calls("github_pr_merge")).toEqual([
      { root: "/src/n", number: 1, method: "squash", deleteBranch: false },
    ]);
    q(el, "[data-testid=pr-merge]")?.click();
    fixture.detectChanges();
    q(el, "[data-testid=merge-cancel]")?.click();
    fixture.detectChanges();
    expect(q(el, "[data-testid=merge-confirm]")).toBeNull();
  });

  it("disables merge with the reason when the merge state blocks it", async () => {
    const { fixture, el } = await open({
      mergeStateStatus: "BEHIND",
      mergeBlock: "The branch is behind its base branch.",
    });
    const button = q(el, "[data-testid=pr-merge]") as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(q(el, "[data-testid=merge-reason]")!.textContent).toContain("behind");
    button.click();
    fixture.detectChanges();
    expect(q(el, "[data-testid=merge-confirm]")).toBeNull();
    expect(tauri.calls("github_pr_merge")).toHaveLength(0);
  });

  it("offers only the methods the repository allows and hides merge without any", async () => {
    const { el } = await open({ mergeMethods: [] });
    expect(q(el, "[data-testid=pr-merge]")).toBeNull();
  });

  it("re-runs failed checks and says how many runs", async () => {
    const { fixture, el } = await open();
    q(el, "[data-testid=pr-rerun]")!.click();
    await fixture.whenStable();
    expect(tauri.calls("github_pr_rerun")).toEqual([{ root: "/src/n", number: 1 }]);
    expect(TestBed.inject(SpaceActions).warning()).toContain("Re-running 2 failed runs of #1");
  });

  it("hides re-run without a failed check", async () => {
    const { el } = await open({ checks: [{ name: "a", state: "passing" as const, url: null }] });
    expect(q(el, "[data-testid=pr-rerun]")).toBeNull();
  });

  it("hides every new control without viewerCanUpdate", async () => {
    const { el } = await open({ viewerCanUpdate: false });
    expect(q(el, "[data-testid=pr-merge]")).toBeNull();
    expect(q(el, "[data-testid=pr-rerun]")).toBeNull();
    expect(q(el, "[data-testid=people]")).toBeNull();
    expect(tauri.calls("github_labels")).toHaveLength(0);
  });

  it("edits reviewers, labels and assignees through the chip pickers", async () => {
    const { fixture, el } = await open();
    const pickers = [...el.querySelectorAll<HTMLElement>("[data-testid=chip-picker]")];
    expect(pickers.map((p) => p.querySelector(".name")!.textContent)).toEqual([
      "Reviewers",
      "Labels",
      "Assignees",
    ]);
    expect(pickers[0].querySelector("[data-testid=chip]")!.textContent).toContain("octocat");
    pickers[0].querySelector<HTMLElement>("[data-testid=chip-remove]")!.click();
    await fixture.whenStable();
    const labels = pickers[1].querySelector<HTMLInputElement>("[data-testid=chip-input]")!;
    labels.value = "git";
    labels.dispatchEvent(new Event("input"));
    fixture.detectChanges();
    pickers[1].querySelector<HTMLElement>("[data-testid=chip-option]")!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    const assignees = el.querySelectorAll<HTMLElement>("[data-testid=chip-picker]")[2];
    const input = assignees.querySelector<HTMLInputElement>("[data-testid=chip-input]")!;
    input.value = "hub";
    input.dispatchEvent(new Event("input"));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    await fixture.whenStable();
    expect(tauri.calls("github_pr_edit")).toEqual([
      { root: "/src/n", number: 1, edit: { removeReviewers: ["octocat"] } },
      { root: "/src/n", number: 1, edit: { addLabels: ["github"] } },
      { root: "/src/n", number: 1, edit: { addAssignees: ["hubot"] } },
    ]);
  });

  it("notifies when the merge fails", async () => {
    const { fixture, el } = await open();
    tauri.answer("github_pr_merge", () => {
      throw { kind: "other", message: "base branch was modified" };
    });
    q(el, "[data-testid=pr-merge]")!.click();
    fixture.detectChanges();
    q(el, "[data-testid=merge-submit]")!.click();
    await fixture.whenStable();
    expect(TestBed.inject(SpaceActions).warning()).toBe("Merge #1: base branch was modified");
  });
});

// The form has its own spec (issue-detail.spec.ts); here it stays empty.
beforeEach(() => {
  TestBed.overrideComponent(IssueFormPane, {
    set: { templateUrl: undefined, styleUrl: undefined, template: "" },
  });
});

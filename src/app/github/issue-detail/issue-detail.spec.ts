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
import { ChipPicker } from "../chip-picker/chip-picker";
import { IssueFormPane } from "../issue-form/issue-form";
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
  assignableUsers: ["octocat", "jk"],
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
  withTemplate(ChipPicker, "chip-picker/chip-picker.html");
  withTemplate(IssueDetailPane, "issue-detail/issue-detail.html");
  withTemplate(IssueFormPane, "issue-form/issue-form.html");
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
    // Closed issue: its task button and Reopen (the viewer may update).
    expect(q(el, "[data-testid=actions]")!.children).toHaveLength(4);
    expect(q(el, "[data-testid=start-task]")!.textContent!.trim()).toBe("Start task…");
    expect(q(el, "[data-testid=issue-reopen]")).not.toBeNull();
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

describe("issue detail editing", () => {
  const OPEN: IssueDetail = { ...DETAIL, state: "OPEN", stateReason: null };

  async function open(detail: IssueDetail = OPEN) {
    tauri.answer("github_labels", () => ["bug", "pbi", "docs"]);
    tauri.answer("github_milestones", () => [
      { root: "/src/n", repo: "o/n", status: "ok", milestones: [{ title: "M2" }, { title: "M3" }] },
    ]);
    tauri.answer("github_issue_edit", () => ({ status: "saved" }));
    tauri.answer("github_issue_action", () => null);
    const fixture = await render(() => detail);
    const el = fixture.nativeElement as HTMLElement;
    q(el, '.row[data-number="1"]')!.click();
    await settle(fixture);
    return { fixture, el };
  }

  function type(el: HTMLElement, sel: string, value: string) {
    const input = q(el, sel) as HTMLInputElement | HTMLTextAreaElement;
    input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }

  it("hides every edit control without viewerCanUpdate", async () => {
    const { el } = await open({ ...OPEN, viewerCanUpdate: false });
    for (const id of [
      "title-edit",
      "body-edit",
      "issue-people",
      "issue-close-completed",
      "issue-close-not-planned",
      "issue-reopen",
      "add-sub-issue",
      "change-parent",
    ]) {
      expect(q(el, `[data-testid=${id}]`), id).toBeNull();
    }
    expect(tauri.calls("github_labels")).toEqual([]);
  });

  it("saves the title inline and reloads tree and pane", async () => {
    const { fixture, el } = await open();
    q(el, "[data-testid=title-edit]")!.click();
    fixture.detectChanges();
    type(el, "[data-testid=title-input]", "  A better title ");
    fixture.detectChanges();
    q(el, "[data-testid=title-save]")!.click();
    await settle(fixture);
    expect(tauri.calls("github_issue_edit")).toEqual([
      { root: "/src/n", number: 1, edit: { title: "A better title" } },
    ]);
    expect(tauri.calls("github_issues").length).toBe(2);
    expect(tauri.calls("github_issue_detail").length).toBe(2);
    expect(q(el, "[data-testid=title-form]")).toBeNull();
  });

  it("does not send a blank or unchanged title and cancel leaves it", async () => {
    const { fixture, el } = await open();
    q(el, "[data-testid=title-edit]")!.click();
    fixture.detectChanges();
    type(el, "[data-testid=title-input]", "   ");
    q(el, "[data-testid=title-save]")!.click();
    await settle(fixture);
    expect(tauri.calls("github_issue_edit")).toEqual([]);
    q(el, "[data-testid=title-edit]")!.click();
    fixture.detectChanges();
    q(el, "[data-testid=title-cancel]")!.click();
    fixture.detectChanges();
    expect(q(el, "[data-testid=title-form]")).toBeNull();
    expect(tauri.calls("github_issue_edit")).toEqual([]);
  });

  it("edits the body with a sanitized preview and saves against the loaded text", async () => {
    const { fixture, el } = await open();
    q(el, "[data-testid=body-edit]")!.click();
    fixture.detectChanges();
    type(el, "[data-testid=body-input]", "New **text** <script>x</script>");
    fixture.detectChanges();
    q(el, "[data-testid=body-mode-preview]")!.click();
    fixture.detectChanges();
    const preview = q(el, "[data-testid=body-preview]")!;
    expect(preview.innerHTML).toContain("<strong>text</strong>");
    expect(preview.innerHTML).not.toContain("<script");
    q(el, "[data-testid=body-save]")!.click();
    await settle(fixture);
    expect(tauri.calls("github_issue_edit")).toEqual([
      {
        root: "/src/n",
        number: 1,
        edit: {
          body: "New **text** <script>x</script>",
          expectedBody: OPEN.body,
          overwrite: false,
        },
      },
    ]);
    expect(q(el, "[data-testid=body-form]")).toBeNull();
  });

  it("offers Overwrite or Reload when the body changed and writes nothing until chosen", async () => {
    const { fixture, el } = await open();
    tauri.answer("github_issue_edit", () => ({ status: "conflict", current: "Changed on GitHub" }));
    q(el, "[data-testid=body-edit]")!.click();
    fixture.detectChanges();
    type(el, "[data-testid=body-input]", "mine");
    q(el, "[data-testid=body-save]")!.click();
    await settle(fixture);
    expect(q(el, "[data-testid=body-conflict]")).not.toBeNull();
    expect(q(el, "[data-testid=body-form]")).not.toBeNull();
    expect(tauri.calls("github_issues").length).toBe(1);
    // Reload: the text on GitHub becomes the description, the draft is dropped.
    q(el, "[data-testid=body-reload]")!.click();
    fixture.detectChanges();
    expect(q(el, "[data-testid=body-form]")).toBeNull();
    expect(q(el, "[data-testid=detail-body]")!.textContent).toContain("Changed on GitHub");
    expect(tauri.calls("github_issue_edit")).toHaveLength(1);
  });

  it("Overwrite sends the draft again with overwrite set", async () => {
    const { fixture, el } = await open();
    tauri.answer("github_issue_edit", (args: unknown) => {
      const edit = (args as { edit: { overwrite: boolean } }).edit;
      return edit.overwrite ? { status: "saved" } : { status: "conflict", current: "other" };
    });
    q(el, "[data-testid=body-edit]")!.click();
    fixture.detectChanges();
    type(el, "[data-testid=body-input]", "mine");
    q(el, "[data-testid=body-save]")!.click();
    await settle(fixture);
    q(el, "[data-testid=body-overwrite]")!.click();
    await settle(fixture);
    const calls = tauri.calls("github_issue_edit") as {
      edit: { overwrite: boolean; body: string };
    }[];
    expect(calls.map((c) => c.edit.overwrite)).toEqual([false, true]);
    expect(calls[1].edit.body).toBe("mine");
    expect(q(el, "[data-testid=body-form]")).toBeNull();
  });

  it("keeps the baseline of a body draft when labels are edited meanwhile", async () => {
    const { fixture, el } = await open();
    q(el, "[data-testid=body-edit]")!.click();
    fixture.detectChanges();
    tauri.answer("github_issue_detail", () => ({ ...OPEN, body: "changed meanwhile" }));
    const issues = TestBed.inject(GithubIssues);
    await issues.editMeta({ addLabels: ["bug"] });
    await settle(fixture);
    expect(issues.bodyDraft()).toBe(OPEN.body);
    await issues.saveBody();
    const last = tauri.calls("github_issue_edit").at(-1) as { edit: { expectedBody: string } };
    expect(last.edit.expectedBody).toBe(OPEN.body);
  });

  it("edits labels, assignees and the milestone through the pickers", async () => {
    const { fixture, el } = await open();
    const issues = TestBed.inject(GithubIssues);
    expect(issues.labelOptions()).toEqual(["bug", "pbi", "docs"]);
    expect(issues.milestoneOptions()).toEqual(["M2", "M3"]);
    const pickers = el.querySelectorAll("[data-testid=issue-people] app-chip-picker");
    expect(pickers).toHaveLength(3);
    await issues.editMeta({ addLabels: ["bug"] });
    await issues.editMeta({ removeAssignees: ["octocat"] });
    await issues.editMeta({ milestone: "M3" });
    await issues.editMeta({ removeMilestone: true });
    expect((tauri.calls("github_issue_edit") as { edit: unknown }[]).map((c) => c.edit)).toEqual([
      { addLabels: ["bug"] },
      { removeAssignees: ["octocat"] },
      { milestone: "M3" },
      { removeMilestone: true },
    ]);
    expect(await issues.editMeta({ addLabels: [] })).toBe(false);
    fixture.detectChanges();
  });

  it("closes as completed or not planned and reopens", async () => {
    const { fixture, el } = await open();
    q(el, "[data-testid=issue-close-completed]")!.click();
    await settle(fixture);
    q(el, "[data-testid=issue-close-not-planned]")!.click();
    await settle(fixture);
    expect(tauri.calls("github_issue_action")).toEqual([
      { root: "/src/n", number: 1, action: "completed" },
      { root: "/src/n", number: 1, action: "not_planned" },
    ]);
    expect(tauri.calls("github_issues").length).toBe(3);
    // A closed issue offers Reopen only.
    tauri.answer("github_issue_detail", () => DETAIL);
    await TestBed.inject(GithubIssues).openDetail("o/n", 1);
    await settle(fixture);
    expect(q(el, "[data-testid=issue-close-completed]")).toBeNull();
    q(el, "[data-testid=issue-reopen]")!.click();
    await settle(fixture);
    expect(tauri.calls("github_issue_action").at(-1)).toEqual({
      root: "/src/n",
      number: 1,
      action: "reopen",
    });
  });

  it("notifies the gh message of a failed write and reloads nothing", async () => {
    const { fixture, el } = await open();
    tauri.answer("github_issue_action", () => {
      throw { kind: "other", message: "boom" };
    });
    q(el, "[data-testid=issue-close-completed]")!.click();
    await settle(fixture);
    expect(tauri.calls("github_issues").length).toBe(1);
    expect(tauri.calls("github_issue_detail").length).toBe(1);
  });
  it("creates an issue from the + of a repository and opens its pane", async () => {
    tauri.answer("github_issue_create", () => ({
      number: 1,
      url: "https://github.com/o/n/issues/1",
    }));
    const { fixture, el } = await open();
    q(el, "[data-testid=detail-close]")!.click();
    q(el, "[data-testid=new-issue]")!.click();
    fixture.detectChanges();
    expect(q(el, "[data-testid=form-create]")!.hasAttribute("disabled")).toBe(true);
    type(el, "[data-testid=form-title]", "  Brand new ");
    type(el, "[data-testid=form-body]", "Text");
    fixture.detectChanges();
    q(el, "[data-testid=form-create]")!.click();
    await settle(fixture);
    expect(tauri.calls("github_issue_create")).toEqual([
      {
        root: "/src/n",
        issue: { title: "Brand new", body: "Text", labels: [], milestone: null, parent: null },
      },
    ]);
    expect(q(el, "[data-testid=issue-form]")).toBeNull();
    expect(tauri.calls("github_issue_detail").at(-1)).toEqual({ root: "/src/n", number: 1 });
    expect(q(el, "[data-testid=issue-detail]")).not.toBeNull();
  });

  it("opens the sub-issue form from the pane and sends the parent", async () => {
    tauri.answer("github_issue_create", () => ({
      number: 2,
      url: "https://github.com/o/n/issues/2",
    }));
    const { fixture, el } = await open();
    q(el, "[data-testid=add-sub-issue]")!.click();
    fixture.detectChanges();
    expect(q(el, "[data-testid=issue-form] h3")!.textContent).toContain("sub-issue of #1");
    type(el, "[data-testid=form-title]", "Child");
    fixture.detectChanges();
    q(el, "[data-testid=form-create]")!.click();
    await settle(fixture);
    expect(
      (tauri.calls("github_issue_create")[0] as { issue: { parent: number } }).issue.parent,
    ).toBe(1);
  });

  it("keeps the form and notifies when the creation fails", async () => {
    tauri.answer("github_issue_create", () => {
      throw { kind: "other", message: "boom" };
    });
    const { fixture, el } = await open();
    q(el, "[data-testid=new-issue]")!.click();
    fixture.detectChanges();
    type(el, "[data-testid=form-title]", "Child");
    fixture.detectChanges();
    q(el, "[data-testid=form-create]")!.click();
    await settle(fixture);
    expect(q(el, "[data-testid=issue-form]")).not.toBeNull();
    expect(tauri.calls("github_issues").length).toBe(1);
  });

  it("changes the parent: search by number or title, then the backend call", async () => {
    tauri.answer("github_issue_change_parent", () => null);
    const { fixture, el } = await open();
    q(el, "[data-testid=change-parent]")!.click();
    fixture.detectChanges();
    const names = () =>
      [...el.querySelectorAll("[data-testid=parent-option]")].map((b) => b.textContent!.trim());
    expect(names()).toEqual(["#2 title 2"]);
    type(el, "[data-testid=parent-search]", "#9");
    fixture.detectChanges();
    expect(names()).toEqual([]);
    type(el, "[data-testid=parent-search]", "2");
    fixture.detectChanges();
    q(el, "[data-testid=parent-option]")!.click();
    await settle(fixture);
    expect(tauri.calls("github_issue_change_parent")).toEqual([
      { root: "/src/n", number: 1, oldParent: 9, newParent: 2 },
    ]);
    expect(q(el, "[data-testid=parent-picker]")).toBeNull();
  });
});

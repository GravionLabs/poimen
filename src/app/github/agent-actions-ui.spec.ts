// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { tauri } from "../core/tauri.mock";
import { UI_STORAGE, UiState } from "../core/ui-state";
import { AgentActionDialog } from "./agent-action-dialog/agent-action-dialog";
import { AgentActionMenu } from "./agent-action-menu/agent-action-menu";
import { AgentActions } from "./agent-actions";
import { GithubView } from "./github-view";
import { InboxSection } from "./inbox-section";
import { IssueDetailPane } from "./issue-detail/issue-detail";
import { IssuesSection } from "./issues-section";
import { MilestonesSection } from "./milestones-section";
import { PullDetailPane } from "./pull-detail/pull-detail";
import { GithubPulls, type Pull, type PullsResult } from "./pulls";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
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

const ACTIONS = {
  actions: [
    { id: "review", label: "Review", on: "pr", prompt: "p", source: "default" },
    { id: "wt", label: "In worktree", on: "pr", prompt: "p", run: "worktree", source: "user" },
    { id: "plan", label: "Plan", on: "issue", prompt: "p", source: "default" },
  ],
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

const q = (el: HTMLElement, sel: string) => el.querySelector<HTMLElement>(sel);

beforeEach(() => {
  localStorage.clear();
  tauri.reset();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [{ provide: UI_STORAGE, useValue: localStorage }],
  });
  tauri.answer("github_pulls", () => RESULT);
  tauri.answer("github_pull_detail", () => ({
    body: "",
    checks: [],
    reviews: [],
    linkedIssues: [],
  }));
  tauri.answer("github_issues", () => []);
  tauri.answer("get_agent_actions", () => ACTIONS);
  tauri.answer("snapshot", () => ({ spaces: [], tabs: [], terminals: [] }));
  tauri.answer("create_space", () => ({ space_id: "w1", created: true }));
  tauri.answer("terminal_create", () => "p1");
  tauri.answer("agent_start", () => undefined);
  tauri.answer("agent_action_plan", (args) =>
    (args?.["items"] as unknown[]).map(() => ({ kind: "claude", args: ["x"] })),
  );
  tauri.answer("get_harnesses", () => [
    { id: "claude", command: ["claude"], models: ["opus"], defaultModel: null, available: true },
  ]);
});

async function openDetail() {
  withTemplate(GithubView, "github-view.html");
  withTemplate(AgentActionMenu, "agent-action-menu/agent-action-menu.html");
  withTemplate(IssuesSection, "issues-section.html");
  withTemplate(InboxSection, "inbox-section.html");
  withTemplate(MilestonesSection, "milestones-section.html");
  withTemplate(PullDetailPane, "pull-detail/pull-detail.html");
  withTemplate(IssueDetailPane, "issue-detail/issue-detail.html");
  withTemplate(AgentActionDialog, "agent-action-dialog/agent-action-dialog.html");
  TestBed.inject(UiState).selectGithubPage("pulls");
  const fixture = TestBed.createComponent(GithubView);
  await fixture.whenStable();
  TestBed.inject(GithubPulls).scope.set("all");
  fixture.detectChanges();
  const el = fixture.nativeElement as HTMLElement;
  q(el, 'tr.row[data-key="o/n#1"]')!.click();
  await fixture.whenStable();
  fixture.detectChanges();
  return { fixture, el };
}

describe("agent actions in the pull request views", () => {
  it("shows the matching actions as buttons; worktree ones are enabled", async () => {
    const { el } = await openDetail();
    const buttons = [...el.querySelectorAll<HTMLButtonElement>("[data-testid=agent-action]")];
    expect(buttons.map((b) => [b.textContent!.trim(), b.disabled, b.title])).toEqual([
      ["Review", false, "Shift+click: choose harness and model"],
      ["In worktree", false, "Shift+click: choose harness and model"],
    ]);
  });

  it("a click starts the agent, Shift+click opens the dialog instead", async () => {
    const { fixture, el } = await openDetail();
    const review = q(el, "[data-testid=agent-action]")!;
    review.click();
    await fixture.whenStable();
    expect(tauri.calls("agent_start")).toHaveLength(1);
    expect(tauri.calls("agent_action_plan")[0]).toMatchObject({ harness: null, model: null });

    review.dispatchEvent(new MouseEvent("click", { shiftKey: true, bubbles: true }));
    await fixture.whenStable();
    expect(tauri.calls("agent_start")).toHaveLength(1);
    expect(TestBed.inject(AgentActions).dialog()?.action.id).toBe("review");
  });

  it("the row menu and the context menu list the actions for the row", async () => {
    const { fixture, el } = await openDetail();
    q(el, 'tr.row[data-key="o/n#2"] [data-testid=row-menu]')!.click();
    fixture.detectChanges();
    const items = () => [...el.querySelectorAll("[data-testid=menu-action]")];
    expect(items().map((i) => i.textContent!.trim())).toEqual(["Review", "In worktree"]);
    q(el, "[data-testid=menu-action]")!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(q(el, "[data-testid=agent-action-menu]")).toBeNull();
    expect(tauri.calls("agent_action_plan")[0]).toMatchObject({ items: [{ nr: 2 }] });

    q(el, 'tr.row[data-key="o/n#1"]')!.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
    fixture.detectChanges();
    expect(items()).toHaveLength(2);
  });

  it("with several rows selected the menu starts one agent per row", async () => {
    const { fixture, el } = await openDetail();
    const pulls = TestBed.inject(GithubPulls);
    pulls.select("o/n#1");
    pulls.select("o/n#2");
    fixture.detectChanges();
    q(el, 'tr.row[data-key="o/n#2"] [data-testid=row-menu]')!.click();
    fixture.detectChanges();
    q(el, "[data-testid=menu-action]")!.click();
    await fixture.whenStable();
    const items = (tauri.calls("agent_action_plan")[0] as { items: { nr: number }[] }).items;
    expect(items.map((i) => i.nr)).toEqual([1, 2]);
  });

  it("the palette offers the actions for the selection, titled with the numbers", async () => {
    const { fixture } = await openDetail();
    const actions = TestBed.inject(AgentActions);
    expect(actions.entries().map((e) => e.title)).toEqual([
      "GitHub: Review #1",
      "GitHub: In worktree #1",
    ]);
    const pulls = TestBed.inject(GithubPulls);
    pulls.select("o/n#1");
    pulls.select("o/n#2");
    expect(actions.entries().map((e) => e.title)).toEqual([
      "GitHub: Review #1, #2",
      "GitHub: In worktree #1, #2",
    ]);
    await actions.runCurrent("review");
    fixture.detectChanges();
    expect(tauri.calls("agent_start")).toHaveLength(2);
    expect(await actions.runCurrent("nope")).toBeNull();
  });

  it("the dialog is prefilled, offers the models of the harness and starts with the choice", async () => {
    const { fixture, el } = await openDetail();
    const dialogFixture = TestBed.createComponent(AgentActionDialog);
    q(el, "[data-testid=agent-action]")!.dispatchEvent(
      new MouseEvent("click", { shiftKey: true, bubbles: true }),
    );
    await fixture.whenStable();
    dialogFixture.detectChanges();
    const d = dialogFixture.nativeElement as HTMLElement;
    expect(q(d, "[data-testid=dialog-harness]")!.textContent).toContain("claude");
    expect(q(d, "[data-testid=dialog-model]")!.textContent).toContain("opus");
    const model = q(d, "[data-testid=dialog-model]") as HTMLSelectElement;
    model.value = "opus";
    model.dispatchEvent(new Event("change"));
    q(d, "[data-testid=dialog-start]")!.click();
    await fixture.whenStable();
    expect(tauri.calls("agent_action_plan")[0]).toMatchObject({ harness: "claude", model: "opus" });
    expect(tauri.calls("agent_start")).toHaveLength(1);
  });
});

// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { parseAgentActions, type AgentAction } from "../core/settings";
import { SpaceActions } from "../core/space-actions";
import { tauri } from "../core/tauri.mock";
import { UI_STORAGE } from "../core/ui-state";
import {
  AgentActions,
  DIFF_ONLY_SUFFIX,
  POST_REVIEW_SUFFIX,
  reviewSuffix,
  issueTarget,
  matches,
  pullTarget,
  summaryText,
  worktreeLabel,
  type ActionTarget,
} from "./agent-actions";
import type { Issue } from "./issues";
import type { PullRow } from "./pulls";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

function action(over: Record<string, unknown> = {}): AgentAction {
  return parseAgentActions({
    actions: [
      { id: "refine", label: "Refine", on: "issue", prompt: "p", source: "default", ...over },
    ],
  })[0];
}

function issue(number: number, over: Partial<Issue> = {}): Issue {
  return {
    number,
    title: `title ${number}`,
    url: `https://github.com/o/n/issues/${number}`,
    state: "OPEN",
    labels: ["pbi"],
    assignees: [],
    milestone: null,
    parent: null,
    subTotal: 0,
    subCompleted: 0,
    prs: [],
    ...over,
  };
}

function pull(number: number, over: Partial<PullRow> = {}): PullRow {
  return {
    number,
    title: `pr ${number}`,
    url: `https://github.com/o/n/pull/${number}`,
    author: "jk",
    isDraft: false,
    additions: 1,
    deletions: 1,
    createdAt: "2026-01-01T00:00:00Z",
    headRef: `feat/${number}-x`,
    isCrossRepository: false,
    baseRef: "main",
    reviewDecision: null,
    reviewRequests: [],
    checks: { state: "passing", passed: 1, failed: 0, pending: 0 },
    repo: "o/n",
    root: "/src/n",
    key: `o/n#${number}`,
    ...over,
  };
}

const ISSUE_T = (n: number, over: Partial<Issue> = {}): ActionTarget =>
  issueTarget("o/n", "/src/n", issue(n, over));

describe("matches", () => {
  it("needs the kind to be the same", () => {
    expect(matches(action(), ISSUE_T(1))).toBe(true);
    expect(matches(action({ on: "pr" }), ISSUE_T(1))).toBe(false);
  });

  it("any level, any label (case-insensitive) and no children", () => {
    const a = action({ when: { level: ["epic", "feature"], noChildren: true } });
    expect(matches(a, ISSUE_T(1, { labels: ["Feature"] }))).toBe(true);
    expect(matches(a, ISSUE_T(1, { labels: ["pbi"] }))).toBe(false);
    expect(matches(a, ISSUE_T(1, { labels: ["feature"], subTotal: 2 }))).toBe(false);
    const l = action({ when: { label: ["needs-refinement"] } });
    expect(matches(l, ISSUE_T(1, { labels: ["Needs-Refinement"] }))).toBe(true);
    expect(matches(l, ISSUE_T(1, { labels: [] }))).toBe(false);
  });

  it("state, checks and review of a pull request", () => {
    const failed = action({ on: "pr", when: { checks: "failed" } });
    const bad = pullTarget(
      pull(1, { checks: { state: "failing", passed: 0, failed: 1, pending: 0 } }),
    );
    expect(matches(failed, bad)).toBe(true);
    expect(matches(failed, pullTarget(pull(2)))).toBe(false);
    const review = action({ on: "pr", when: { review: "CHANGES_REQUESTED", state: ["open"] } });
    expect(matches(review, pullTarget(pull(3, { reviewDecision: "CHANGES_REQUESTED" })))).toBe(
      true,
    );
    expect(matches(review, pullTarget(pull(3, { reviewDecision: "APPROVED" })))).toBe(false);
    const closed = action({ when: { state: ["closed"] } });
    expect(matches(closed, ISSUE_T(1))).toBe(false);
    expect(matches(closed, ISSUE_T(1, { state: "CLOSED" }))).toBe(true);
  });

  it("an action without conditions matches every row of its kind", () => {
    expect(matches(action({ when: {} }), ISSUE_T(9, { labels: [] }))).toBe(true);
  });
});

describe("summaryText", () => {
  it("lists started and failed with the reason", () => {
    expect(
      summaryText("Review", { started: [1, 2], failed: [{ numbers: [3], reason: "boom" }] }),
    ).toBe("Review: started #1, #2; failed #3 (boom)");
  });
});

const LAUNCH = (nr: number) => ({ kind: "claude", args: [`prompt ${nr}`] });

describe("reviewSuffix", () => {
  it("is empty without extras", () => {
    expect(reviewSuffix(false, false)).toBe("");
  });
  it("diff only tells the agent to read gh pr diff and not to change files", () => {
    expect(reviewSuffix(true, false)).toBe(DIFF_ONLY_SUFFIX);
    expect(DIFF_ONLY_SUFFIX).toBe(
      "Read the changes with `gh pr diff {nr}` and do not change any files.",
    );
  });
  it("posting adds the gh pr review instruction, after the diff-only one", () => {
    expect(reviewSuffix(false, true)).toBe(
      "When you are done, post your review on GitHub with `gh pr review {nr} --comment`.",
    );
    expect(reviewSuffix(true, true)).toBe(`${DIFF_ONLY_SUFFIX}\n\n${POST_REVIEW_SUFFIX}`);
  });
});

describe("worktreeLabel", () => {
  it("prefixes pull requests and cuts long titles to 40 characters", () => {
    expect(worktreeLabel(ISSUE_T(5))).toBe("#5 title 5");
    expect(worktreeLabel(pullTarget(pull(9)))).toBe("PR #9 pr 9");
    const long = worktreeLabel(ISSUE_T(5, { title: "x".repeat(80) }));
    expect([...long]).toHaveLength(40);
    expect(long.endsWith("…")).toBe(true);
    expect(worktreeLabel(ISSUE_T(5, { title: " " }))).toBe("#5");
  });
});

describe("AgentActions", () => {
  let actions: AgentActions;
  let spaces: SpaceActions;

  beforeEach(() => {
    tauri.reset();
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: null }] });
    tauri.answer("snapshot", () => ({ spaces: [], tabs: [], terminals: [] }));
    tauri.answer("create_space", () => ({ workspace_id: "w1", created: true }));
    let pane = 0;
    tauri.answer("terminal_create", () => `p${++pane}`);
    tauri.answer("agent_start", () => undefined);
    tauri.answer("agent_action_plan", (args) =>
      (args?.["items"] as { nr: number }[]).map((i) => LAUNCH(i.nr)),
    );
    tauri.answer("get_harnesses", () => [
      {
        id: "claude",
        command: ["claude"],
        models: ["opus", "sonnet"],
        defaultModel: null,
        available: true,
      },
      {
        id: "copilot",
        command: ["copilot"],
        models: ["auto"],
        defaultModel: null,
        available: false,
      },
    ]);
    actions = TestBed.inject(AgentActions);
    spaces = TestBed.inject(SpaceActions);
  });

  const review = (): AgentAction => action({ id: "review", label: "Review", on: "pr" });

  it("loads the actions of a repository and offers the matching ones", async () => {
    tauri.answer("get_agent_actions", () => ({
      actions: [
        {
          id: "refine",
          label: "Refine",
          on: "issue",
          prompt: "p",
          source: "default",
          when: { label: ["bug"] },
        },
        { id: "plan", label: "Plan", on: "issue", prompt: "p", source: "user" },
        {
          id: "wt",
          label: "In worktree",
          on: "issue",
          prompt: "p",
          run: "worktree",
          source: "user",
        },
        { id: "ask", label: "Ask", on: "issue", prompt: "p", run: "ask", source: "user" },
        { id: "review", label: "Review", on: "pr", prompt: "p", source: "default" },
      ],
    }));
    await actions.load("/src/n");
    expect(tauri.calls("get_agent_actions")).toEqual([{ root: "/src/n" }]);
    const offered = actions.offered([ISSUE_T(1, { labels: ["pbi"] })]);
    expect(offered.map((o) => [o.action.id, o.enabled, o.reason])).toEqual([
      ["plan", true, null],
      ["wt", true, null],
      ["ask", true, null],
    ]);
    expect(actions.offered([ISSUE_T(2, { labels: ["bug"] })])[0].action.id).toBe("refine");
    // A mixed selection of kinds has no common action.
    expect(actions.offered([ISSUE_T(1), pullTarget(pull(2))])).toEqual([]);
    expect(actions.offered([])).toEqual([]);
  });

  it("a click starts one agent in a new tab of the repository space", async () => {
    const summary = await actions.request(review(), [pullTarget(pull(7))], false);
    expect(tauri.calls("agent_action_plan")).toEqual([
      {
        root: "/src/n",
        id: "review",
        repo: "o/n",
        items: [
          {
            nr: 7,
            title: "pr 7",
            url: "https://github.com/o/n/pull/7",
            branch: "feat/7-x",
            base: "main",
          },
        ],
        harness: null,
        model: null,
      },
    ]);
    expect(tauri.calls("create_space")).toEqual([
      { cwd: "/src/n", label: null, decision: null, command: null },
    ]);
    expect(tauri.calls("terminal_create")).toEqual([{ workspaceId: "w1", cwd: "/src/n" }]);
    expect(tauri.calls("agent_start")).toEqual([
      { paneId: "p1", kind: "claude", name: "Review #7", args: ["prompt 7"] },
    ]);
    expect(summary).toEqual({ started: [7], failed: [] });
    expect(spaces.warning()).toBe("Review: started #7");
  });

  it("batch each starts one agent per item", async () => {
    const summary = await actions.request(
      review(),
      [pullTarget(pull(1)), pullTarget(pull(2))],
      false,
    );
    expect(tauri.calls("agent_start")).toEqual([
      { paneId: "p1", kind: "claude", name: "Review #1", args: ["prompt 1"] },
      { paneId: "p2", kind: "claude", name: "Review #2", args: ["prompt 2"] },
    ]);
    expect(summary?.started).toEqual([1, 2]);
  });

  it("batch once starts one agent for all items", async () => {
    tauri.answer("agent_action_plan", () => [{ kind: "claude", args: ["do 1 2"] }]);
    const once = action({ id: "review", label: "Review", on: "pr", batch: "once" });
    const summary = await actions.request(once, [pullTarget(pull(1)), pullTarget(pull(2))], false);
    expect(tauri.calls("agent_start")).toEqual([
      { paneId: "p1", kind: "claude", name: "Review #1 #2", args: ["do 1 2"] },
    ]);
    expect(summary).toEqual({ started: [1, 2], failed: [] });
  });

  it("a failed start does not stop the others and the summary lists both", async () => {
    tauri.answer("agent_start", (args) => {
      if (args?.["paneId"] === "p2") throw "claude did not start";
      return undefined;
    });
    const rows = [1, 2, 3].map((n) => pullTarget(pull(n)));
    const summary = await actions.request(review(), rows, false);
    expect(tauri.calls("agent_start")).toHaveLength(3);
    expect(summary).toEqual({
      started: [1, 3],
      failed: [{ numbers: [2], reason: "claude did not start" }],
    });
    expect(spaces.warning()).toBe("Review: started #1, #3; failed #2 (claude did not start)");
  });

  it("a terminal that cannot be created is a failure of that item only", async () => {
    let n = 0;
    tauri.answer("terminal_create", () => (++n === 1 ? null : `p${n}`));
    const summary = await actions.request(
      review(),
      [pullTarget(pull(1)), pullTarget(pull(2))],
      false,
    );
    expect(summary?.started).toEqual([2]);
    expect(summary?.failed[0].numbers).toEqual([1]);
  });

  it("a failed plan starts nothing and says why", async () => {
    tauri.answer("agent_action_plan", () => {
      throw 'unknown harness "x"';
    });
    expect(await actions.request(review(), [pullTarget(pull(1))], false)).toBeNull();
    expect(tauri.calls("create_space")).toEqual([]);
    expect(spaces.warning()).toBe('Review: unknown harness "x"');
  });

  describe("worktree runs", () => {
    const wt = (over: Record<string, unknown> = {}) =>
      action({ run: "worktree", id: "wt", label: "Work", ...over });
    const prWt = () => wt({ on: "pr" });

    beforeEach(() => {
      tauri.answer("get_task_modes", () => ({
        worktreeDir: "..",
        modes: [
          { name: "plain", worktree: false, branch: "" },
          { name: "agent", worktree: true, branch: "feat/{nr}" },
        ],
      }));
      tauri.answer("task_worktree", () => ({ path: "/src/n-1", branch: "feat/1", created: true }));
      tauri.answer("pr_worktree", () => ({ path: "/src/n-pr7", branch: "pr/7", created: true }));
      tauri.answer("snapshot", () => ({
        workspaces: [],
        tabs: [],
        panes: [{ pane_id: "root1", workspace_id: "w1" }],
      }));
    });

    it("an issue gets the task worktree, a space on it and the agent in its root pane", async () => {
      const summary = await actions.request(wt(), [ISSUE_T(1)], false);
      expect(tauri.calls("task_worktree")).toEqual([
        {
          repoDir: "/src/n",
          issue: { nr: 1, title: "title 1", url: "https://github.com/o/n/issues/1" },
          mode: "agent",
        },
      ]);
      expect(tauri.calls("create_space")).toEqual([
        { cwd: "/src/n-1", label: "#1 title 1", decision: null, command: null },
      ]);
      expect(tauri.calls("terminal_create")).toEqual([]);
      expect(tauri.calls("agent_start")).toEqual([
        { paneId: "root1", kind: "claude", name: "Work #1", args: ["prompt 1"] },
      ]);
      expect(summary).toEqual({ started: [1], failed: [] });
      expect(spaces.warning()).toBe("Work: started #1");
    });

    it("a pull request gets the PR worktree and the label PR #<nr> <title>", async () => {
      const summary = await actions.request(prWt(), [pullTarget(pull(7))], false);
      expect(tauri.calls("pr_worktree")).toEqual([
        { repoDir: "/src/n", nr: 7, headRef: "feat/7-x" },
      ]);
      expect(tauri.calls("create_space")).toEqual([
        { cwd: "/src/n-pr7", label: "PR #7 pr 7", decision: null, command: null },
      ]);
      expect(tauri.calls("agent_start")[0]).toMatchObject({ paneId: "root1" });
      expect(summary).toEqual({ started: [7], failed: [] });
    });

    it("an existing space is focused and the agent starts in a new tab there", async () => {
      tauri.answer("create_space", () => ({ workspace_id: "w9", created: false }));
      const summary = await actions.request(wt(), [ISSUE_T(1)], false);
      expect(tauri.calls("terminal_create")).toEqual([{ workspaceId: "w9", cwd: "/src/n-1" }]);
      expect(tauri.calls("agent_start")).toEqual([
        { paneId: "p1", kind: "claude", name: "Work #1", args: ["prompt 1"] },
      ]);
      expect(summary?.started).toEqual([1]);
    });

    it("a failed worktree names the step and creates no space", async () => {
      tauri.answer("task_worktree", () => {
        throw "path exists";
      });
      const summary = await actions.request(wt(), [ISSUE_T(1)], false);
      expect(tauri.calls("create_space")).toEqual([]);
      expect(tauri.calls("agent_start")).toEqual([]);
      expect(summary).toEqual({
        started: [],
        failed: [{ numbers: [1], reason: "worktree failed: path exists" }],
      });
      expect(spaces.warning()).toBe("Work: failed #1 (worktree failed: path exists)");
    });

    it("an issue without a worktree task mode fails in the worktree step", async () => {
      tauri.answer("get_task_modes", () => ({ worktreeDir: "..", modes: [] }));
      const summary = await actions.request(wt(), [ISSUE_T(1)], false);
      expect(summary?.failed[0].reason).toContain("worktree failed: no task mode");
      expect(tauri.calls("create_space")).toEqual([]);
    });

    it("a failed space names the step and starts no agent", async () => {
      tauri.answer("create_space", () => {
        throw "no repository";
      });
      const summary = await actions.request(prWt(), [pullTarget(pull(7))], false);
      expect(summary?.failed).toEqual([{ numbers: [7], reason: "space failed: no repository" }]);
      expect(tauri.calls("agent_start")).toEqual([]);
    });

    it("a failed agent start names the step", async () => {
      tauri.answer("agent_start", () => {
        throw "claude missing";
      });
      const summary = await actions.request(wt(), [ISSUE_T(1)], false);
      expect(summary).toEqual({
        started: [],
        failed: [{ numbers: [1], reason: "agent failed: claude missing" }],
      });
    });

    it("each target gets its own worktree and space; one failure leaves the others", async () => {
      tauri.answer("task_worktree", (args) => {
        const nr = (args?.["issue"] as { nr: number }).nr;
        if (nr === 1) throw "boom";
        return { path: `/src/n-${nr}`, branch: `feat/${nr}`, created: true };
      });
      const summary = await actions.request(wt(), [ISSUE_T(1), ISSUE_T(2)], false);
      expect(tauri.calls("create_space")).toEqual([
        { cwd: "/src/n-2", label: "#2 title 2", decision: null, command: null },
      ]);
      expect(summary).toEqual({
        started: [2],
        failed: [{ numbers: [1], reason: "worktree failed: boom" }],
      });
    });
  });

  describe("ask", () => {
    const ask = (over: Record<string, unknown> = {}) =>
      action({ run: "ask", id: "ask", label: "Ask", ...over });

    beforeEach(() => {
      tauri.answer("get_task_modes", () => ({
        worktreeDir: "..",
        modes: [{ name: "agent", worktree: true, branch: "feat/{nr}" }],
      }));
      tauri.answer("task_worktree", () => ({ path: "/src/n-1", branch: "feat/1", created: true }));
    });

    it("always opens the dialog, also without Shift, and starts nothing yet", async () => {
      expect(await actions.request(ask(), [ISSUE_T(1)], false)).toBeNull();
      expect(actions.dialog()?.action.id).toBe("ask");
      expect(actions.dialog()?.where).toBe("root");
      expect(tauri.calls("agent_action_plan")).toEqual([]);
      expect(tauri.calls("agent_start")).toEqual([]);
    });

    it("choosing repository root starts at the root", async () => {
      await actions.request(ask(), [ISSUE_T(1)], false);
      await actions.confirmDialog();
      expect(tauri.calls("task_worktree")).toEqual([]);
      expect(tauri.calls("create_space")).toEqual([
        { cwd: "/src/n", label: null, decision: null, command: null },
      ]);
      expect(tauri.calls("agent_start")).toHaveLength(1);
    });

    it("choosing worktree creates the worktree and its space", async () => {
      await actions.request(ask(), [ISSUE_T(1)], false);
      actions.setDialog({ where: "worktree" });
      await actions.confirmDialog();
      expect(tauri.calls("task_worktree")).toHaveLength(1);
      expect(tauri.calls("create_space")[0]).toMatchObject({ cwd: "/src/n-1" });
    });
  });

  describe("review extras", () => {
    const rev = (): AgentAction => action({ id: "review", label: "Review", on: "pr", run: "ask" });
    const rows = (...n: number[]): ActionTarget[] => n.map((x) => pullTarget(pull(x)));

    it("the dialog defaults to diff only without posting", async () => {
      await actions.request(rev(), rows(1), false);
      expect(actions.dialog()).toMatchObject({ where: "diff", post: false });
    });

    it("diff only starts a tab review #<nr> at the root with the fixed suffix", async () => {
      await actions.request(rev(), rows(5), false);
      await actions.confirmDialog();
      expect(tauri.calls("agent_action_plan")[0]).toMatchObject({ suffix: DIFF_ONLY_SUFFIX });
      expect(tauri.calls("task_worktree")).toEqual([]);
      expect(tauri.calls("pr_worktree")).toEqual([]);
      expect(tauri.calls("terminal_create")).toEqual([{ workspaceId: "w1", cwd: "/src/n" }]);
      expect(tauri.calls("agent_start")[0]).toMatchObject({ name: "review #5" });
    });

    it("posting appends the gh pr review instruction", async () => {
      await actions.request(rev(), rows(5), false);
      actions.setDialog({ post: true });
      await actions.confirmDialog();
      expect(tauri.calls("agent_action_plan")[0]).toMatchObject({
        suffix: reviewSuffix(true, true),
      });
    });

    it("worktree mode has no diff-only suffix", async () => {
      tauri.answer("pr_worktree", () => ({ path: "/src/n-pr5" }));
      await actions.request(rev(), rows(5), false);
      actions.setDialog({ where: "worktree" });
      await actions.confirmDialog();
      expect(tauri.calls("agent_action_plan")[0]?.["suffix"]).toBeUndefined();
      expect(tauri.calls("pr_worktree")).toHaveLength(1);
    });

    it("a multi-select starts one agent per PR and one failure leaves the others", async () => {
      tauri.answer("agent_start", (args) => {
        if (args?.["paneId"] === "p2") throw "claude did not start";
        return undefined;
      });
      await actions.request(rev(), rows(1, 2, 3), false);
      const summary = await actions.confirmDialog();
      expect(tauri.calls("agent_start").map((c) => c?.["name"])).toEqual([
        "review #1",
        "review #2",
        "review #3",
      ]);
      expect(summary).toEqual({
        started: [1, 3],
        failed: [{ numbers: [2], reason: "claude did not start" }],
      });
      expect(spaces.warning()).toBe("Review: started #1, #3; failed #2 (claude did not start)");
    });

    it("other actions get no suffix", async () => {
      await actions.run(review(), rows(1), { harness: "claude", model: "", where: "root" });
      expect(tauri.calls("agent_action_plan")[0]?.["suffix"]).toBeUndefined();
    });
  });

  it("Shift opens the dialog prefilled instead of starting", async () => {
    const withModel = action({ id: "review", label: "Review", on: "pr", model: "sonnet" });
    expect(await actions.request(withModel, [pullTarget(pull(4))], true)).toBeNull();
    expect(tauri.calls("agent_action_plan")).toEqual([]);
    const d = actions.dialog();
    expect(d?.harness).toBe("claude");
    expect(d?.model).toBe("sonnet");
    expect(actions.harnesses().map((h) => h.id)).toEqual(["claude"]);

    actions.setDialog({ harness: "claude", model: "opus" });
    await actions.confirmDialog();
    expect(actions.dialog()).toBeNull();
    expect(tauri.calls("agent_action_plan")[0]).toMatchObject({ harness: "claude", model: "opus" });
    expect(tauri.calls("agent_start")).toHaveLength(1);
  });

  it("the dialog starts with the harness of the action when it has one", async () => {
    const own = action({ id: "review", on: "pr", harness: "copilot" });
    await actions.request(own, [pullTarget(pull(4))], true);
    expect(actions.dialog()?.harness).toBe("copilot");
    actions.cancelDialog();
    expect(actions.dialog()).toBeNull();
    expect(tauri.calls("agent_start")).toEqual([]);
  });
});

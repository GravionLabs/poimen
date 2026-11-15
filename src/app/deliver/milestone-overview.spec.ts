import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { Issue, RepoIssues } from "../github/issues";
import type { Milestone, RepoMilestones } from "../github/milestones";
import { cardsOf, groupRuns } from "./deliver-status";
import {
  buildOverview,
  githubProgress,
  groupsOf,
  issueState,
  milestoneIssues,
  nextIssues,
  runContext,
  stateCounts,
  subjectIssue,
  subjectMatches,
  taskTitle,
  titleKey,
  type RunContext,
} from "./milestone-overview";

const FOUR = JSON.parse(
  readFileSync("src/app/deliver/fixtures/runs-milestone.json", "utf8"),
) as Record<string, unknown>[];
/** Shortly after the last event of the newest run of the fixture. */
const NOW = Date.parse("2026-11-08T09:35:00Z");
const URL = "https://github.com/GravionLabs/demo/milestone/14";

const none: RunContext = { working: new Set(), needsYou: new Set() };
const issue = (number: number, extra: Partial<Issue> = {}): Issue => ({
  number,
  title: `[PBI] Issue ${number}`,
  url: `https://github.com/GravionLabs/demo/issues/${number}`,
  state: "OPEN",
  labels: ["pbi"],
  assignees: [],
  milestone: "M014.1 Runs view",
  parent: null,
  subTotal: 0,
  subCompleted: 0,
  prs: [],
  ...extra,
});
const pr = (state: "OPEN" | "MERGED", n = 900) => ({
  number: n,
  url: `https://github.com/GravionLabs/demo/pull/${n}`,
  state,
  checks: { state: "none" } as never,
});

describe("issueState (#585)", () => {
  it("is done for a closed issue, whatever else it has", () => {
    expect(issueState(issue(1, { state: "CLOSED", labels: ["needs-decision"] }), none)).toBe(
      "done",
    );
  });

  it.each(["needs-decision", "interactive", "release-test"])(
    "is needs-you for the label %s",
    (label) => {
      expect(issueState(issue(1, { labels: ["pbi", label] }), none)).toBe("needs-you");
    },
  );

  it("is needs-you when a run asks about it, before in-progress", () => {
    const runs = { working: new Set([2]), needsYou: new Set([2]) };
    expect(issueState(issue(2), runs)).toBe("needs-you");
  });

  it("is in-progress for the issue a run works on, an open pull request or an assignee", () => {
    expect(issueState(issue(3), { ...none, working: new Set([3]) })).toBe("in-progress");
    expect(issueState(issue(4, { prs: [pr("OPEN")] }), none)).toBe("in-progress");
    expect(issueState(issue(5, { assignees: ["me"] }), none)).toBe("in-progress");
    expect(issueState(issue(6, { prs: [pr("MERGED")] }), none)).toBe("next");
  });

  it("is blocked by open dependencies, unless a run works on it", () => {
    expect(issueState(issue(7, { blockedBy: [3] }), none)).toBe("blocked");
    expect(issueState(issue(7, { blockedBy: [3] }), { ...none, working: new Set([7]) })).toBe(
      "in-progress",
    );
  });

  it("is next when open and ready", () => {
    expect(issueState(issue(8), none)).toBe("next");
  });
});

describe("milestone issues", () => {
  const feature = issue(10, { title: "[Feature] F", labels: ["feature"] });
  const list = [
    issue(13),
    issue(11, { parent: { number: 10, repo: "GravionLabs/demo" } }),
    feature,
    issue(12, { labels: ["bug"], state: "CLOSED", prs: [pr("MERGED", 55)] }),
    issue(14, { milestone: "M015" }),
    issue(15, { labels: ["epic"] }),
    issue(9, { labels: ["pbi", "needs-decision"] }),
  ];

  it("lists PBIs and Bugs of the milestone, a Feature with its sub-issues as a group", () => {
    const rows = milestoneIssues(list, "M014.1 Runs view", none);
    expect(rows.map((r) => r.issue.number)).toEqual([9, 10, 12, 13]);
    expect(rows[1]!.children.map((c) => c.issue.number)).toEqual([11]);
    expect(rows[2]!.pr).toEqual({ number: 55, url: expect.stringContaining("/pull/55") });
  });

  it("counts a Feature through its sub-issues and orders the next ones by number", () => {
    const rows = milestoneIssues(list, "M014.1 Runs view", none);
    expect(stateCounts(rows)).toEqual({
      "in-progress": 0,
      "needs-you": 1,
      blocked: 0,
      next: 2,
      done: 1,
    });
    expect(nextIssues(rows)).toEqual([11, 13]);
  });
});

describe("runs on a milestone", () => {
  const groups = groupRuns(cardsOf("/r", { kind: "events", runs: FOUR }));

  it("reads the issue number of a task subject", () => {
    expect(subjectIssue({ id: "GravionLabs/demo#101", title: "x", url: null })).toBe(101);
    expect(subjectIssue({ id: "t", title: "#7 Group", url: null })).toBe(7);
    expect(subjectIssue({ id: "t", title: "x", url: "https://github.com/o/n/issues/9" })).toBe(9);
    expect(subjectIssue({ id: "t", title: "x", url: null })).toBeNull();
  });

  it("matches a subject by URL, else by the M-number of its id or title", () => {
    const subject = { kind: "github.milestone", id: "x", title: "M014.1 Runs view", url: URL };
    expect(subjectMatches(subject, { title: "other", url: URL })).toBe(true);
    expect(subjectMatches(subject, { title: "M014.1 Runs view", url: "https://x/other" })).toBe(
      false,
    );
    const old = { ...subject, url: null, id: "M014.1" };
    expect(subjectMatches(old, { title: "M014.1 Runs view", url: URL })).toBe(true);
    expect(subjectMatches(old, { title: "M014.2", url: "" })).toBe(false);
    expect(subjectMatches({ ...old, kind: "github.issue" }, { title: "M014.1", url: "" })).toBe(
      false,
    );
    expect(titleKey("M014.1 Runs view")).toBe("m014.1");
    expect(titleKey("1.2.0")).toBe("1.2.0");
  });

  it("takes the issues a running run works on from its open tasks", () => {
    const running = JSON.parse(JSON.stringify(FOUR[2])) as Record<string, unknown>;
    running["status"] = "running";
    const g = groupRuns(cardsOf("/r", { kind: "events", runs: [running] }));
    const ctx = runContext(g[0]!.runs, Date.parse(running["updated"] as string) + 1000);
    expect([...ctx.working]).toEqual([104]);
    expect([...ctx.needsYou]).toEqual([104]);
    // A finished or abandoned run works on nothing.
    expect(
      runContext(groupRuns(cardsOf("/r", { kind: "events", runs: FOUR }))[0]!.runs, NOW).working
        .size,
    ).toBe(0);
    expect(groupsOf(groups, "/r", { title: "M014.1", url: URL })).toHaveLength(1);
    expect(groupsOf(groups, "/elsewhere", { title: "M014.1", url: URL })).toHaveLength(0);
  });
});

describe("buildOverview", () => {
  const milestone = (extra: Partial<Milestone> = {}): Milestone => ({
    number: 14,
    title: "M014.1 Runs view",
    url: URL,
    dueOn: null,
    description: "",
    open: 3,
    closed: 1,
    current: true,
    ...extra,
  });
  const groups = groupRuns(cardsOf("/r", { kind: "events", runs: FOUR }));
  const ok = (milestones: Milestone[]): RepoMilestones => ({
    root: "/r",
    repo: "GravionLabs/demo",
    status: "ok",
    milestones,
  });
  const issues = (list: Issue[]): RepoIssues => ({
    root: "/r",
    repo: "GravionLabs/demo",
    status: "ok",
    issues: list,
    truncated: false,
  });

  it("lists the current milestone first with its states and the newest run", () => {
    const [repo] = buildOverview({
      milestones: [ok([milestone({ title: "M015", url: "u", current: false }), milestone()])],
      issues: [issues([issue(101), issue(102, { blockedBy: [101] })])],
      closed: [issues([issue(100, { state: "CLOSED" })])],
      groups,
      now: NOW,
    });
    expect(repo!.rows.map((r) => r.title)).toEqual(["M014.1 Runs view", "M015"]);
    const [current, other] = repo!.rows;
    expect(current!.counts).toMatchObject({ next: 1, blocked: 1, done: 1 });
    expect(current!.run?.status.run).toBe("20261108-080000-loop");
    expect(other!.run).toBeUndefined();
    expect(repo!.error).toBeNull();
  });

  it("leaves out runs of a milestone GitHub no longer lists open", () => {
    const [repo] = buildOverview({
      milestones: [ok([milestone({ title: "M020", url: "u2", current: true })])],
      issues: [],
      closed: [],
      groups,
      now: NOW,
    });
    expect(repo!.rows.map((r) => r.title)).toEqual(["M020"]);
  });

  it("still lists the milestones of the run subjects when GitHub fails", () => {
    const out = buildOverview({
      milestones: [
        { root: "/r", repo: "GravionLabs/demo", status: "error", error: { kind: "offline" } },
      ],
      issues: [],
      closed: [],
      groups,
      now: NOW,
    });
    expect(out[0]!.error).toEqual({ kind: "offline" });
    expect(out[0]!.rows).toHaveLength(1);
    expect(out[0]!.rows[0]).toMatchObject({ milestone: null, title: "M014.1 Runs view" });
    expect(out[0]!.rows[0]!.run).toBeDefined();
  });

  it("lists the milestones of repositories GitHub was not asked about and of machines", () => {
    const remote = groupRuns(cardsOf("/srv", { kind: "events", runs: FOUR }, "box"));
    const out = buildOverview({
      milestones: [],
      issues: [],
      closed: [],
      groups: [...groups, ...remote],
      now: NOW,
    });
    expect(out.map((r) => [r.root, r.machine])).toEqual([
      ["/r", undefined],
      ["/srv", "box"],
    ]);
  });

  it("shows the issue title for a bare #<nr> task and keeps the number without GitHub", () => {
    const task = { subject: { kind: "github.issue", id: "x", title: "#12", url: null } };
    expect(taskTitle(task, [issue(12, { title: "Group runs" })])).toBe("#12 Group runs");
    expect(taskTitle(task, [])).toBe("#12");
    expect(taskTitle({ subject: { ...task.subject, title: "#12 Own" } }, [issue(12)])).toBe(
      "#12 Own",
    );
  });

  it("takes the progress from GitHub's counts", () => {
    expect(githubProgress(milestone())).toEqual({ done: 1, total: 4, label: "closed" });
    expect(githubProgress(undefined)).toBeNull();
  });
});

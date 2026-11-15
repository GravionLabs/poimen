import { issueLevel, type Issue, type RepoIssues } from "../github/issues";
import type { Milestone, RepoMilestones } from "../github/milestones";
import type { GhError } from "../github/pulls";
import {
  runState,
  runningRun,
  type GroupRun,
  type OpenTask,
  type RunGroup,
  type Subject,
} from "./deliver-status";

/** The state of an issue in the overview (#585). */
export type IssueState = "in-progress" | "needs-you" | "blocked" | "next" | "done";

/** Labels that ask for the user: the issue is not for an unattended run. */
export const NEEDS_YOU_LABELS: readonly string[] = [
  "needs-decision",
  "interactive",
  "release-test",
];

/** What the runs say about the issues of a milestone, by issue number. */
export interface RunContext {
  /** Issues an open task of a running run works on. */
  working: ReadonlySet<number>;
  /** Issues a run asks the user about (an attention that needs you, a task of a needs-you run). */
  needsYou: ReadonlySet<number>;
}

/** The issue number a task names (see {@link subjectIssue}). */
export const issueSubjectNumber = (task: Pick<OpenTask, "subject">): number | null =>
  subjectIssue(task.subject);

/** The issue number a run subject names: `owner/repo#12`, `#12`, or `/issues/12` in the URL. */
export function subjectIssue(subject: Pick<Subject, "id" | "title" | "url">): number | null {
  const found =
    /#(\d+)$/.exec(subject.id) ??
    /^#(\d+)\b/.exec(subject.title) ??
    /\/issues\/(\d+)\/?$/.exec(subject.url ?? "");
  return found ? Number(found[1]) : null;
}

/**
 * The state of one issue, the first that applies: closed is `done`; a needs-you label or a run
 * that asks for the user is `needs-you`; an issue a run works on, an open pull request for it or
 * an assignee is `in-progress`; open blockers make it `blocked`; the rest is `next`.
 */
export function issueState(issue: Issue, runs: RunContext): IssueState {
  if (issue.state === "CLOSED") return "done";
  const labels = issue.labels.map((l) => l.toLowerCase());
  if (NEEDS_YOU_LABELS.some((l) => labels.includes(l)) || runs.needsYou.has(issue.number))
    return "needs-you";
  if (runs.working.has(issue.number)) return "in-progress";
  if ((issue.blockedBy?.length ?? 0) > 0) return "blocked";
  if (issue.prs.some((p) => p.state === "OPEN") || issue.assignees.length > 0) return "in-progress";
  return "next";
}

/** The issue numbers the runs of a milestone name, for {@link issueState}. */
export function runContext(runs: readonly GroupRun[], now: number): RunContext {
  const working = new Set<number>();
  const needsYou = new Set<number>();
  for (const run of runs) {
    const live = runState(run.status, now) === "running";
    if (!live) continue;
    for (const t of run.status.tasks) {
      const nr = subjectIssue(t.subject);
      if (nr !== null) working.add(nr);
    }
    for (const a of run.status.attentions) {
      if (!a.needsYou) continue;
      const task = run.status.tasks.find((t) => t.task === a.task);
      const nr = task ? subjectIssue(task.subject) : null;
      if (nr !== null) needsYou.add(nr);
    }
  }
  return { working, needsYou };
}

/** One issue of the overview with its state. */
export interface OverviewIssue {
  issue: Issue;
  state: IssueState;
  /** Open blocking issues, for `blocked`. */
  blockedBy: number[];
  /** The pull request to show: an open one, else the merged one of a done issue. */
  pr: { number: number; url: string } | null;
  /** Sub-issues of a Feature, in the order of {@link order}. */
  children: OverviewIssue[];
}

/** Sort key of the order `deliver` picks issues in: lowest number first. */
const byNumber = (a: Issue, b: Issue): number => a.number - b.number;

function entry(issue: Issue, runs: RunContext): OverviewIssue {
  const open = issue.prs.find((p) => p.state === "OPEN");
  const merged = issue.prs.find((p) => p.state === "MERGED");
  const pr = open ?? (issue.state === "CLOSED" ? merged : undefined);
  return {
    issue,
    state: issueState(issue, runs),
    blockedBy: issue.blockedBy ?? [],
    pr: pr ? { number: pr.number, url: pr.url } : null,
    children: [],
  };
}

/**
 * The PBIs and Bugs of a milestone, Features with their sub-issues as a group. `issues` are the
 * open and closed issues of the repository; only those of `milestone` count. A sub-issue of a
 * Feature in the milestone sits under it; Epics and other levels are left out.
 */
export function milestoneIssues(
  issues: readonly Issue[],
  milestone: string,
  runs: RunContext,
): OverviewIssue[] {
  const inside = issues.filter((i) => i.milestone === milestone).sort(byNumber);
  const features = new Map<number, OverviewIssue>();
  for (const i of inside) if (issueLevel(i) === "feature") features.set(i.number, entry(i, runs));
  const top: OverviewIssue[] = [];
  for (const i of inside) {
    const level = issueLevel(i);
    if (level === "feature") continue;
    if (level !== "pbi" && level !== "bug") continue;
    const parent = i.parent ? features.get(i.parent.number) : undefined;
    (parent ? parent.children : top).push(entry(i, runs));
  }
  return [...top, ...features.values()].sort((a, b) => a.issue.number - b.issue.number);
}

/** All entries of a list, Features' sub-issues included. */
export const flatten = (list: readonly OverviewIssue[]): OverviewIssue[] =>
  list.flatMap((e) => [e, ...flatten(e.children)]);

/** How many issues are in each state, Features counted by their sub-issues (not themselves). */
export function stateCounts(list: readonly OverviewIssue[]): Record<IssueState, number> {
  const counts: Record<IssueState, number> = {
    "in-progress": 0,
    "needs-you": 0,
    blocked: 0,
    next: 0,
    done: 0,
  };
  for (const e of list) {
    if (e.children.length > 0) {
      for (const [k, n] of Object.entries(stateCounts(e.children))) counts[k as IssueState] += n;
    } else counts[e.state] += 1;
  }
  return counts;
}

/** The words of the order a milestone is delivered in: the next issues, lowest number first. */
export const nextIssues = (list: readonly OverviewIssue[]): number[] =>
  flatten(list)
    .filter((e) => e.state === "next" && e.children.length === 0)
    .map((e) => e.issue.number);

/** `M014.1` for `M014.1 Runs view` and `M014.1`; the whole lower-cased title otherwise. */
export function titleKey(title: string): string {
  const t = title.trim();
  const m = /^M\d+(?:\.\d+)?/i.exec(t);
  return (m ? m[0] : t).toLowerCase();
}

/** Whether a run subject names the milestone: by URL, else by its id or title (`M014.1`). */
export function subjectMatches(
  subject: Pick<Subject, "kind" | "id" | "title" | "url"> | null,
  milestone: Pick<Milestone, "title" | "url">,
): boolean {
  if (subject?.kind !== "github.milestone") return false;
  if (subject.url && milestone.url) return subject.url === milestone.url;
  const wanted = titleKey(milestone.title);
  return titleKey(subject.id) === wanted || titleKey(subject.title) === wanted;
}

/** The run groups of a repository on this Mac that work on the milestone. */
export const groupsOf = (
  groups: readonly RunGroup[],
  root: string,
  milestone: Pick<Milestone, "title" | "url">,
): RunGroup[] =>
  groups.filter(
    (g) => g.machine === undefined && g.root === root && subjectMatches(g.subject, milestone),
  );

/**
 * The run to show on a milestone row: the running one, else the newest finished or abandoned one
 * (across the groups, by start).
 */
export function rowRun(groups: readonly RunGroup[], now: number): GroupRun | undefined {
  for (const g of groups) {
    const running = runningRun(g, now);
    if (running) return running;
  }
  return groups
    .map((g) => g.runs[0])
    .filter((r): r is GroupRun => r !== undefined)
    .sort((a, b) => Date.parse(b.status.updated) - Date.parse(a.status.updated))[0];
}

/** One milestone row. */
export interface MilestoneRow {
  /** Unique, the key of the expanded state: `<repo>#<title>` or `<machine>:<root>#<title>`. */
  key: string;
  title: string;
  url: string | null;
  /** GitHub's counts; `null` without GitHub (only the runs are known). */
  milestone: Milestone | null;
  current: boolean;
  issues: OverviewIssue[];
  counts: Record<IssueState, number>;
  groups: RunGroup[];
  run: GroupRun | undefined;
}

/** The overview of one repository (or of a machine's repository, runs only). */
export interface RepoOverview {
  key: string;
  root: string;
  machine?: string;
  /** `owner/name` when GitHub answered or knew it. */
  repo: string | null;
  /** Why GitHub did not answer; the rows then come from the runs alone. */
  error: GhError | null;
  rows: MilestoneRow[];
}

const emptyCounts = (): Record<IssueState, number> => ({
  "in-progress": 0,
  "needs-you": 0,
  blocked: 0,
  next: 0,
  done: 0,
});

/** The title of a milestone subject without GitHub: its title, else its id. */
const subjectTitle = (s: Subject): string => s.title || s.id;

/**
 * The overview rows: per repository of `milestones` its open milestones (the current one first,
 * the others after it) with issue states from `issues` and the runs of `groups`; a repository
 * GitHub did not answer for (and every machine's) lists the milestones of its run subjects. Runs of
 * a milestone GitHub no longer lists open are left out.
 */
export function buildOverview(input: {
  milestones: readonly RepoMilestones[];
  issues: readonly RepoIssues[];
  closed: readonly RepoIssues[];
  groups: readonly RunGroup[];
  now: number;
}): RepoOverview[] {
  const { milestones, issues, closed, groups, now } = input;
  const out: RepoOverview[] = [];
  const seen = new Set<string>();
  for (const repo of milestones) {
    seen.add(repo.root);
    const runGroups = groups.filter((g) => g.machine === undefined && g.root === repo.root);
    if (repo.status === "error") {
      out.push({
        key: repo.root,
        root: repo.root,
        repo: repo.repo,
        error: repo.error,
        rows: runOnlyRows(repo.repo, runGroups, now),
      });
      continue;
    }
    const open = issues.find((r) => r.root === repo.root);
    const done = closed.find((r) => r.root === repo.root);
    const known = [
      ...(open?.status === "ok" ? open.issues : []),
      ...(done?.status === "ok" ? done.issues : []),
    ];
    const ordered = [...repo.milestones].sort((a, b) => Number(b.current) - Number(a.current));
    const rows = ordered.map((m): MilestoneRow => {
      const mine = groupsOf(runGroups, repo.root, m);
      const run = rowRun(mine, now);
      const list = milestoneIssues(
        known,
        m.title,
        runContext(
          mine.flatMap((g) => g.runs),
          now,
        ),
      );
      return {
        key: `${repo.repo}#${m.title}`,
        title: m.title,
        url: m.url,
        milestone: m,
        current: m.current,
        issues: list,
        counts: stateCounts(list),
        groups: mine,
        run,
      };
    });
    const error =
      open?.status === "error" ? open.error : done?.status === "error" ? done.error : null;
    out.push({ key: repo.root, root: repo.root, repo: repo.repo, error, rows });
  }
  // Repositories GitHub was not asked about and every machine's: the runs alone.
  const rest = new Map<string, RunGroup[]>();
  for (const g of groups) {
    if (g.subject?.kind !== "github.milestone") continue;
    if (g.machine === undefined && seen.has(g.root)) continue;
    const key = g.machine === undefined ? g.root : `${g.machine}:${g.root}`;
    rest.set(key, [...(rest.get(key) ?? []), g]);
  }
  for (const [key, list] of rest) {
    const first = list[0]!;
    out.push({
      key,
      root: first.root,
      ...(first.machine === undefined ? {} : { machine: first.machine }),
      repo: null,
      error: null,
      rows: runOnlyRows(key, list, now),
    });
  }
  return out.filter((r) => r.rows.length > 0 || r.error !== null);
}

/** Rows made of the milestone run groups alone (no GitHub), the running ones first. */
export function runOnlyRows(
  where: string,
  groups: readonly RunGroup[],
  now: number,
): MilestoneRow[] {
  return groups
    .filter((g) => g.subject?.kind === "github.milestone")
    .map((g): MilestoneRow => {
      const subject = g.subject!;
      return {
        key: `${where}#${subjectTitle(subject)}`,
        title: subjectTitle(subject),
        url: subject.url,
        milestone: null,
        current: false,
        issues: [],
        counts: emptyCounts(),
        groups: [g],
        run: rowRun([g], now),
      };
    })
    .sort(
      (a, b) =>
        Number(b.run !== undefined && runState(b.run.status, now) === "running") -
        Number(a.run !== undefined && runState(a.run.status, now) === "running"),
    );
}

/**
 * The title of a run task: its own, or for a bare `#<nr>` the title of that issue among `issues`
 * (`#12 Group runs`); the bare number stays without GitHub.
 */
export function taskTitle(task: Pick<OpenTask, "subject">, issues: readonly Issue[]): string {
  const title = task.subject.title;
  const bare = /^#(\d+)$/.exec(title.trim());
  if (!bare) return title;
  const found = issues.find((i) => i.number === Number(bare[1]));
  return found ? `#${found.number} ${found.title}` : title;
}

/** GitHub's progress of a milestone as a run progress (`closed/total`), else `null`. */
export function githubProgress(
  milestone: Pick<Milestone, "open" | "closed"> | undefined,
): { done: number; total: number; label: string } | null {
  if (!milestone) return null;
  return { done: milestone.closed, total: milestone.open + milestone.closed, label: "closed" };
}

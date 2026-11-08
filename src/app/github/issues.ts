import { Injectable, computed, inject, signal } from "@angular/core";
import { paneDir } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import { GithubFilter } from "./github-filter";
import { GithubMilestones } from "./milestones";
import { REFRESH_MIN_MS, ghErrorOf, isGithubUrl, type Checks, type GhError } from "./pulls";

/** A pull request linked to an issue (`LinkedPr` of `github_issues.rs`). */
export interface LinkedPr {
  number: number;
  url: string;
  state: "OPEN" | "MERGED" | "CLOSED";
  checks: Checks;
}

/** An issue as `github_issues` returns it (`Issue` of `github_issues.rs`). */
export interface Issue {
  number: number;
  title: string;
  url: string;
  state: "OPEN" | "CLOSED";
  labels: string[];
  assignees: string[];
  milestone: string | null;
  parent: { number: number; repo: string } | null;
  subTotal: number;
  subCompleted: number;
  prs: LinkedPr[];
}

export type RepoIssues = { root: string; repo: string } & (
  { status: "ok"; issues: Issue[]; truncated: boolean } | { status: "error"; error: GhError }
);

export type Level = "epic" | "feature" | "pbi" | "bug" | "other";

export const LEVELS: readonly Level[] = ["epic", "feature", "pbi", "bug", "other"];

export type StateFilter = "open" | "closed";

/** The level of an issue: its `epic`/`feature`/`pbi`/`bug` label, else a `[Epic]`... title prefix. */
export function issueLevel(issue: Pick<Issue, "labels" | "title">): Level {
  const labels = issue.labels.map((l) => l.toLowerCase());
  for (const level of LEVELS.slice(0, 4)) if (labels.includes(level)) return level;
  const prefix = /^\s*\[(epic|feature|pbi|bug)\]/i.exec(issue.title);
  return prefix ? (prefix[1].toLowerCase() as Level) : "other";
}

/** An issue in the tree. */
export interface IssueNode {
  issue: Issue;
  level: Level;
  children: IssueNode[];
}

/**
 * The tree of one repository from the `parent` links. A parent that is missing (closed, or not
 * read), in another repository or the issue itself does not count: the issue becomes a root. Issues
 * on a parent cycle become roots too, so every issue shows up exactly once. Siblings are ordered by
 * number.
 */
export function buildTree(repo: string, issues: readonly Issue[]): IssueNode[] {
  const byNumber = new Map(issues.map((i) => [i.number, i]));
  const parentOf = new Map<number, number>();
  for (const i of issues) {
    const p = i.parent;
    if (p && p.number !== i.number && p.repo.toLowerCase() === repo.toLowerCase()) {
      if (byNumber.has(p.number)) parentOf.set(i.number, p.number);
    }
  }
  // Follow the parents upwards; coming back to the start means a cycle.
  const onCycle: number[] = [];
  for (const i of issues) {
    let at = parentOf.get(i.number);
    for (let steps = 0; at !== undefined && steps <= issues.length; steps++) {
      if (at === i.number) {
        onCycle.push(i.number);
        break;
      }
      at = parentOf.get(at);
    }
  }
  for (const number of onCycle) parentOf.delete(number);
  const nodes = new Map<number, IssueNode>(
    issues.map((i) => [i.number, { issue: i, level: issueLevel(i), children: [] }]),
  );
  const roots: IssueNode[] = [];
  for (const [number, node] of nodes) {
    const parent = parentOf.get(number);
    (parent === undefined ? roots : nodes.get(parent)!.children).push(node);
  }
  const byNr = (a: IssueNode, b: IssueNode) => a.issue.number - b.issue.number;
  roots.sort(byNr);
  for (const node of nodes.values()) node.children.sort(byNr);
  return roots;
}

export interface IssueFilter {
  label: string | null;
  level: Level | null;
  assignee: string | null;
  /** Title of the milestone the issues must belong to. */
  milestone: string | null;
  text: string;
}

export const NO_ISSUE_FILTER: IssueFilter = {
  label: null,
  level: null,
  assignee: null,
  milestone: null,
  text: "",
};

function matches(node: IssueNode, f: IssueFilter): boolean {
  const i = node.issue;
  if (f.label && !i.labels.some((l) => l.toLowerCase() === f.label!.toLowerCase())) return false;
  if (f.level && node.level !== f.level) return false;
  if (f.assignee && !i.assignees.some((a) => a.toLowerCase() === f.assignee!.toLowerCase())) {
    return false;
  }
  if (f.milestone && i.milestone !== f.milestone) return false;
  const text = f.text.trim().toLowerCase().replace(/^#/, "");
  return !text || i.title.toLowerCase().includes(text) || String(i.number).includes(text);
}

/** The tree reduced to the matching issues and the ancestors that lead to them. */
export function filterTree(roots: readonly IssueNode[], f: IssueFilter): IssueNode[] {
  const walk = (node: IssueNode): IssueNode | null => {
    const children = node.children.flatMap((c) => walk(c) ?? []);
    return children.length || matches(node, f) ? { ...node, children } : null;
  };
  return roots.flatMap((r) => walk(r) ?? []);
}

/** A line of the rendered tree. */
export interface IssueRow {
  key: string;
  node: IssueNode;
  depth: number;
  collapsed: boolean;
}

/** Depth-first rows; the children of a collapsed node are left out. */
export function flattenTree(
  roots: readonly IssueNode[],
  isCollapsed: (number: number) => boolean,
): IssueRow[] {
  const rows: IssueRow[] = [];
  const walk = (node: IssueNode, depth: number) => {
    const collapsed = node.children.length > 0 && isCollapsed(node.issue.number);
    rows.push({ key: String(node.issue.number), node, depth, collapsed });
    if (!collapsed) for (const c of node.children) walk(c, depth + 1);
  };
  for (const r of roots) walk(r, 0);
  return rows;
}

/** The labels shown as chips: all but the level labels, which the level badge stands for. */
export function labelChips(issue: Pick<Issue, "labels">): string[] {
  return issue.labels.filter((l) => !LEVELS.includes(l.toLowerCase() as Level));
}

/** The level as a badge; the tone picks the color of the level in the stylesheet. */
export function levelBadge(level: Level): { label: string; tone: Level } {
  return { label: level, tone: level };
}

/** Sub-issue progress of an epic or feature: `done/total` and a percentage. */
export function issueProgress(
  level: Level,
  issue: Pick<Issue, "subTotal" | "subCompleted">,
): { done: number; total: number; percent: number } | null {
  if ((level !== "epic" && level !== "feature") || issue.subTotal <= 0) return null;
  const done = Math.min(issue.subCompleted, issue.subTotal);
  return { done, total: issue.subTotal, percent: Math.round((done / issue.subTotal) * 100) };
}

/** A sub-issue or the parent in an issue's detail (`RelatedIssue` of `github_issues.rs`). */
export interface RelatedIssue {
  number: number;
  title: string;
  state: string;
  /** `owner/name`. */
  repo: string;
}

/** A pull request that closes the issue (`ClosingPr` of `github_issues.rs`). */
export interface ClosingPr {
  number: number;
  title: string;
  url: string;
  state: "OPEN" | "MERGED" | "CLOSED";
  repo: string;
}

/** An issue in detail (`IssueDetail` of `github_issues.rs`). */
export interface IssueDetail {
  number: number;
  title: string;
  url: string;
  body: string;
  state: "OPEN" | "CLOSED";
  stateReason: string | null;
  author: string | null;
  labels: string[];
  assignees: string[];
  milestone: string | null;
  parent: RelatedIssue | null;
  subIssues: RelatedIssue[];
  pullRequests: ClosingPr[];
  viewerCanUpdate: boolean;
}

/** The key of an issue over all repositories: `owner/name#number`. */
export function issueKey(repo: string, number: number): string {
  return `${repo}#${number}`;
}

/** One repository ready for the view. */
export interface RepoTree {
  root: string;
  repo: string;
  error?: GhError;
  truncated: boolean;
  /** Filtered tree. */
  roots: IssueNode[];
  count: number;
}

/**
 * The issues of the repositories behind the spaces and the explorer roots (`github_issues`; the
 * backend caches for 30 s). Holds the last answer and the filters; which nodes are collapsed is UI
 * state per repository.
 */
@Injectable({ providedIn: "root" })
export class GithubIssues {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(HerdrStore);
  private readonly settings = inject(SettingsStore);
  private readonly ui = inject(UiState);
  private readonly milestones = inject(GithubMilestones);
  private readonly filter = inject(GithubFilter);

  private readonly _result = signal<RepoIssues[] | null>(null);
  private readonly _loading = signal(false);
  private readonly _error = signal<string | null>(null);
  private readonly _selected = signal<{ repo: string; number: number } | null>(null);
  private readonly _cursor = signal<string | null>(null);
  private readonly _detailKey = signal<string | null>(null);
  private readonly _detail = signal<IssueDetail | null>(null);
  private readonly _detailError = signal<GhError | string | null>(null);
  private readonly _detailLoading = signal(false);
  private last = 0;

  readonly result = this._result.asReadonly();
  readonly loading = this._loading.asReadonly();
  readonly error = this._error.asReadonly();

  /** The issue picked from elsewhere (a pull request's linked issue); highlighted in the tree. */
  readonly selected = this._selected.asReadonly();
  /** Key (`owner/name#number`) of the row the keyboard cursor is on. */
  readonly cursor = this._cursor.asReadonly();
  /** Key of the issue whose detail pane is open. */
  readonly detailKey = this._detailKey.asReadonly();
  readonly detail = this._detail.asReadonly();
  readonly detailError = this._detailError.asReadonly();
  readonly detailLoading = this._detailLoading.asReadonly();
  readonly state = signal<StateFilter>("open");
  readonly label = signal<string | null>(null);
  readonly level = signal<Level | null>(null);
  readonly assignee = signal<string | null>(null);
  readonly text = signal("");

  private readonly okRepos = computed(() =>
    (this._result() ?? []).flatMap((r) => (r.status === "ok" ? [r] : [])),
  );
  /** Labels and assignees present in the data, for the filter lists. */
  readonly labelNames = computed(() =>
    [...new Set(this.okRepos().flatMap((r) => r.issues.flatMap((i) => i.labels)))].sort(),
  );
  readonly assigneeNames = computed(() =>
    [...new Set(this.okRepos().flatMap((r) => r.issues.flatMap((i) => i.assignees)))].sort(),
  );
  readonly errors = computed(() =>
    (this._result() ?? []).flatMap((r) =>
      r.status === "error" ? [{ repo: r.repo, error: r.error }] : [],
    ),
  );

  readonly trees = computed<RepoTree[]>(() => {
    const base: IssueFilter = {
      label: this.label(),
      level: this.level(),
      assignee: this.assignee(),
      milestone: null,
      text: this.text(),
    };
    return this.okRepos()
      .filter((r) => this.filter.allows(r.repo))
      .map((r) => {
        // The selected milestone filters the tree of its own repository.
        const filter = { ...base, milestone: this.milestones.forRepo(r.repo) };
        const roots = filterTree(buildTree(r.repo, r.issues), filter);
        return {
          root: r.root,
          repo: r.repo,
          truncated: r.truncated,
          roots,
          count: flattenTree(roots, () => false).length,
        };
      })
      .filter((t) => t.count > 0 || this.ui.githubShowEmpty());
  });

  /** Issues shown after all filters, over the repositories that pass the repository filter. */
  readonly count = computed(() => this.trees().reduce((n, t) => n + t.count, 0));

  /** Repositories that were read, for the shared repository filter. */
  readonly repoNames = computed(() => (this._result() ?? []).map((r) => r.repo));

  /** The issue of the open detail pane as the list knows it (absent when filtered out). */
  readonly detailIssue = computed(() => {
    const key = this._detailKey();
    if (key === null) return null;
    for (const r of this.okRepos()) {
      const issue = r.issues.find((i) => issueKey(r.repo, i.number) === key);
      if (issue) return { repo: r.repo, issue };
    }
    return null;
  });

  /** The visible rows over all repositories, in screen order. */
  private readonly visibleKeys = computed(() =>
    this.trees().flatMap((t) => this.rows(t).map((r) => issueKey(t.repo, r.node.issue.number))),
  );

  /** The visible rows of one repository. */
  rows(tree: RepoTree): IssueRow[] {
    return flattenTree(tree.roots, (n) => this.isCollapsed(tree.repo, n));
  }

  select(repo: string, number: number): void {
    this._selected.set({ repo, number });
    this._cursor.set(issueKey(repo, number));
  }

  /** Moves the keyboard cursor by `delta` rows within the visible rows (clamped). */
  moveCursor(delta: number): void {
    const keys = this.visibleKeys();
    if (!keys.length) return;
    const at = this._cursor() === null ? -1 : keys.indexOf(this._cursor()!);
    const next = Math.min(
      keys.length - 1,
      Math.max(0, at < 0 ? (delta > 0 ? 0 : keys.length - 1) : at + delta),
    );
    this._cursor.set(keys[next]);
  }

  /** Opens the detail pane of the cursor row, if any. */
  openCursor(): void {
    const key = this._cursor();
    const at = key === null ? -1 : key.lastIndexOf("#");
    if (key !== null && at > 0) void this.openDetail(key.slice(0, at), Number(key.slice(at + 1)));
  }

  /**
   * Opens the detail pane of an issue and reads its detail (the backend caches for 30 s). A
   * repository the list did not read has no directory to ask, so nothing opens.
   */
  async openDetail(repo: string, number: number): Promise<void> {
    const root = (this._result() ?? []).find(
      (r) => r.repo.toLowerCase() === repo.toLowerCase(),
    )?.root;
    if (root === undefined) return;
    const key = issueKey(repo, number);
    this._selected.set({ repo, number });
    this._cursor.set(key);
    this._detailKey.set(key);
    this._detail.set(null);
    this._detailError.set(null);
    this._detailLoading.set(true);
    try {
      const detail = await this.bridge.invoke<IssueDetail>("github_issue_detail", {
        root,
        number,
      });
      if (this._detailKey() === key) this._detail.set(detail);
    } catch (err) {
      if (this._detailKey() === key) this._detailError.set(ghErrorOf(err));
    } finally {
      if (this._detailKey() === key) this._detailLoading.set(false);
    }
  }

  closeDetail(): void {
    this._detailKey.set(null);
    this._detail.set(null);
    this._detailError.set(null);
    this._detailLoading.set(false);
  }

  /** True when `repo` was read, so its issues can open in the pane. */
  knows(repo: string): boolean {
    return (this._result() ?? []).some((r) => r.repo.toLowerCase() === repo.toLowerCase());
  }

  isCollapsed(repo: string, number: number): boolean {
    return this.ui.collapsedIssues()[repo]?.includes(number) ?? false;
  }

  toggle(repo: string, number: number): void {
    this.ui.setIssueCollapsed(repo, number, !this.isCollapsed(repo, number));
  }

  /** Switching between open and closed issues reads again. */
  async setState(state: StateFilter): Promise<void> {
    if (state === this.state()) return;
    this.state.set(state);
    await this.load();
  }

  async load(force = true): Promise<void> {
    if (this._loading() || (!force && Date.now() - this.last < REFRESH_MIN_MS)) return;
    this._loading.set(true);
    this.last = Date.now();
    try {
      this._result.set(
        await this.bridge.invoke<RepoIssues[]>("github_issues", {
          dirs: this.dirs(),
          closed: this.state() === "closed",
        }),
      );
      this._error.set(null);
    } catch (err) {
      this._error.set(String(err));
    } finally {
      this._loading.set(false);
    }
  }

  /** Opens an issue or pull request in the browser; only https://github.com/ links. */
  async openUrl(url: string): Promise<void> {
    if (isGithubUrl(url)) await this.bridge.openUrl(url);
  }

  private dirs(): string[] {
    const found = new Set<string>();
    for (const pane of this.store.snapshot().terminals) {
      const dir = paneDir(pane);
      if (dir) found.add(dir);
    }
    for (const root of this.settings.settings().explorerRoots) found.add(root);
    return [...found];
  }
}

import { Injectable, computed, inject, signal } from "@angular/core";
import { paneDir } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import { GithubFilter } from "./github-filter";
import { GithubMilestones } from "./milestones";
import { REFRESH_MIN_MS, isGithubUrl, type Checks, type GhError } from "./pulls";

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

/** Labels shown as a bracketed marker on the row. */
export const MARKER_LABELS = ["needs-refinement", "needs-decision"] as const;

export function markers(issue: Issue): string[] {
  return MARKER_LABELS.filter((m) => issue.labels.some((l) => l.toLowerCase() === m));
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
  private last = 0;

  readonly result = this._result.asReadonly();
  readonly loading = this._loading.asReadonly();
  readonly error = this._error.asReadonly();

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
      });
  });

  /** Issues shown after all filters, over the repositories that pass the repository filter. */
  readonly count = computed(() => this.trees().reduce((n, t) => n + t.count, 0));

  /** Repositories that were read, for the shared repository filter. */
  readonly repoNames = computed(() => (this._result() ?? []).map((r) => r.repo));

  /** The visible rows of one repository. */
  rows(tree: RepoTree): IssueRow[] {
    return flattenTree(tree.roots, (n) => this.isCollapsed(tree.repo, n));
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

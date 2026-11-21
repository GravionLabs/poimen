import { Injectable, computed, inject, signal } from "@angular/core";
import { paneDir } from "../core/snapshot";
import { SpaceActions } from "../core/space-actions";
import { SpacesStore } from "../core/spaces-store";
import { SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import { GithubFilter } from "./github-filter";
import { GithubMilestones } from "./milestones";
import {
  REFRESH_MIN_MS,
  errorText,
  ghErrorOf,
  isGithubUrl,
  keyRange,
  type Checks,
  type GhError,
} from "./pulls";

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
  /** Numbers of the open issues that block this one (native dependencies). */
  blockedBy?: number[];
  prs: LinkedPr[];
}

export type RepoIssues = { root: string; repo: string } & (IssuesLoaded | IssuesFailed);
type IssuesLoaded = { status: "ok"; issues: Issue[]; truncated: boolean };
type IssuesFailed = { status: "error"; error: GhError };

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

/** The level a sub-issue of `parent` gets: Epic gets Feature, Feature gets PBI, else none. */
export function childLevel(parent: Level): Level | null {
  return parent === "epic" ? "feature" : parent === "feature" ? "pbi" : null;
}

/** The title prefix of a level (`[Feature] `), empty for none. */
export function levelPrefix(level: Level | null): string {
  return level === null || level === "other" ? "" : `[${level[0].toUpperCase()}${level.slice(1)}] `;
}

/** The form for a new issue (#341): level label and title prefix follow the parent, both editable. */
export interface IssueForm {
  repo: string;
  root: string;
  parent: { number: number; title: string } | null;
  title: string;
  body: string;
  labels: string[];
  milestone: string | null;
  /** `body` shown rendered. */
  previewing: boolean;
}

/** A blank form, or the one for a sub-issue of `parent` with the level of its children. */
export function newIssueForm(
  repo: string,
  root: string,
  parent: Pick<Issue, "number" | "title" | "labels"> | null,
): IssueForm {
  const level = parent ? childLevel(issueLevel(parent)) : null;
  return {
    repo,
    root,
    parent: parent ? { number: parent.number, title: parent.title } : null,
    title: levelPrefix(level),
    body: "",
    labels: level && level !== "other" ? [level] : [],
    milestone: null,
    previewing: false,
  };
}

/** Whether the form can be sent: a title beyond the bare prefix. */
export function formReady(form: Pick<IssueForm, "title">): boolean {
  return form.title.replace(/^\s*\[[^\]]*\]/, "").trim() !== "";
}

/**
 * Issues that can become the parent of `number`: not itself, not its current parent, not one of
 * its descendants; matched by number (`12`, `#12`) or title, at most `limit`.
 */
export function parentCandidates(
  issues: readonly Issue[],
  number: number,
  currentParent: number | null,
  query: string,
  limit = 8,
): Issue[] {
  const below = new Set<number>([number]);
  for (let grew = true; grew;) {
    grew = false;
    for (const i of issues) {
      if (i.parent && below.has(i.parent.number) && !below.has(i.number)) {
        below.add(i.number);
        grew = true;
      }
    }
  }
  const q = query.trim().toLowerCase().replace(/^#/, "");
  return issues
    .filter(
      (i) =>
        !below.has(i.number) &&
        i.number !== currentParent &&
        (q === "" || String(i.number) === q || i.title.toLowerCase().includes(q)),
    )
    .slice(0, limit);
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
  /** Logins the assignee picker offers. */
  assignableUsers: string[];
}

/** What `github_issue_edit` changes (`IssueEdit` of `github_issues.rs`). */
export interface IssueEdit {
  title?: string;
  body?: string;
  expectedBody?: string;
  overwrite?: boolean;
  addLabels?: string[];
  removeLabels?: string[];
  addAssignees?: string[];
  removeAssignees?: string[];
  milestone?: string;
  removeMilestone?: boolean;
}

/** A state change of an issue (`IssueAction` of `github_issues.rs`). */
export type IssueAction = "completed" | "not_planned" | "reopen";

export const ISSUE_ACTION_LABELS: Record<IssueAction, string> = {
  completed: "Close as completed",
  not_planned: "Close as not planned",
  reopen: "Reopen",
};

/** Whether the edit controls are shown: only when GitHub says the viewer may update the issue. */
export function canEditIssue(detail: Pick<IssueDetail, "viewerCanUpdate">): boolean {
  return detail.viewerCanUpdate;
}

/** The key of an issue over all repositories: `owner/name#number`. */
export function issueKey(repo: string, number: number): string {
  return `${repo}#${number}`;
}

/** The outcome of a bulk action: issue numbers done, and failed with the reason. */
export interface BulkResult {
  done: number[];
  failed: { number: number; error: string }[];
}

/** The one notification of a bulk action. */
export function bulkSummary(what: string, r: BulkResult): string {
  const nums = (ns: number[]) => ns.map((n) => `#${n}`).join(", ");
  let text = `Bulk ${what}: ${r.done.length} done`;
  if (r.done.length) text += ` (${nums(r.done)})`;
  if (r.failed.length) {
    text += `, ${r.failed.length} failed (${r.failed.map((f) => `#${f.number}: ${f.error}`).join("; ")})`;
  }
  return text;
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
  private readonly store = inject(SpacesStore);
  private readonly settings = inject(SettingsStore);
  private readonly ui = inject(UiState);
  private readonly milestones = inject(GithubMilestones);
  private readonly filter = inject(GithubFilter);
  private readonly spaces = inject(SpaceActions);

  private readonly _result = signal<RepoIssues[] | null>(null);
  private readonly _loading = signal(false);
  private readonly _error = signal<string | null>(null);
  private readonly _selected = signal<{ repo: string; number: number } | null>(null);
  private readonly _cursor = signal<string | null>(null);
  private readonly _marked = signal<ReadonlySet<string>>(new Set());
  private readonly _bulkBusy = signal(false);
  private anchor: string | null = null;
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

  /** Keys (`owner/name#number`) of the issues marked for a bulk action (one repository). */
  readonly marked = this._marked.asReadonly();
  readonly bulkBusy = this._bulkBusy.asReadonly();

  /** The repository of the marked issues; null without a selection. */
  readonly markedRepo = computed(() => {
    const first = [...this._marked()][0];
    return first === undefined ? null : first.slice(0, first.lastIndexOf("#"));
  });

  /** The marked issues that pass the filters, in screen order, with their repository root. */
  readonly markedIssues = computed(() => {
    const keys = this._marked();
    return this.trees().flatMap((t) =>
      this.rows(t).flatMap((r) =>
        keys.has(issueKey(t.repo, r.node.issue.number))
          ? [{ repo: t.repo, root: t.root, issue: r.node.issue }]
          : [],
      ),
    );
  });

  /** Whether the row cannot be marked: a selection exists in another repository. */
  markDisabled(repo: string): boolean {
    const at = this.markedRepo();
    return at !== null && at !== repo;
  }

  /**
   * Adds or removes one issue from the bulk selection; with `range` marks everything since the
   * anchor. The selection stays in one repository: a key of another one is ignored.
   */
  mark(key: string, mode: "toggle" | "range" = "toggle"): void {
    const repo = key.slice(0, key.lastIndexOf("#"));
    if (this.markDisabled(repo)) return;
    this._cursor.set(key);
    if (mode === "range") {
      const same = this.visibleKeys().filter((k) => k.startsWith(repo + "#"));
      const keys = keyRange(same, this.anchor, key);
      this._marked.update((s) => new Set([...s, ...keys]));
      return;
    }
    this._marked.update((s) => {
      const next = new Set(s);
      if (!next.delete(key)) next.add(key);
      return next;
    });
    this.anchor = key;
  }

  clearMarked(): void {
    this._marked.set(new Set());
    this.anchor = null;
  }

  /** Sets the milestone, or adds or removes a label, on every marked issue. */
  bulkEdit(
    change: Pick<IssueEdit, "milestone" | "addLabels" | "removeLabels">,
  ): Promise<BulkResult> {
    const text = change.milestone
      ? `milestone ${change.milestone}`
      : change.addLabels?.length
        ? `add label ${change.addLabels.join(", ")}`
        : `remove label ${(change.removeLabels ?? []).join(", ")}`;
    return this.bulk(text, (root, number) =>
      this.bridge.invoke("github_issue_edit", { root, number, edit: change }),
    );
  }

  /** Closes every marked issue with the reason. */
  bulkClose(reason: "completed" | "not_planned"): Promise<BulkResult> {
    return this.bulk(ISSUE_ACTION_LABELS[reason].toLowerCase(), (root, number) =>
      this.bridge.invoke("github_issue_action", { root, number, action: reason }),
    );
  }

  /**
   * One request per marked issue, in order; a failure does not stop the rest. One notification
   * lists the numbers done and failed; the selection is cleared and the tree read again.
   */
  private async bulk(
    what: string,
    run: (root: string, number: number) => Promise<unknown>,
  ): Promise<BulkResult> {
    const targets = this.markedIssues();
    const result: BulkResult = { done: [], failed: [] };
    if (targets.length === 0 || this._bulkBusy()) return result;
    this._bulkBusy.set(true);
    try {
      for (const t of targets) {
        try {
          await run(t.root, t.issue.number);
          result.done.push(t.issue.number);
        } catch (err) {
          const e = ghErrorOf(err);
          result.failed.push({
            number: t.issue.number,
            error: typeof e === "string" ? e : errorText(e),
          });
        }
      }
    } finally {
      this._bulkBusy.set(false);
    }
    this.spaces.notify(bulkSummary(what, result));
    this.clearMarked();
    await this.load(true);
    return result;
  }

  /** Labels and milestone titles of the marked repository, for the bulk bar. */
  readonly bulkLabels = signal<string[]>([]);
  readonly bulkMilestones = signal<string[]>([]);

  /** Reads the labels and milestones of the marked repository (called by the bar). */
  async loadBulkOptions(): Promise<void> {
    const repo = this.markedRepo();
    const root = (this._result() ?? []).find((r) => r.repo === repo)?.root;
    if (!root) return;
    const [labels, milestones] = await Promise.all([
      this.bridge.invoke<string[]>("github_labels", { root }).catch(() => []),
      this.bridge
        .invoke<{ status: string; milestones?: { title: string }[] }[]>("github_milestones", {
          dirs: [root],
        })
        .catch(() => []),
    ]);
    this.bulkLabels.set(labels ?? []);
    this.bulkMilestones.set(
      (milestones ?? [])
        .flatMap((r) => (r.status === "ok" ? (r.milestones ?? []) : []))
        .map((m) => m.title),
    );
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
    this.resetEdit();
    this.parentQuery.set(null);
    this._detailKey.set(key);
    this._detail.set(null);
    this._detailError.set(null);
    this._detailLoading.set(true);
    try {
      const detail = await this.bridge.invoke<IssueDetail>("github_issue_detail", {
        root,
        number,
      });
      if (this._detailKey() === key) {
        this._detail.set(detail);
        if (detail.viewerCanUpdate) void this.loadOptions(key, root);
      }
    } catch (err) {
      if (this._detailKey() === key) this._detailError.set(ghErrorOf(err));
    } finally {
      if (this._detailKey() === key) this._detailLoading.set(false);
    }
  }

  closeDetail(): void {
    this.resetEdit();
    this._detailKey.set(null);
    this._detail.set(null);
    this._detailError.set(null);
    this._detailLoading.set(false);
  }

  /** Label names and milestone titles of the repository of the open issue, for the pickers. */
  readonly labelOptions = signal<string[]>([]);
  readonly milestoneOptions = signal<string[]>([]);
  /** The new-issue form, null while closed. */
  readonly form = signal<IssueForm | null>(null);
  /** The text of the "Change parent" search, null while the picker is closed. */
  readonly parentQuery = signal<string | null>(null);

  /** Opens the form for a top-level issue of `repo`, or a sub-issue of `parent`. */
  openForm(repo: string, parent: number | null = null): void {
    const r = this.okRepos().find((x) => x.repo === repo);
    if (!r) return;
    const p = parent === null ? null : (r.issues.find((i) => i.number === parent) ?? null);
    if (parent !== null && !p) return;
    this.form.set(newIssueForm(repo, r.root, p));
    void this.loadFormOptions(repo, r.root);
  }

  closeForm(): void {
    this.form.set(null);
  }

  patchForm(change: Partial<IssueForm>): void {
    this.form.update((f) => (f ? { ...f, ...change } : f));
  }

  private async loadFormOptions(repo: string, root: string): Promise<void> {
    const [labels, milestones] = await Promise.all([
      this.bridge.invoke<string[]>("github_labels", { root }).catch(() => []),
      this.bridge
        .invoke<{ status: string; milestones?: { title: string }[] }[]>("github_milestones", {
          dirs: [root],
        })
        .catch(() => []),
    ]);
    if (this.form()?.repo !== repo) return;
    this.labelOptions.set(labels ?? []);
    this.milestoneOptions.set(
      (milestones ?? [])
        .flatMap((r) => (r.status === "ok" ? (r.milestones ?? []) : []))
        .map((m) => m.title),
    );
  }

  /** Creates the issue, then selects it and opens its pane. A failure is a notification. */
  async submitForm(): Promise<boolean> {
    const f = this.form();
    if (!f || !formReady(f) || this._busy()) return false;
    this._busy.set(true);
    let made: { number: number; url: string };
    try {
      made = await this.bridge.invoke("github_issue_create", {
        root: f.root,
        issue: {
          title: f.title.trim(),
          body: f.body,
          labels: f.labels,
          milestone: f.milestone,
          parent: f.parent?.number ?? null,
        },
      });
    } catch (err) {
      const e = ghErrorOf(err);
      this.spaces.fail(`Create issue: ${typeof e === "string" ? e : errorText(e)}`);
      return false;
    } finally {
      this._busy.set(false);
    }
    this.form.set(null);
    await this.load(true);
    await this.openDetail(f.repo, made.number);
    return true;
  }

  /** Moves the open issue under `newParent` (an issue of the same repository). */
  async changeParent(newParent: number): Promise<boolean> {
    const d = this._detail();
    if (!d || !canEditIssue(d)) return false;
    const old =
      d.parent && d.parent.repo.toLowerCase() === this.detailRepo()?.toLowerCase()
        ? d.parent.number
        : null;
    this.parentQuery.set(null);
    return this.write("Change parent of", (root) =>
      this.bridge.invoke("github_issue_change_parent", {
        root,
        number: d.number,
        oldParent: old,
        newParent,
      }),
    );
  }

  private detailRepo(): string | null {
    const key = this._detailKey();
    return key === null ? null : key.slice(0, key.lastIndexOf("#"));
  }

  /** Issues the open one can move under (see `parentCandidates`). */
  parentOptions(): Issue[] {
    const q = this.parentQuery();
    const sel = this.detailIssue();
    const d = this._detail();
    if (q === null || !sel || !d) return [];
    const all = this.okRepos().find((r) => r.repo === sel.repo)?.issues ?? [];
    return parentCandidates(all, d.number, d.parent?.number ?? null, q);
  }

  /** The title being edited, null while it is not. */
  readonly titleDraft = signal<string | null>(null);
  /** The description being edited, null while it is not; `previewing` shows it rendered. */
  readonly bodyDraft = signal<string | null>(null);
  readonly previewing = signal(false);
  /** The body the draft started from; a save is checked against it, even after a reload. */
  private bodyBase = "";
  /** The body on GitHub when it changed since the pane loaded it (Overwrite or Reload). */
  readonly conflict = signal<string | null>(null);
  private readonly _busy = signal(false);
  readonly busy = this._busy.asReadonly();

  private resetEdit(): void {
    this.titleDraft.set(null);
    this.bodyDraft.set(null);
    this.previewing.set(false);
    this.conflict.set(null);
    this.labelOptions.set([]);
    this.milestoneOptions.set([]);
  }

  private async loadOptions(key: string, root: string): Promise<void> {
    const [labels, milestones] = await Promise.all([
      this.bridge.invoke<string[]>("github_labels", { root }).catch(() => []),
      this.bridge
        .invoke<{ status: string; milestones?: { title: string }[] }[]>("github_milestones", {
          dirs: [root],
        })
        .catch(() => []),
    ]);
    if (this._detailKey() !== key) return;
    this.labelOptions.set(labels ?? []);
    this.milestoneOptions.set(
      (milestones ?? [])
        .flatMap((r) => (r.status === "ok" ? (r.milestones ?? []) : []))
        .map((m) => m.title),
    );
  }

  startTitle(): void {
    const d = this._detail();
    if (d && canEditIssue(d)) this.titleDraft.set(d.title);
  }

  cancelTitle(): void {
    this.titleDraft.set(null);
  }

  /** Writes the title; a blank or unchanged one is not sent. */
  async saveTitle(): Promise<boolean> {
    const d = this._detail();
    const title = this.titleDraft()?.trim();
    if (!d || !canEditIssue(d) || title === undefined) return false;
    if (title === "" || title === d.title) {
      this.titleDraft.set(null);
      return false;
    }
    const done = await this.write("Edit", (root) => this.edit(root, d, { title }));
    if (done) this.titleDraft.set(null);
    return done;
  }

  startBody(): void {
    const d = this._detail();
    if (!d || !canEditIssue(d)) return;
    this.conflict.set(null);
    this.previewing.set(false);
    this.bodyBase = d.body;
    this.bodyDraft.set(d.body);
  }

  cancelBody(): void {
    this.bodyDraft.set(null);
    this.previewing.set(false);
    this.conflict.set(null);
  }

  /**
   * Writes the description. The backend re-reads it first: when it changed on GitHub since the pane
   * loaded it, nothing is written and `conflict` holds the current text until the user picks
   * Overwrite (`overwrite`) or Reload (`reloadBody`).
   */
  async saveBody(overwrite = false): Promise<boolean> {
    const d = this._detail();
    const body = this.bodyDraft();
    if (!d || !canEditIssue(d) || body === null) return false;
    const done = await this.write("Edit", (root) =>
      this.edit(root, d, { body, expectedBody: this.bodyBase, overwrite }),
    );
    if (done) this.cancelBody();
    return done;
  }

  /** Takes the text found on GitHub as the new baseline and drops the draft. */
  reloadBody(): void {
    const current = this.conflict();
    if (current === null) return;
    this._detail.update((d) => (d ? { ...d, body: current } : d));
    this.cancelBody();
  }

  /** Adds and removes labels, assignees or the milestone; nothing to change is not sent. */
  async editMeta(change: IssueEdit): Promise<boolean> {
    const d = this._detail();
    if (!d || !canEditIssue(d)) return false;
    if (!Object.values(change).some((v) => (Array.isArray(v) ? v.length > 0 : Boolean(v)))) {
      return false;
    }
    return this.write("Edit", (root) => this.edit(root, d, change));
  }

  /** Closes with a reason or reopens. */
  async act(action: IssueAction): Promise<boolean> {
    const d = this._detail();
    if (!d || !canEditIssue(d)) return false;
    if ((action === "reopen") !== (d.state === "CLOSED")) return false;
    return this.write(ISSUE_ACTION_LABELS[action], (root) =>
      this.bridge.invoke("github_issue_action", { root, number: d.number, action }),
    );
  }

  private async edit(root: string, d: IssueDetail, change: IssueEdit): Promise<boolean> {
    const out = await this.bridge.invoke<
      { status: "saved" } | { status: "conflict"; current: string }
    >("github_issue_edit", { root, number: d.number, edit: change });
    if (out.status === "conflict") {
      this.conflict.set(out.current);
      return false;
    }
    return true;
  }

  /** Runs one write, then reloads the tree and the pane; a failure is a notification. */
  private async write(
    label: string,
    fn: (root: string) => Promise<boolean | unknown>,
  ): Promise<boolean> {
    const key = this._detailKey();
    const at = key === null ? -1 : key.lastIndexOf("#");
    if (key === null || at < 0 || this._busy()) return false;
    const repo = key.slice(0, at);
    const number = Number(key.slice(at + 1));
    const root = (this._result() ?? []).find(
      (r) => r.repo.toLowerCase() === repo.toLowerCase(),
    )?.root;
    if (root === undefined) return false;
    this._busy.set(true);
    try {
      if ((await fn(root)) === false) return false;
    } catch (err) {
      const e = ghErrorOf(err);
      this.spaces.fail(`${label} #${number}: ${typeof e === "string" ? e : errorText(e)}`);
      return false;
    } finally {
      this._busy.set(false);
    }
    await this.load(true);
    if (this._detailKey() === key) {
      const draft = { title: this.titleDraft(), body: this.bodyDraft() };
      await this.openDetail(repo, number);
      this.titleDraft.set(draft.title);
      this.bodyDraft.set(draft.body);
    }
    return true;
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
      const known = new Set(
        this.okRepos().flatMap((r) => r.issues.map((i) => issueKey(r.repo, i.number))),
      );
      this._marked.update((s) => new Set([...s].filter((k) => known.has(k))));
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
